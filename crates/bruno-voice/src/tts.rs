//! Text-to-speech via AVSpeechSynthesizer (main-thread runtime).

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};

use bruno_core::{BrunoBus, VoiceEvent};

#[cfg(target_os = "macos")]
mod platform {
    use objc2::rc::Retained;
    use objc2_avf_audio::{AVSpeechSynthesisVoice, AVSpeechSynthesizer, AVSpeechUtterance};
    use objc2_foundation::NSString;
    use tracing::warn;

    use crate::config::{TtsBackend, TtsConfig};

    /// Dispatches to whichever TTS backend the config selected.
    pub enum TtsEngine {
        Apple(AppleTts),
        #[cfg(feature = "piper")]
        Piper(PiperBackend),
    }

    impl TtsEngine {
        pub fn new(cfg: &TtsConfig) -> Self {
            match cfg.backend {
                TtsBackend::Apple => Self::Apple(AppleTts::new()),
                TtsBackend::Piper => {
                    #[cfg(feature = "piper")]
                    match PiperBackend::new(&cfg.piper) {
                        Ok(backend) => return Self::Piper(backend),
                        Err(e) => warn!("piper TTS unavailable: {e}; using Apple TTS"),
                    }
                    #[cfg(not(feature = "piper"))]
                    warn!("tts.backend=piper but built without the `piper` feature; using Apple TTS");
                    Self::Apple(AppleTts::new())
                }
            }
        }

        pub fn speak(&self, text: &str) {
            match self {
                Self::Apple(a) => a.speak(text),
                #[cfg(feature = "piper")]
                Self::Piper(p) => p.speak(text),
            }
        }

        pub fn is_speaking(&self) -> bool {
            match self {
                Self::Apple(a) => a.is_speaking(),
                #[cfg(feature = "piper")]
                Self::Piper(p) => p.is_speaking(),
            }
        }
    }

    // ---- Apple AVSpeechSynthesizer ---------------------------------------

    pub struct AppleTts {
        synthesizer: Retained<AVSpeechSynthesizer>,
    }

    /// Preferred male en voices, in order. The first that resolves on this
    /// machine is used; if none are installed, the system default applies.
    const MALE_VOICE_IDS: &[&str] = &[
        "com.apple.voice.super-compact.en-GB.Daniel", // British male (installed here)
        "com.apple.speech.synthesis.voice.Fred",      // US male, classic
        "com.apple.voice.compact.en-IN.Rishi",        // Indian-English male
        // Common on other machines as a further fallback:
        "com.apple.voice.compact.en-US.Aaron",
        "com.apple.speech.synthesis.voice.Alex",
        "com.apple.voice.compact.en-GB.Daniel",
    ];

    fn male_voice() -> Option<Retained<AVSpeechSynthesisVoice>> {
        for id in MALE_VOICE_IDS {
            let v = unsafe {
                AVSpeechSynthesisVoice::voiceWithIdentifier(&NSString::from_str(id))
            };
            if v.is_some() {
                return v;
            }
        }
        None
    }

    impl AppleTts {
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
                if let Some(voice) = male_voice() {
                    utterance.setVoice(Some(&voice));
                }
                utterance.setRate(0.45);
                utterance.setPitchMultiplier(0.95);
                self.synthesizer.speakUtterance(&utterance);
            }
        }

        pub fn is_speaking(&self) -> bool {
            unsafe { self.synthesizer.isSpeaking() }
        }
    }

    // ---- Piper (libpiper) ------------------------------------------------

    #[cfg(feature = "piper")]
    pub struct PiperBackend {
        tts: std::sync::Arc<std::sync::Mutex<crate::piper::PiperTts>>,
        speaking: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    #[cfg(feature = "piper")]
    impl PiperBackend {
        fn new(cfg: &crate::config::PiperConfig) -> Result<Self, String> {
            let tts = crate::piper::PiperTts::new(cfg)?;
            Ok(Self {
                tts: std::sync::Arc::new(std::sync::Mutex::new(tts)),
                speaking: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            })
        }

        fn speak(&self, text: &str) {
            use std::sync::atomic::Ordering;
            let tts = self.tts.clone();
            let speaking = self.speaking.clone();
            let text = text.to_string();
            // Mark speaking synchronously so the runtime queues follow-ups; synthesis
            // and playback run off the main thread.
            speaking.store(true, Ordering::Relaxed);
            std::thread::spawn(move || {
                let result = tts.lock().ok().and_then(|t| t.synthesize(&text).ok());
                match result {
                    Some((samples, sample_rate)) => {
                        if let Err(e) = play_samples(&samples, sample_rate) {
                            warn!("piper playback failed: {e}");
                        }
                    }
                    None => warn!("piper synthesis failed"),
                }
                speaking.store(false, Ordering::Relaxed);
            });
        }

        fn is_speaking(&self) -> bool {
            self.speaking.load(std::sync::atomic::Ordering::Relaxed)
        }
    }

    /// Write f32 samples to a temp WAV and play them with `afplay` (blocks until
    /// playback finishes).
    #[cfg(feature = "piper")]
    fn play_samples(samples: &[f32], sample_rate: u32) -> Result<(), String> {
        use hound::{SampleFormat, WavSpec, WavWriter};

        let path = std::env::temp_dir().join(format!("bruno-tts-{}.wav", std::process::id()));
        let spec = WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        {
            let mut writer = WavWriter::create(&path, spec).map_err(|e| e.to_string())?;
            for &s in samples {
                writer.write_sample(s).map_err(|e| e.to_string())?;
            }
            writer.finalize().map_err(|e| e.to_string())?;
        }
        let status = std::process::Command::new("afplay")
            .arg(&path)
            .status()
            .map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&path);
        if status.success() {
            Ok(())
        } else {
            Err(format!("afplay exited with {status}"))
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use crate::config::TtsConfig;

    pub struct TtsEngine;

    impl TtsEngine {
        pub fn new(_cfg: &TtsConfig) -> Self {
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
            engine: platform::TtsEngine::new(&crate::config::TtsConfig::load()),
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
