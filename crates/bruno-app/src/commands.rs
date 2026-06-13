use std::sync::{Arc, Mutex};

use bruno_core::OrbMood;

#[derive(Debug, Clone, Default)]
pub struct OrbGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub enum HudCommand {
    Show,
    Hide,
    #[allow(dead_code)]
    Toggle,
    SetText(String),
    SetPulsing(bool),
    ShowInput(bool),
}

#[derive(Debug, Clone)]
pub enum MoodCommand {
    Set(OrbMood),
}

pub type SharedGeometry = Arc<Mutex<OrbGeometry>>;

pub fn orb_mood_to_mood(mood: OrbMood) -> bruno_orb::Mood {
    use bruno_core::OrbMood;
    use bruno_orb::Mood;
    match mood {
        OrbMood::Sleepy | OrbMood::Idle => Mood::Neutral,
        OrbMood::Curious | OrbMood::Loading => Mood::Thinking,
        OrbMood::Flow => Mood::Happy,
        OrbMood::Concerned => Mood::Angry,
    }
}
