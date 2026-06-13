use tokio::sync::broadcast;

use crate::events::{ActivityEvent, VoiceEvent};

#[derive(Clone)]
pub struct BrunoBus {
    pub activity: broadcast::Sender<ActivityEvent>,
    pub voice: broadcast::Sender<VoiceEvent>,
}

impl BrunoBus {
    pub fn new(capacity: usize) -> Self {
        let (activity, _) = broadcast::channel(capacity);
        let (voice, _) = broadcast::channel(capacity);
        Self { activity, voice }
    }

    pub fn subscribe_activity(&self) -> broadcast::Receiver<ActivityEvent> {
        self.activity.subscribe()
    }

    pub fn subscribe_voice(&self) -> broadcast::Receiver<VoiceEvent> {
        self.voice.subscribe()
    }

    pub fn emit_activity(&self, event: ActivityEvent) {
        let _ = self.activity.send(event);
    }

    pub fn emit_voice(&self, event: VoiceEvent) {
        let _ = self.voice.send(event);
    }
}
