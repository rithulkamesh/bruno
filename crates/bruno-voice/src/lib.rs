//! Bruno voice input and output for macOS.

pub mod config;
mod intent;
mod permissions;
#[cfg(feature = "piper")]
pub mod piper;
mod speaker;
mod stt;
mod tts;
pub mod whisper;

use std::sync::Arc;

use bruno_core::BrunoBus;

pub use intent::detect;
pub use permissions::ensure as ensure_permissions;
pub use stt::Stt;
pub use tts::{pair as tts_pair, TtsHandle, TtsRuntime};

pub struct VoiceService {
    pub tts: TtsHandle,
    pub tts_runtime: TtsRuntime,
    pub stt: Arc<Stt>,
}

impl VoiceService {
    pub fn new(bus: BrunoBus) -> Self {
        let (tts, tts_runtime) = tts_pair(bus.clone());
        let stt = Arc::new(Stt::new(bus));
        Self {
            tts,
            tts_runtime,
            stt,
        }
    }

    pub fn speak(&self, text: &str) {
        self.tts.speak(text);
    }

    pub fn set_listening(&self, enabled: bool) {
        self.stt.set_listening(enabled);
    }

    pub fn poll_tts(&mut self) {
        self.tts_runtime.poll();
    }

    pub async fn run(self) {
        std::future::pending::<()>().await;
    }
}
