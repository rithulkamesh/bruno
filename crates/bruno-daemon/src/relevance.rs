//! Debounced relevance state machine.

use bruno_core::{ActivityEvent, BrunoBus};
use tracing::debug;

use crate::classify::Classification;

const CONFIDENCE_THRESHOLD: f32 = 0.85;
const CONSECUTIVE_IRRELEVANT_REQUIRED: u8 = 3;

pub struct RelevanceTracker {
    consecutive_irrelevant: u8,
    was_irrelevant_streak: bool,
}

impl RelevanceTracker {
    pub fn new() -> Self {
        Self {
            consecutive_irrelevant: 0,
            was_irrelevant_streak: false,
        }
    }

    pub fn process(
        &mut self,
        bus: &BrunoBus,
        classification: Classification,
        app: &str,
        title: &str,
    ) {
        if classification.relevant {
            self.consecutive_irrelevant = 0;
            if self.was_irrelevant_streak {
                debug!(app, title, "content became relevant");
                bus.emit_activity(ActivityEvent::RelevantContent {
                    app: app.to_string(),
                    title: title.to_string(),
                });
            }
            self.was_irrelevant_streak = false;
            return;
        }

        if classification.confidence <= CONFIDENCE_THRESHOLD {
            self.consecutive_irrelevant = 0;
            return;
        }

        self.consecutive_irrelevant = self.consecutive_irrelevant.saturating_add(1);
        debug!(
            count = self.consecutive_irrelevant,
            reason = %classification.reason,
            confidence = classification.confidence,
            "irrelevant check"
        );

        if self.consecutive_irrelevant >= CONSECUTIVE_IRRELEVANT_REQUIRED {
            self.was_irrelevant_streak = true;
            bus.emit_activity(ActivityEvent::IrrelevantContent {
                reason: classification.reason.clone(),
                confidence: classification.confidence,
            });
            self.consecutive_irrelevant = 0;
        }
    }
}

impl Default for RelevanceTracker {
    fn default() -> Self {
        Self::new()
    }
}
