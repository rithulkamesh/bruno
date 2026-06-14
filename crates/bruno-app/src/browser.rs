//! Headless in-app web browser for the agent.
//!
//! A `WKWebView` lives on the main thread (driven from the winit loop) and
//! renders pages with full JavaScript. The agent (on a worker thread) talks to
//! it through a channel: it sends a URL + a JS snippet to evaluate, and gets the
//! result back over a oneshot. No window is shown.

use std::sync::mpsc::{Receiver, Sender};

use tokio::sync::oneshot;

/// One browse request: load `url`, evaluate `js`, return its string result.
pub struct Job {
    url: String,
    js: String,
    reply: oneshot::Sender<Result<String, String>>,
}

/// Send-safe handle the agent holds; implements [`bruno_ai::Browser`].
pub struct BrowserHandle {
    tx: Sender<Job>,
}

pub fn channel() -> (BrowserHandle, Receiver<Job>) {
    let (tx, rx) = std::sync::mpsc::channel();
    (BrowserHandle { tx }, rx)
}

impl BrowserHandle {
    async fn run(&self, url: String, js: String) -> Result<String, bruno_ai::AiError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Job { url, js, reply })
            .map_err(|_| bruno_ai::AiError::Request("browser unavailable".into()))?;
        match rx.await {
            Ok(Ok(s)) => Ok(s),
            Ok(Err(e)) => Err(bruno_ai::AiError::Request(e)),
            Err(_) => Err(bruno_ai::AiError::Request("browser dropped".into())),
        }
    }
}

#[async_trait::async_trait]
impl bruno_ai::Browser for BrowserHandle {
    async fn fetch(&self, url: &str) -> Result<String, bruno_ai::AiError> {
        let js = "(document.body && document.body.innerText) ? document.body.innerText : ''";
        self.run(url.to_string(), js.to_string()).await
    }

    async fn search(&self, query: &str) -> Result<String, bruno_ai::AiError> {
        let url = format!("https://www.bing.com/search?q={}", encode(query));
        let raw = self.run(url, SEARCH_JS.to_string()).await?;
        Ok(format_results(&raw))
    }
}

const SEARCH_JS: &str = r#"(function(){
  try {
    var out = Array.prototype.slice.call(document.querySelectorAll('#b_results > li.b_algo'), 0, 6).map(function(el){
      var a = el.querySelector('h2 a');
      var p = el.querySelector('.b_caption p') || el.querySelector('p');
      return { title: a ? a.innerText : '', url: a ? a.href : '', snippet: p ? p.innerText : '' };
    }).filter(function(x){ return x.title && x.url; });
    return JSON.stringify(out);
  } catch (e) { return '[]'; }
})()"#;

fn format_results(raw: &str) -> String {
    #[derive(serde::Deserialize)]
    struct Hit {
        title: String,
        url: String,
        #[serde(default)]
        snippet: String,
    }
    match serde_json::from_str::<Vec<Hit>>(raw) {
        Ok(hits) if !hits.is_empty() => hits
            .iter()
            .enumerate()
            .map(|(i, h)| format!("{}. {}\n   {}\n   {}", i + 1, h.title, h.url, h.snippet))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Percent-encode a query string for a URL.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ---- main-thread driver --------------------------------------------------

#[cfg(target_os = "macos")]
pub use driver::Driver;

#[cfg(target_os = "macos")]
mod driver {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::sync::mpsc::Receiver;
    use std::time::{Duration, Instant};

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_foundation::{NSError, NSPoint, NSRect, NSSize, NSString, NSURLRequest, NSURL};
    use objc2_web_kit::{WKWebView, WKWebViewConfiguration};

    use super::Job;

    const MIN_LOAD: Duration = Duration::from_millis(300);
    const SETTLE: Duration = Duration::from_millis(800);
    const TIMEOUT: Duration = Duration::from_secs(20);

    enum Phase {
        Loading,
        Settle(Instant),
        Evaluating,
    }

    struct InFlight {
        reply: Option<tokio::sync::oneshot::Sender<Result<String, String>>>,
        js: String,
        started: Instant,
        phase: Phase,
        done: Rc<Cell<bool>>,
        result: Rc<RefCell<Option<Result<String, String>>>>,
        /// Re-evaluation budget for client-rendered pages that aren't ready yet.
        retries: u8,
    }

    pub struct Driver {
        rx: Receiver<Job>,
        webview: Retained<WKWebView>,
        inflight: Option<InFlight>,
    }

    impl Driver {
        pub fn new(rx: Receiver<Job>, mtm: MainThreadMarker) -> Self {
            let config = unsafe { WKWebViewConfiguration::new(mtm) };
            let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1280.0, 1024.0));
            let webview = unsafe {
                WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &config)
            };
            Self {
                rx,
                webview,
                inflight: None,
            }
        }

        /// Call once per frame from the main-thread event loop.
        pub fn pump(&mut self) {
            if self.inflight.is_none() {
                let Ok(job) = self.rx.try_recv() else {
                    return;
                };
                match NSURL::URLWithString(&NSString::from_str(&job.url)) {
                    Some(url) => {
                        let req = NSURLRequest::requestWithURL(&url);
                        unsafe { self.webview.loadRequest(&req) };
                        self.inflight = Some(InFlight {
                            reply: Some(job.reply),
                            js: job.js,
                            started: Instant::now(),
                            phase: Phase::Loading,
                            done: Rc::new(Cell::new(false)),
                            result: Rc::new(RefCell::new(None)),
                            retries: 6,
                        });
                    }
                    None => {
                        let _ = job.reply.send(Err("bad url".into()));
                    }
                }
                return;
            }

            enum Action {
                Wait,
                Settle,
                Evaluate,
                Finish,
                Timeout,
            }

            let action = {
                let f = self.inflight.as_ref().unwrap();
                if f.started.elapsed() > TIMEOUT {
                    Action::Timeout
                } else {
                    match f.phase {
                        Phase::Loading => {
                            if f.started.elapsed() > MIN_LOAD
                                && !unsafe { self.webview.isLoading() }
                            {
                                Action::Settle
                            } else {
                                Action::Wait
                            }
                        }
                        Phase::Settle(t) => {
                            if t.elapsed() > SETTLE {
                                Action::Evaluate
                            } else {
                                Action::Wait
                            }
                        }
                        Phase::Evaluating => {
                            if f.done.get() {
                                Action::Finish
                            } else {
                                Action::Wait
                            }
                        }
                    }
                }
            };

            match action {
                Action::Wait => {}
                Action::Settle => {
                    self.inflight.as_mut().unwrap().phase = Phase::Settle(Instant::now());
                }
                Action::Evaluate => {
                    let (js, result, done) = {
                        let f = self.inflight.as_ref().unwrap();
                        (f.js.clone(), f.result.clone(), f.done.clone())
                    };
                    let block = RcBlock::new(move |res: *mut AnyObject, err: *mut NSError| {
                        *result.borrow_mut() = Some(extract(res, err));
                        done.set(true);
                    });
                    unsafe {
                        self.webview.evaluateJavaScript_completionHandler(
                            &NSString::from_str(&js),
                            Some(&block),
                        );
                    }
                    self.inflight.as_mut().unwrap().phase = Phase::Evaluating;
                }
                Action::Finish => {
                    // If the page returned nothing yet (client-rendered), wait and
                    // re-evaluate until the budget runs out.
                    let empty = {
                        let f = self.inflight.as_ref().unwrap();
                        matches!(f.result.borrow().as_ref(), Some(Ok(s)) if s.trim().is_empty())
                    };
                    let f = self.inflight.as_mut().unwrap();
                    if empty && f.retries > 0 {
                        f.retries -= 1;
                        f.done.set(false);
                        *f.result.borrow_mut() = None;
                        f.phase = Phase::Settle(Instant::now());
                    } else {
                        let mut f = self.inflight.take().unwrap();
                        let r = f.result.borrow_mut().take().unwrap_or_else(|| Ok(String::new()));
                        if let Some(tx) = f.reply.take() {
                            let _ = tx.send(r);
                        }
                    }
                }
                Action::Timeout => {
                    let mut f = self.inflight.take().unwrap();
                    if let Some(tx) = f.reply.take() {
                        let _ = tx.send(Err("page load timed out".into()));
                    }
                }
            }
        }
    }

    fn extract(res: *mut AnyObject, err: *mut NSError) -> Result<String, String> {
        if !err.is_null() {
            let desc = unsafe { (*err).localizedDescription() };
            return Err(desc.to_string());
        }
        if res.is_null() {
            return Ok(String::new());
        }
        let obj = unsafe { Retained::retain(res) };
        match obj.and_then(|o| o.downcast::<NSString>().ok()) {
            Some(s) => Ok(s.to_string()),
            None => Ok(String::new()),
        }
    }
}
