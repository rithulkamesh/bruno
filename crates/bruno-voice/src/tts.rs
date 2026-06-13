//! Text-to-speech via AVSpeechSynthesizer (main-thread runtime).

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};

use bruno_core::{BrunoBus, VoiceEvent};

#[cfg(target_os = "macos")]
mod platform {
    use objc2::rc::Retained;
    use objc2_avf_audio::{AVSpeechSynthesisVoice, AVSpeechSynthesizer, AVSpeechUtterance};
    use objc2_foundation::NSString;

    pub struct TtsEngine {
        synthesizer: Retained<AVSpeechSynthesizer>,
    }

    impl TtsEngine {
        pub fn new() -> Self {
            unsafe {
                Self {
                    synthesizer: AVSpeechSynthesizer::new(),
                }
            }
        }

        pub fn speak(&self, text: &str) {
            unsafe {
                let utterance =
                    AVSpeechUtterance::speechUtteranceWithString(&NSString::from_str(text));
                if let Some(voice) = AVSpeechSynthesisVoice::voiceWithIdentifier(
                    &NSString::from_str("com.apple.voice.compact.en-US.Siri"),
                ) {
                    utterance.setVoice(Some(&voice));
                }
                utterance.setRate(0.45);
                utterance.setPitchMultiplier(1.05);
                self.synthesizer.speakUtterance(&utterance);
            }
        }

        pub fn is_speaking(&self) -> bool {
            unsafe { self.synthesizer.isSpeaking() }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    pub struct TtsEngine;

    impl TtsEngine {
        pub fn new() -> Self {
            Self
        }

        pub fn speak(&self, _text: &str) {}

        pub fn is_speaking(&self) -> bool {
            false
        }
    }
}

/// Send-safe handle for requesting speech from any thread.
#[derive(Clone)]
pub struct TtsHandle {
    tx: Sender<String>,
}

impl TtsHandle {
    pub fn speak(&self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let _ = self.tx.send(text.to_string());
    }
}

/// Main-thread TTS engine; call `poll` from the UI event loop.
pub struct TtsRuntime {
    engine: platform::TtsEngine,
    rx: Receiver<String>,
    bus: BrunoBus,
    queue: VecDeque<String>,
    was_speaking: bool,
}

impl TtsRuntime {
    pub fn new(bus: BrunoBus) -> (TtsHandle, Self) {
        let (tx, rx) = mpsc::channel();
        let runtime = Self {
            engine: platform::TtsEngine::new(),
            rx,
            bus,
            queue: VecDeque::new(),
            was_speaking: false,
        };
        (TtsHandle { tx }, runtime)
    }

    pub fn poll(&mut self) {
        while let Ok(text) = self.rx.try_recv() {
            if self.engine.is_speaking() {
                self.queue.push_back(text);
            } else {
                self.engine.speak(&text);
            }
        }

        let speaking = self.engine.is_speaking();
        if speaking && !self.was_speaking {
            self.bus.emit_voice(VoiceEvent::BrunoSpeakingStarted);
            self.was_speaking = true;
        } else if !speaking && self.was_speaking {
            self.bus.emit_voice(VoiceEvent::BrunoSpeakingFinished);
            self.was_speaking = false;
            if let Some(next) = self.queue.pop_front() {
                self.engine.speak(&next);
            }
        }
    }
}

pub fn pair(bus: BrunoBus) -> (TtsHandle, TtsRuntime) {
    TtsRuntime::new(bus)
}
