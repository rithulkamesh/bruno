//! Neurodivergence-aware nudge gate.
//!
//! Implements the adaptive feedback engine described in Deshmukh, *"Toward
//! Neurodivergent-Aware Productivity: A Systems and AI-Based Human-in-the-Loop
//! Framework for ADHD-Affected Professionals"* (CHItaly 2025,
//! doi:10.1145/3750069.3750114). The daemon decides *whether* content is
//! off-task; this decides whether it's actually kind to say something right now.
//!
//! The paper's design implications, made concrete:
//! - **Non-disruptive:** a cooldown plus an hourly cap prevent alarm fatigue.
//! - **Respect attention rhythms:** sustained focus (hyperfocus) and quiet hours
//!   suppress nudges entirely.
//! - **User autonomy:** "snooze" / "go away" / "taking a break" silences Bruno
//!   for a configurable window.
//! - **Shame-free, adaptive tone:** the spoken prompt adapts to the user's
//!   communication profile.
//!
//! All state is in-memory and on-device; nothing here is persisted or sent off.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use bruno_ai::{NeuroConfig, NeuroProfile, NudgeTone};

/// Why a nudge was suppressed — useful for logs, never shown to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suppressed {
    Disabled,
    Snoozed,
    QuietHours,
    Cooldown,
    HourlyCap,
    Hyperfocus,
}

pub struct NudgePolicy {
    cfg: NeuroConfig,
    last_nudge: Option<Instant>,
    /// Nudge timestamps within the last hour (for the rolling cap).
    recent: VecDeque<Instant>,
    snooze_until: Option<Instant>,
    /// Consecutive "still on a single focused task" signals from the daemon.
    focus_streak: u32,
}

/// How many sustained on-task signals count as hyperfocus worth protecting.
const HYPERFOCUS_STREAK: u32 = 4;

impl NudgePolicy {
    pub fn new(cfg: NeuroConfig) -> Self {
        Self {
            cfg,
            last_nudge: None,
            recent: VecDeque::new(),
            snooze_until: None,
            focus_streak: 0,
        }
    }

    /// The user is engaged with relevant work — reinforce hyperfocus protection.
    pub fn note_focus(&mut self) {
        self.focus_streak = self.focus_streak.saturating_add(1);
    }

    /// The user drifted; deep-focus protection no longer applies.
    pub fn clear_focus(&mut self) {
        self.focus_streak = 0;
    }

    /// Whether quiet-hours is configured (so the caller knows to read the clock).
    pub fn quiet_hours_enabled(&self) -> bool {
        !self.cfg.quiet_hours.trim().is_empty()
    }

    /// Explicit user request for quiet ("snooze", "go away", "taking a break").
    pub fn snooze(&mut self, now: Instant) {
        self.snooze_until = Some(now + Duration::from_secs(self.cfg.snooze_minutes * 60));
        self.focus_streak = 0;
    }

    /// Decide whether to nudge now. On `Ok`, the nudge is recorded so the
    /// cooldown and hourly cap advance; on `Err`, nothing changes.
    ///
    /// `local_minutes` is minutes-since-midnight in the user's local time, used
    /// only for the quiet-hours window (see [`local_minutes`]).
    pub fn try_nudge(&mut self, now: Instant, local_minutes: Option<u32>) -> Result<(), Suppressed> {
        if !self.cfg.enabled {
            return Err(Suppressed::Disabled);
        }
        if let Some(until) = self.snooze_until {
            if now < until {
                return Err(Suppressed::Snoozed);
            }
        }
        if let Some(mins) = local_minutes {
            if in_quiet_hours(&self.cfg.quiet_hours, mins) {
                return Err(Suppressed::QuietHours);
            }
        }
        if self.cfg.hyperfocus_protection && self.focus_streak >= HYPERFOCUS_STREAK {
            return Err(Suppressed::Hyperfocus);
        }
        if let Some(last) = self.last_nudge {
            if now.duration_since(last) < Duration::from_secs(self.cfg.nudge_cooldown_secs) {
                return Err(Suppressed::Cooldown);
            }
        }
        self.prune(now);
        if self.recent.len() as u32 >= self.cfg.max_nudges_per_hour {
            return Err(Suppressed::HourlyCap);
        }

        self.last_nudge = Some(now);
        self.recent.push_back(now);
        Ok(())
    }

    /// Build the spoken nudge instruction for the LLM, shame-free and adapted to
    /// the configured profile and tone.
    pub fn nudge_instruction(&self, reason: &str, minutes: u64) -> String {
        let drift = if minutes >= 1 {
            format!("for about {minutes} minute(s)")
        } else {
            "just now".to_string()
        };
        let style = match self.cfg.tone {
            NudgeTone::Gentle => "Keep it soft and reassuring.",
            NudgeTone::Direct => "Keep it plain and brief, no cushioning.",
        };
        match self.cfg.profile {
            NeuroProfile::Adhd => format!(
                "The user has ADHD and drifted from their work ({reason}) {drift}. There is zero \
                 judgment here — drifting is normal. Warmly check in and offer one tiny, concrete \
                 step to ease back in. Do not list reasons or lecture. {style} One short spoken \
                 sentence."
            ),
            NeuroProfile::Autistic => format!(
                "The user drifted from their work ({reason}) {drift}. Use clear, literal language \
                 with no idioms, sarcasm, or ambiguity. State plainly what they were doing and \
                 suggest one specific next action. {style} One short spoken sentence."
            ),
            NeuroProfile::Generic => format!(
                "The user drifted from their work ({reason}) {drift}. Gently remind them and \
                 suggest one concrete next step. No shaming. {style} One short spoken sentence."
            ),
        }
    }

    fn prune(&mut self, now: Instant) {
        let hour = Duration::from_secs(3600);
        while let Some(&front) = self.recent.front() {
            if now.duration_since(front) >= hour {
                self.recent.pop_front();
            } else {
                break;
            }
        }
    }
}

/// Minutes-since-midnight in local time, or `None` if it can't be determined.
/// Only called when quiet-hours is configured, so the `date` spawn is rare.
pub fn local_minutes() -> Option<u32> {
    let out = std::process::Command::new("date").arg("+%H:%M").output().ok()?;
    let s = String::from_utf8(out.stdout).ok()?;
    parse_hm(s.trim())
}

/// Parse `"HH:MM"` into minutes-since-midnight.
fn parse_hm(s: &str) -> Option<u32> {
    let (h, m) = s.split_once(':')?;
    let h: u32 = h.trim().parse().ok()?;
    let m: u32 = m.trim().parse().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    Some(h * 60 + m)
}

/// Is `now_min` (minutes-since-midnight) inside the `"HH:MM-HH:MM"` window?
/// Handles windows that wrap past midnight (e.g. `"22:00-08:00"`).
fn in_quiet_hours(spec: &str, now_min: u32) -> bool {
    let spec = spec.trim();
    if spec.is_empty() {
        return false;
    }
    let Some((start, end)) = spec.split_once('-') else {
        return false;
    };
    let (Some(start), Some(end)) = (parse_hm(start), parse_hm(end)) else {
        return false;
    };
    if start == end {
        false
    } else if start < end {
        now_min >= start && now_min < end
    } else {
        // Wraps midnight.
        now_min >= start || now_min < end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> NeuroConfig {
        NeuroConfig {
            nudge_cooldown_secs: 600,
            max_nudges_per_hour: 4,
            snooze_minutes: 30,
            ..Default::default()
        }
    }

    #[test]
    fn first_nudge_allowed_then_cooldown_blocks() {
        let mut p = NudgePolicy::new(cfg());
        let t0 = Instant::now();
        assert!(p.try_nudge(t0, None).is_ok());
        // 5 minutes later — still inside the 10-minute cooldown.
        let t1 = t0 + Duration::from_secs(300);
        assert_eq!(p.try_nudge(t1, None), Err(Suppressed::Cooldown));
        // 11 minutes after the first — cooldown elapsed.
        let t2 = t0 + Duration::from_secs(660);
        assert!(p.try_nudge(t2, None).is_ok());
    }

    #[test]
    fn hourly_cap_enforced() {
        let mut p = NudgePolicy::new(cfg());
        let mut t = Instant::now();
        for _ in 0..4 {
            assert!(p.try_nudge(t, None).is_ok());
            t += Duration::from_secs(601); // clear cooldown each time
        }
        assert_eq!(p.try_nudge(t, None), Err(Suppressed::HourlyCap));
        // After the rolling hour passes, capacity frees up again.
        let later = t + Duration::from_secs(3600);
        assert!(p.try_nudge(later, None).is_ok());
    }

    #[test]
    fn snooze_silences_then_expires() {
        let mut p = NudgePolicy::new(cfg());
        let t0 = Instant::now();
        p.snooze(t0);
        assert_eq!(p.try_nudge(t0 + Duration::from_secs(60), None), Err(Suppressed::Snoozed));
        // 31 minutes later the 30-minute snooze is over.
        assert!(p.try_nudge(t0 + Duration::from_secs(31 * 60), None).is_ok());
    }

    #[test]
    fn hyperfocus_protected() {
        let mut p = NudgePolicy::new(cfg());
        for _ in 0..HYPERFOCUS_STREAK {
            p.note_focus();
        }
        assert_eq!(p.try_nudge(Instant::now(), None), Err(Suppressed::Hyperfocus));
        // Drifting clears the protection.
        p.clear_focus();
        assert!(p.try_nudge(Instant::now(), None).is_ok());
    }

    #[test]
    fn disabled_short_circuits() {
        let mut p = NudgePolicy::new(NeuroConfig { enabled: false, ..cfg() });
        assert_eq!(p.try_nudge(Instant::now(), None), Err(Suppressed::Disabled));
    }

    #[test]
    fn quiet_hours_wrap_midnight() {
        // 22:00–08:00
        assert!(in_quiet_hours("22:00-08:00", 23 * 60)); // 23:00 inside
        assert!(in_quiet_hours("22:00-08:00", 2 * 60)); // 02:00 inside
        assert!(!in_quiet_hours("22:00-08:00", 12 * 60)); // noon outside
        // Same-day window 09:00–17:00
        assert!(in_quiet_hours("09:00-17:00", 12 * 60));
        assert!(!in_quiet_hours("09:00-17:00", 20 * 60));
        // Empty / malformed = never quiet.
        assert!(!in_quiet_hours("", 12 * 60));
        assert!(!in_quiet_hours("bogus", 12 * 60));
    }

    #[test]
    fn quiet_hours_blocks_nudge() {
        let mut p = NudgePolicy::new(NeuroConfig { quiet_hours: "22:00-08:00".into(), ..cfg() });
        assert_eq!(p.try_nudge(Instant::now(), Some(23 * 60)), Err(Suppressed::QuietHours));
        assert!(p.try_nudge(Instant::now(), Some(12 * 60)).is_ok());
    }

    #[test]
    fn tone_and_profile_shape_prompt() {
        let adhd = NudgePolicy::new(NeuroConfig { profile: NeuroProfile::Adhd, ..cfg() });
        assert!(adhd.nudge_instruction("youtube", 3).contains("ADHD"));
        let autistic =
            NudgePolicy::new(NeuroConfig { profile: NeuroProfile::Autistic, ..cfg() });
        assert!(autistic.nudge_instruction("youtube", 3).to_lowercase().contains("literal"));
    }
}
