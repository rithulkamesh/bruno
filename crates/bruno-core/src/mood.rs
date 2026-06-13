use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrbMood {
    Sleepy,
    Idle,
    Concerned,
    Flow,
    Curious,
    Loading,
}
