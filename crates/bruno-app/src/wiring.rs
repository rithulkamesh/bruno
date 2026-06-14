//! Event wiring: daemon/voice → mood, HUD, AI.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use bruno_core::{ActivityEvent, BrunoBus, Intent, OrbMood, VoiceEvent};
use tokio::sync::Mutex as AsyncMutex;
use tracing::info;

use crate::calendar;
use crate::commands::{HudCommand, MoodCommand};
use crate::nudge::{NudgePolicy, local_minutes};
use bruno_ai::AiClient;
use bruno_voice::TtsHandle;

/// Process-wide tool-using agent, initialized in [`run`]. `Some(None)` means the
/// provider has no agent support; the response path falls back to plain chat.
static AGENT: OnceLock<Option<Arc<bruno_ai::Agent>>> = OnceLock::new();

pub struct WiringState {
    pub focus_mode: bool,
    pub hud_visible: bool,
    pub voice_active: bool,
    pub listening: bool,
    pub enrolling: bool,
    pub irrelevant_minutes: u64,
}

impl Default for WiringState {
    fn default() -> Self {
        Self {
            focus_mode: false,
            hud_visible: false,
            voice_active: false,
            listening: false,
            enrolling: false,
            irrelevant_minutes: 0,
        }
    }
}

pub async fn run(
    bus: BrunoBus,
    hud_tx: Sender<HudCommand>,
    mood_tx: Sender<MoodCommand>,
    tts: TtsHandle,
    state: Arc<Mutex<WiringState>>,
    browser: Arc<dyn bruno_ai::Browser>,
) {
    let ai = AsyncMutex::new(AiClient::from_default_config());
    // Neurodivergence-aware nudge gate (cooldown, hourly cap, snooze, hyperfocus,
    // quiet hours, adaptive tone). See `crate::nudge` and the cited paper.
    let nudge = Arc::new(Mutex::new(NudgePolicy::new(bruno_ai::Config::load().neuro)));
    // The tool-using agent (RAG + web). None for providers without agent support.
    let browser_for_test = browser.clone();
    let agent = bruno_ai::Agent::from_config(&bruno_ai::Config::load())
        .map(|a| a.with_browser(browser))
        .map(Arc::new);
    let _ = AGENT.set(agent);

    // Optional: verify the headless WKWebView end-to-end (BRUNO_BROWSER_TEST=1).
    if std::env::var("BRUNO_BROWSER_TEST").is_ok() {
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(3)).await;
            match browser_for_test.fetch("https://example.com").await {
                Ok(t) => tracing::info!(len = t.len(), preview = %t.chars().take(80).collect::<String>(), "browser fetch test"),
                Err(e) => tracing::warn!("browser fetch test failed: {e}"),
            }
            match browser_for_test.search("rust programming language").await {
                Ok(t) => tracing::info!(len = t.len(), preview = %t.chars().take(140).collect::<String>(), "browser search test"),
                Err(e) => tracing::warn!("browser search test failed: {e}"),
            }
        });
    }
    let mut activity_rx = bus.subscribe_activity();
    let mut voice_rx = bus.subscribe_voice();

    loop {
        tokio::select! {
            result = activity_rx.recv() => {
                let Ok(event) = result else { continue };
                handle_activity(&event, &hud_tx, &mood_tx, &tts, &ai, &state, &nudge).await;
            }
            result = voice_rx.recv() => {
                let Ok(event) = result else { continue };
                handle_voice(&event, &bus, &hud_tx, &mood_tx, &tts, &ai, &state, &nudge).await;
            }
        }
    }
}

async fn handle_activity(
    event: &ActivityEvent,
    hud_tx: &Sender<HudCommand>,
    mood_tx: &Sender<MoodCommand>,
    tts: &TtsHandle,
    ai: &AsyncMutex<AiClient>,
    state: &Arc<Mutex<WiringState>>,
    nudge: &Arc<Mutex<NudgePolicy>>,
) {
    match event {
        ActivityEvent::IdleStarted { .. } => {
            let _ = mood_tx.send(MoodCommand::Set(OrbMood::Sleepy));
        }
        ActivityEvent::IdleEnded { duration_secs } => {
            let _ = mood_tx.send(MoodCommand::Set(OrbMood::Idle));
            if let Ok(mut s) = state.lock() {
                s.irrelevant_minutes = *duration_secs / 60;
            }
        }
        ActivityEvent::IrrelevantContent { reason, .. } => {
            let minutes = state.lock().map(|s| s.irrelevant_minutes).unwrap_or(1);

            // Neurodivergence-aware gate: decide whether interrupting now is kind.
            // If suppressed, stay completely quiet — no HUD, no voice, no mood
            // change — so Bruno never nags (the paper's core design implication).
            let want_clock = nudge.lock().map(|p| p.quiet_hours_enabled()).unwrap_or(false);
            let lm = if want_clock { local_minutes() } else { None };
            let decision = nudge
                .lock()
                .map(|mut p| p.try_nudge(Instant::now(), lm))
                .unwrap_or(Ok(()));
            // The drift broke the focus streak; reset so sustained off-task time
            // isn't masked by stale hyperfocus protection on the next check.
            if let Ok(mut p) = nudge.lock() {
                p.clear_focus();
            }
            let prompt = match decision {
                Ok(()) => nudge
                    .lock()
                    .map(|p| p.nudge_instruction(reason, minutes))
                    .unwrap_or_default(),
                Err(why) => {
                    tracing::debug!(?why, %reason, "nudge suppressed");
                    return;
                }
            };

            let _ = mood_tx.send(MoodCommand::Set(OrbMood::Concerned));
            show_hud(hud_tx, state);
            let _ = hud_tx.send(HudCommand::SetText(format!("Still there? {reason}")));
            schedule_hide(hud_tx.clone(), state.clone(), Duration::from_secs(5));
            respond_with_ai(hud_tx, tts, ai, &prompt).await;
        }
        ActivityEvent::RelevantContent { .. } => {
            let _ = mood_tx.send(MoodCommand::Set(OrbMood::Flow));
            // Sustained relevant work builds hyperfocus protection.
            if let Ok(mut p) = nudge.lock() {
                p.note_focus();
            }
        }
        ActivityEvent::WindowChanged { .. } => {}
    }
}

async fn handle_voice(
    event: &VoiceEvent,
    bus: &BrunoBus,
    hud_tx: &Sender<HudCommand>,
    mood_tx: &Sender<MoodCommand>,
    tts: &TtsHandle,
    ai: &AsyncMutex<AiClient>,
    state: &Arc<Mutex<WiringState>>,
    nudge: &Arc<Mutex<NudgePolicy>>,
) {
    match event {
        VoiceEvent::ListeningChanged { enabled } => {
            if let Ok(mut s) = state.lock() {
                s.listening = *enabled;
                s.voice_active = *enabled;
            }
            if *enabled {
                let enrolling = state.lock().map(|s| s.enrolling).unwrap_or(false);
                if enrolling {
                    let _ = mood_tx.send(MoodCommand::Set(OrbMood::Loading));
                    show_hud(hud_tx, state);
                    let _ = hud_tx.send(HudCommand::SetText(
                        "Training your voice — say: Hey Bruno".into(),
                    ));
                } else {
                    let _ = mood_tx.send(MoodCommand::Set(OrbMood::Curious));
                    show_hud(hud_tx, state);
                    let _ = hud_tx.send(HudCommand::SetText("Listening…".into()));
                }
                let _ = hud_tx.send(HudCommand::SetPulsing(true));
                let _ = hud_tx.send(HudCommand::ShowInput(true));
            } else {
                let _ = mood_tx.send(MoodCommand::Set(OrbMood::Idle));
                let _ = hud_tx.send(HudCommand::SetPulsing(false));
                hide_hud(hud_tx, state);
            }
        }
        VoiceEvent::PartialTranscript { text } => {
            if state.lock().map(|s| s.enrolling).unwrap_or(false) {
                return;
            }
            let _ = mood_tx.send(MoodCommand::Set(OrbMood::Curious));
            show_hud(hud_tx, state);
            let _ = hud_tx.send(HudCommand::SetText(text.clone()));
        }
        VoiceEvent::UserSpeechStarted => {
            let _ = mood_tx.send(MoodCommand::Set(OrbMood::Curious));
            let _ = hud_tx.send(HudCommand::SetPulsing(true));
        }
        VoiceEvent::UserSpeechEnded => {
            let _ = hud_tx.send(HudCommand::SetPulsing(false));
            if state.lock().map(|s| s.listening && !s.enrolling).unwrap_or(false) {
                let _ = mood_tx.send(MoodCommand::Set(OrbMood::Loading));
                let _ = hud_tx.send(HudCommand::SetText("Thinking…".into()));
            }
        }
        VoiceEvent::PermissionDenied => {
            show_hud(hud_tx, state);
            let _ = hud_tx.send(HudCommand::SetText(
                "Enable microphone and speech recognition in System Settings.".into(),
            ));
            let _ = mood_tx.send(MoodCommand::Set(OrbMood::Concerned));
        }
        VoiceEvent::SpeakerRejected { .. } => {
            let _ = mood_tx.send(MoodCommand::Set(OrbMood::Concerned));
            schedule_mood_reset(mood_tx.clone(), Duration::from_millis(600));
        }
        VoiceEvent::EnrollmentProgress { step, total } => {
            if let Ok(mut s) = state.lock() {
                s.enrolling = true;
            }
            let _ = mood_tx.send(MoodCommand::Set(OrbMood::Loading));
            show_hud(hud_tx, state);
            let _ = hud_tx.send(HudCommand::SetText(format!(
                "Training your voice ({step}/{total}) — say: Hey Bruno"
            )));
        }
        VoiceEvent::EnrollmentComplete => {
            if let Ok(mut s) = state.lock() {
                s.enrolling = false;
            }
            let _ = mood_tx.send(MoodCommand::Set(OrbMood::Curious));
            let msg = "Got it. I learned your voice.";
            let _ = hud_tx.send(HudCommand::SetText(msg.into()));
            tts.speak(msg);
        }
        VoiceEvent::BrunoSpeakingStarted => {
            let _ = mood_tx.send(MoodCommand::Set(OrbMood::Flow));
            let _ = hud_tx.send(HudCommand::SetPulsing(true));
        }
        VoiceEvent::BrunoSpeakingFinished => {
            let listening = state.lock().map(|s| s.listening).unwrap_or(false);
            if listening {
                let _ = mood_tx.send(MoodCommand::Set(OrbMood::Curious));
                let _ = hud_tx.send(HudCommand::SetText("Listening…".into()));
            } else {
                let _ = mood_tx.send(MoodCommand::Set(OrbMood::Idle));
            }
            let _ = hud_tx.send(HudCommand::SetPulsing(listening));
            schedule_hide(hud_tx.clone(), state.clone(), Duration::from_secs(3));
        }
        VoiceEvent::Utterance { text } => {
            let lower = text.to_lowercase();
            if lower.contains("go away") || lower.contains("hide") || lower.contains("snooze") {
                // User asked for quiet — honor autonomy and stop nudging for a while.
                if let Ok(mut p) = nudge.lock() {
                    p.snooze(Instant::now());
                }
                hide_hud(hud_tx, state);
                return;
            }
        }
        VoiceEvent::IntentDetected(intent) => {
            if matches!(intent, Intent::Ignored) {
                return;
            }

            show_hud(hud_tx, state);
            if let Ok(mut s) = state.lock() {
                s.voice_active = true;
            }
            let _ = hud_tx.send(HudCommand::ShowInput(true));

            match intent {
                Intent::Ignored => {}
                Intent::EnrollVoice | Intent::ForgetVoice => {
                    if matches!(intent, Intent::ForgetVoice) {
                        let msg = "Voice profile cleared.";
                        let _ = hud_tx.send(HudCommand::SetText(msg.into()));
                        tts.speak(msg);
                    }
                }
                Intent::HearingCheck => {
                    let msg = "Yes, I can hear you.";
                    let _ = hud_tx.send(HudCommand::SetText(msg.to_string()));
                    tts.speak(msg);
                }
                Intent::Greeting => {
                    let msg = greeting_reply();
                    let _ = hud_tx.send(HudCommand::SetText(msg.to_string()));
                    tts.speak(msg);
                }
                Intent::EnterFocus => {
                    if let Ok(mut s) = state.lock() {
                        s.focus_mode = true;
                    }
                    let _ = mood_tx.send(MoodCommand::Set(OrbMood::Flow));
                    hide_hud(hud_tx, state);
                }
                Intent::WhereWasI => {
                    respond_with_ai(
                        hud_tx,
                        tts,
                        ai,
                        "The user asked where they were. Briefly summarize what they were likely working on based on context.",
                    )
                    .await;
                }
                Intent::Calendar => {
                    let summary = calendar::today_summary();
                    let _ = hud_tx.send(HudCommand::SetText(summary.clone()));
                    tts.speak(&summary);
                }
                Intent::Command { action } => {
                    respond_with_ai(
                        hud_tx,
                        tts,
                        ai,
                        &format!(
                            "The user gave a voice command: {action}. Respond briefly with what you would do or ask one clarifying question."
                        ),
                    )
                    .await;
                }
                Intent::Research { query } => {
                    respond_with_ai(hud_tx, tts, ai, query).await;
                }
                Intent::Break => {
                    // Stepping away on purpose isn't drifting — silence nudges so
                    // Bruno doesn't pester the user about their own break.
                    if let Ok(mut p) = nudge.lock() {
                        p.snooze(Instant::now());
                    }
                    let msg = "Step away for a few minutes. I'll be here.";
                    let _ = hud_tx.send(HudCommand::SetText(msg.to_string()));
                    tts.speak(msg);
                }
                Intent::NextTask => {
                    respond_with_ai(
                        hud_tx,
                        tts,
                        ai,
                        "The user asked what to work on next. Suggest one small concrete next step.",
                    )
                    .await;
                }
                Intent::Converse { text } => {
                    respond_with_ai(hud_tx, tts, ai, text).await;
                }
            }

            schedule_hide(hud_tx.clone(), state.clone(), Duration::from_secs(2));
            let _ = bus;
        }
    }
}

fn schedule_mood_reset(mood_tx: Sender<MoodCommand>, delay: Duration) {
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        let _ = mood_tx.send(MoodCommand::Set(OrbMood::Curious));
    });
}

fn show_hud(hud_tx: &Sender<HudCommand>, state: &std::sync::Arc<Mutex<WiringState>>) {
    if state.lock().map(|s| s.focus_mode).unwrap_or(false) {
        return;
    }
    if let Ok(mut s) = state.lock() {
        s.hud_visible = true;
    }
    let _ = hud_tx.send(HudCommand::Show);
}

fn hide_hud(hud_tx: &Sender<HudCommand>, state: &std::sync::Arc<Mutex<WiringState>>) {
    if let Ok(mut s) = state.lock() {
        s.hud_visible = false;
        s.voice_active = false;
        s.listening = false;
    }
    let _ = hud_tx.send(HudCommand::Hide);
    let _ = hud_tx.send(HudCommand::ShowInput(false));
}

fn schedule_hide(
    hud_tx: Sender<HudCommand>,
    state: std::sync::Arc<Mutex<WiringState>>,
    delay: Duration,
) {
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        if state.lock().map(|s| s.voice_active || s.listening).unwrap_or(false) {
            return;
        }
        if let Ok(mut s) = state.lock() {
            if !s.hud_visible {
                return;
            }
            s.hud_visible = false;
        }
        let _ = hud_tx.send(HudCommand::Hide);
        let _ = hud_tx.send(HudCommand::ShowInput(false));
    });
}

async fn respond_with_ai(
    hud_tx: &Sender<HudCommand>,
    tts: &TtsHandle,
    ai: &AsyncMutex<AiClient>,
    user_message: &str,
) {
    let _ = hud_tx.send(HudCommand::SetText("Thinking…".into()));

    // Prefer the tool-using agent (RAG + web). It can take longer (web fetches),
    // so give it a generous timeout; fall back to plain chat on any failure.
    if let Some(Some(agent)) = AGENT.get() {
        let result =
            tokio::time::timeout(Duration::from_secs(90), agent.run(user_message)).await;
        if let Ok(Ok(text)) = result {
            let text = text.trim();
            if !text.is_empty() {
                let _ = hud_tx.send(HudCommand::SetText(text.to_string()));
                tts.speak(text);
                return;
            }
        }
        // else: fall through to plain chat below.
    }

    let available = tokio::time::timeout(Duration::from_secs(2), async {
        ai.lock().await.is_available().await
    })
    .await
    .unwrap_or(false);

    if !available {
        let msg = "I'm offline.";
        let _ = hud_tx.send(HudCommand::SetText(msg.to_string()));
        tts.speak(msg);
        return;
    }

    let hud = hud_tx.clone();
    let user = user_message.to_string();
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        async {
            ai
                .lock()
                .await
                .chat_stream(&user, |full| {
                    let _ = hud.send(HudCommand::SetText(full.to_string()));
                })
                .await
        },
    )
    .await;

    match result {
        Ok(Ok(text)) => {
            if !text.is_empty() {
                tts.speak(&text);
            }
        }
        Ok(Err(_)) | Err(_) => {
            let msg = "I'm offline.";
            let _ = hud_tx.send(HudCommand::SetText(msg.to_string()));
            tts.speak(msg);
        }
    }
}

fn greeting_reply() -> &'static str {
    const REPLIES: &[&str] = &[
        "Doing well. You?",
        "All good here.",
        "I'm here. What's up?",
    ];
    let idx = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as usize)
        .unwrap_or(0)
        % REPLIES.len();
    REPLIES[idx]
}

pub fn handle_hotkey(
    hud_tx: &Sender<HudCommand>,
    voice_enabled: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    state: &std::sync::Arc<Mutex<WiringState>>,
    voice_on: bool,
) {
    info!("hotkey: toggle voice");
    if state.lock().map(|s| s.focus_mode).unwrap_or(false) {
        return;
    }
    voice_enabled.store(voice_on, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut s) = state.lock() {
        s.voice_active = voice_on;
        s.listening = voice_on;
        if voice_on {
            s.hud_visible = true;
            let _ = hud_tx.send(HudCommand::Show);
            let _ = hud_tx.send(HudCommand::SetText("Starting voice…".into()));
            let _ = hud_tx.send(HudCommand::ShowInput(true));
        } else {
            s.hud_visible = false;
            let _ = hud_tx.send(HudCommand::Hide);
            let _ = hud_tx.send(HudCommand::ShowInput(false));
        }
    }
}
