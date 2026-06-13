use serde::{Deserialize, Serialize};

use crate::intent::Intent;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ActivityEvent {
    WindowChanged {
        app: String,
        title: String,
        timestamp: u64,
    },
    IdleStarted {
        since: u64,
    },
    IdleEnded {
        duration_secs: u64,
    },
    IrrelevantContent {
        reason: String,
        confidence: f32,
    },
    RelevantContent {
        app: String,
        title: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum VoiceEvent {
    ListeningChanged {
        enabled: bool,
    },
    PartialTranscript {
        text: String,
    },
    UserSpeechStarted,
    UserSpeechEnded,
    Utterance {
        text: String,
    },
    IntentDetected(Intent),
    BrunoSpeakingStarted,
    BrunoSpeakingFinished,
    /// Mic or speech recognition permission denied.
    PermissionDenied,
    /// Speaker verification failed (enrolled profile exists).
    SpeakerRejected {
        score: f32,
    },
    /// Voice enrollment progress or completion.
    EnrollmentProgress {
        step: u8,
        total: u8,
    },
    EnrollmentComplete,
}
