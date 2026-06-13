//! Bruno presence moods — distinct motion languages in the orb shader (`anim_style`).
//!
//! - **Neutral** (idle): slow breathe, barely drifts
//! - **Thinking** (loading): golden-hour warmth while Bruno processes your request
//! - **Happy**: soft blue, gentle bounce and sway
//! - **Angry** (rage): shake, jitter, aggressive churn — mirrors heated user state

use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Mood {
    #[default]
    Neutral,
    Thinking,
    Happy,
    Angry,
}

impl Mood {
    pub const ALL: [Mood; 4] = [Mood::Neutral, Mood::Thinking, Mood::Happy, Mood::Angry];

    /// Shader animation profile: 0 idle, 1 loading, 2 happy, 3 rage.
    pub const fn anim_style(self) -> f32 {
        match self {
            Mood::Neutral => 0.0,
            Mood::Thinking => 1.0,
            Mood::Happy => 2.0,
            Mood::Angry => 3.0,
        }
    }

    pub fn config(self) -> MoodConfig {
        match self {
            Mood::Neutral => MoodConfig {
                anim_style: 0.0,
                speed: 0.72,
                intensity: 0.90,
                motion: 0.42,
                pulse: 0.62,
                core_color: [0.10, 0.06, 0.38],
                glow_color: [0.36, 0.24, 0.72],
            },
            // Golden hour — amber core, honey-peach glow
            Mood::Thinking => MoodConfig {
                anim_style: 1.0,
                speed: 1.35,
                intensity: 0.96,
                motion: 1.05,
                pulse: 1.18,
                core_color: [0.26, 0.08, 0.04],
                glow_color: [0.94, 0.58, 0.28],
            },
            // Soft sky blue — calm, light
            Mood::Happy => MoodConfig {
                anim_style: 2.0,
                speed: 0.95,
                intensity: 0.96,
                motion: 0.88,
                pulse: 1.05,
                core_color: [0.06, 0.10, 0.30],
                glow_color: [0.48, 0.68, 0.90],
            },
            Mood::Angry => MoodConfig {
                anim_style: 3.0,
                speed: 1.55,
                intensity: 0.98,
                motion: 1.65,
                pulse: 1.45,
                core_color: [0.34, 0.04, 0.06],
                glow_color: [0.92, 0.18, 0.22],
            },
        }
    }
}

impl FromStr for Mood {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            s if s.eq_ignore_ascii_case("neutral") => Ok(Mood::Neutral),
            s if s.eq_ignore_ascii_case("thinking") => Ok(Mood::Thinking),
            s if s.eq_ignore_ascii_case("happy") => Ok(Mood::Happy),
            s if s.eq_ignore_ascii_case("angry") => Ok(Mood::Angry),
            s if s.eq_ignore_ascii_case("distracted") => Ok(Mood::Angry),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for Mood {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mood::Neutral => write!(f, "neutral"),
            Mood::Thinking => write!(f, "thinking"),
            Mood::Happy => write!(f, "happy"),
            Mood::Angry => write!(f, "angry"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MoodConfig {
    /// 0 idle, 1 loading, 2 happy, 3 rage — blended in the shader during mood transitions.
    pub anim_style: f32,
    pub speed: f32,
    pub intensity: f32,
    /// Float / drift amplitude multiplier
    pub motion: f32,
    /// Breathing pulse multiplier
    pub pulse: f32,
    pub core_color: [f32; 3],
    pub glow_color: [f32; 3],
}

impl MoodConfig {
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let lerp3 = |x: [f32; 3], y: [f32; 3]| {
            [
                x[0] + (y[0] - x[0]) * t,
                x[1] + (y[1] - x[1]) * t,
                x[2] + (y[2] - x[2]) * t,
            ]
        };
        Self {
            anim_style: a.anim_style + (b.anim_style - a.anim_style) * t,
            speed: a.speed + (b.speed - a.speed) * t,
            intensity: a.intensity + (b.intensity - a.intensity) * t,
            motion: a.motion + (b.motion - a.motion) * t,
            pulse: a.pulse + (b.pulse - a.pulse) * t,
            core_color: lerp3(a.core_color, b.core_color),
            glow_color: lerp3(a.glow_color, b.glow_color),
        }
    }
}
