use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Intent {
    WhereWasI,
    Research { query: String },
    EnterFocus,
    Break,
    NextTask,
    HearingCheck,
    /// Casual hello / how-are-you — instant reply, no LLM.
    Greeting,
    Converse { text: String },
    Calendar,
    Command { action: String },
    /// Re-enroll or clear voice profile.
    EnrollVoice,
    ForgetVoice,
    /// Transcript had no wake phrase (ignored in Jarvis mode).
    Ignored,
}
