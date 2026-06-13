//! Microphone and speech recognition permission requests (non-blocking).

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

const UNDETERMINED: u8 = 0;
const GRANTED: u8 = 1;
const DENIED: u8 = 2;

pub struct PermissionGate {
    speech: AtomicU8,
    mic: AtomicU8,
    speech_requested: AtomicBool,
    mic_requested: AtomicBool,
}

impl PermissionGate {
    pub fn new() -> Arc<Self> {
        let gate = Arc::new(Self {
            speech: AtomicU8::new(UNDETERMINED),
            mic: AtomicU8::new(UNDETERMINED),
            speech_requested: AtomicBool::new(false),
            mic_requested: AtomicBool::new(false),
        });
        gate.refresh();
        gate
    }

    pub fn begin(self: &Arc<Self>) {
        self.refresh();
        self.request_if_needed();
    }

    /// `None` = still waiting, `Some(true/false)` = resolved.
    pub fn poll(self: &Arc<Self>) -> Option<bool> {
        self.refresh();
        let speech = self.speech.load(Ordering::Acquire);
        let mic = self.mic.load(Ordering::Acquire);
        if speech == GRANTED && mic == GRANTED {
            return Some(true);
        }
        if speech == DENIED || mic == DENIED {
            return Some(false);
        }
        None
    }

    fn refresh(&self) {
        #[cfg(target_os = "macos")]
        {
            use objc2_avf_audio::{AVAudioApplication, AVAudioApplicationRecordPermission};
            use objc2_speech::{SFSpeechRecognizer, SFSpeechRecognizerAuthorizationStatus};

            let speech = unsafe { SFSpeechRecognizer::authorizationStatus() };
            if speech == SFSpeechRecognizerAuthorizationStatus::Authorized {
                self.speech.store(GRANTED, Ordering::Release);
            } else if speech == SFSpeechRecognizerAuthorizationStatus::Denied
                || speech == SFSpeechRecognizerAuthorizationStatus::Restricted
            {
                self.speech.store(DENIED, Ordering::Release);
            }

            let mic = unsafe { AVAudioApplication::sharedInstance().recordPermission() };
            if mic == AVAudioApplicationRecordPermission::Granted {
                self.mic.store(GRANTED, Ordering::Release);
            } else if mic == AVAudioApplicationRecordPermission::Denied {
                self.mic.store(DENIED, Ordering::Release);
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.speech.store(GRANTED, Ordering::Release);
            self.mic.store(GRANTED, Ordering::Release);
        }
    }

    fn request_if_needed(self: &Arc<Self>) {
        #[cfg(target_os = "macos")]
        platform::request_if_needed(self);
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_avf_audio::AVAudioApplication;
    use objc2_speech::{SFSpeechRecognizer, SFSpeechRecognizerAuthorizationStatus};
    use tracing::{info, warn};

    use super::{PermissionGate, DENIED, GRANTED, UNDETERMINED};

    pub fn request_if_needed(gate: &Arc<PermissionGate>) {
        if gate.speech.load(Ordering::Acquire) == UNDETERMINED
            && !gate.speech_requested.swap(true, Ordering::AcqRel)
        {
            let speech = Arc::clone(gate);
            let block = RcBlock::new(move |status: SFSpeechRecognizerAuthorizationStatus| {
                if status == SFSpeechRecognizerAuthorizationStatus::Authorized {
                    info!("speech recognition authorized");
                    speech.speech.store(GRANTED, Ordering::Release);
                } else {
                    warn!("speech recognition not authorized: {:?}", status.0);
                    speech.speech.store(DENIED, Ordering::Release);
                }
            });
            unsafe { SFSpeechRecognizer::requestAuthorization(&block) };
        }

        if gate.mic.load(Ordering::Acquire) == UNDETERMINED
            && !gate.mic_requested.swap(true, Ordering::AcqRel)
        {
            let mic_gate = Arc::clone(gate);
            let block = RcBlock::new(move |granted: Bool| {
                if granted.as_bool() {
                    info!("microphone authorized");
                    mic_gate.mic.store(GRANTED, Ordering::Release);
                } else {
                    warn!("microphone denied");
                    mic_gate.mic.store(DENIED, Ordering::Release);
                }
            });
            unsafe { AVAudioApplication::requestRecordPermissionWithCompletionHandler(&block) };
        }
    }
}

/// Blocking helper for tests/tools only.
pub fn ensure() -> bool {
    let gate = PermissionGate::new();
    gate.begin();
    for _ in 0..600 {
        if let Some(ok) = gate.poll() {
            return ok;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    false
}
