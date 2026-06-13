//! Speech-to-text.
//!
//! Two backends share one audio-capture pipeline (AVAudioEngine on macOS):
//! - **Apple** — `SFSpeechRecognizer`, native streaming partials.
//! - **Whisper** — whisper.cpp over the captured PCM, with an energy VAD for
//!   endpointing and a periodic re-transcription loop for live partials.
//!
//! The backend is chosen by `[stt]` in `~/.config/bruno/config.toml`
//! ([`SttBackend`]). `Auto` (the default) uses Whisper, falling back to Apple if
//! the model can't be loaded.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use bruno_core::{BrunoBus, Intent, VoiceEvent};

use crate::config::{SttBackend, SttConfig};
use crate::intent;
use crate::permissions::PermissionGate;
use crate::speaker::{self, SpeakerGate};
use crate::whisper::WhisperEngine;

const SILENCE_THRESHOLD: Duration = Duration::from_millis(450);
const PARTIAL_THROTTLE: Duration = Duration::from_millis(100);
/// How often the whisper backend re-transcribes the growing buffer for partials.
const WHISPER_PARTIAL_INTERVAL: Duration = Duration::from_millis(700);
/// RMS energy above which a frame counts as speech (whisper VAD endpointing).
const VAD_ENERGY_THRESHOLD: f32 = 0.012;

// Whisper model load status (background loader → poll_main).
const WHISPER_LOADING: u8 = 0;
const WHISPER_READY: u8 = 1;
const WHISPER_FAILED: u8 = 2;

/// Which concrete engine to instantiate once a backend has been resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EngineKind {
    Apple,
    Whisper,
}

struct SttInner {
    bus: BrunoBus,
    partial_text: String,
    last_partial: Instant,
    last_emit: Instant,
    user_speech_active: bool,
    audio_buffer: Vec<f32>,
    device_sample_rate: f32,
    speaker: SpeakerGate,
    enrolling: bool,
    enroll_step: u8,
    /// Present when the whisper backend is active; used for the final, full-buffer
    /// transcription in [`finalize_utterance`].
    whisper: Option<Arc<WhisperEngine>>,
}

impl SttInner {
    fn reset_utterance(&mut self) {
        self.user_speech_active = false;
        self.audio_buffer.clear();
    }
}

fn finalize_utterance(stt: &mut SttInner) {
    // Whisper: do a final, full-buffer transcription for best accuracy (the last
    // periodic partial may have missed the closing words). Runs before the buffer
    // is consumed by speaker verification below.
    if let Some(whisper) = stt.whisper.clone() {
        if !stt.audio_buffer.is_empty() {
            let samples = speaker::resample_to_16k(&stt.audio_buffer, stt.device_sample_rate);
            if let Some(text) = whisper.transcribe(&samples) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    stt.partial_text = trimmed.to_string();
                }
            }
        }
    }

    let text = std::mem::take(&mut stt.partial_text);
    let trimmed = text.trim();
    stt.bus.emit_voice(VoiceEvent::UserSpeechEnded);
    stt.user_speech_active = false;

    if trimmed.is_empty() {
        stt.audio_buffer.clear();
        return;
    }

    if stt.enrolling {
        let detected = intent::detect(trimmed);
        if matches!(
            detected,
            Intent::HearingCheck | Intent::Greeting | Intent::Converse { .. }
        ) {
            stt.bus.emit_voice(VoiceEvent::Utterance {
                text: trimmed.to_string(),
            });
            stt.bus.emit_voice(VoiceEvent::IntentDetected(detected));
        }
        match stt.speaker.enroll_chunk(&stt.audio_buffer, stt.device_sample_rate) {
            Ok(()) => {
                stt.enroll_step += 1;
                stt.bus.emit_voice(VoiceEvent::EnrollmentProgress {
                    step: stt.enroll_step,
                    total: speaker::ENROLL_SAMPLES,
                });
                if stt.enroll_step >= speaker::ENROLL_SAMPLES {
                    if stt.speaker.finish_enrollment().is_ok() {
                        stt.enrolling = false;
                        stt.bus.emit_voice(VoiceEvent::EnrollmentComplete);
                    }
                }
            }
            Err(e) => tracing::warn!("enrollment chunk failed: {e}"),
        }
        stt.audio_buffer.clear();
        return;
    }

    if stt.speaker.has_profile() {
        match stt.speaker.verify(&stt.audio_buffer, stt.device_sample_rate) {
            Ok(score) if score >= speaker::VERIFY_THRESHOLD => {
                tracing::debug!(score, "speaker verified");
            }
            Ok(score) => {
                tracing::info!(score, "speaker rejected");
                stt.bus.emit_voice(VoiceEvent::SpeakerRejected { score });
                stt.audio_buffer.clear();
                return;
            }
            Err(e) => tracing::warn!("speaker verify error: {e}, allowing utterance"),
        }
    }

    stt.audio_buffer.clear();

    let detected = intent::detect(trimmed);
    if matches!(detected, Intent::Ignored) {
        tracing::debug!(text = trimmed, "utterance ignored (no wake phrase)");
        return;
    }

    if matches!(detected, Intent::EnrollVoice) {
        stt.enrolling = true;
        stt.enroll_step = 0;
        stt.speaker.reset_enrollment();
        stt.bus.emit_voice(VoiceEvent::EnrollmentProgress {
            step: 0,
            total: speaker::ENROLL_SAMPLES,
        });
        return;
    }

    if matches!(detected, Intent::ForgetVoice) {
        let _ = stt.speaker.clear_profile();
        return;
    }

    tracing::info!(text = trimmed, ?detected, "voice utterance");
    stt.bus.emit_voice(VoiceEvent::Utterance {
        text: trimmed.to_string(),
    });
    stt.bus.emit_voice(VoiceEvent::IntentDetected(detected));
}

fn schedule_finalize(inner: Arc<Mutex<SttInner>>) {
    thread::spawn(move || {
        if let Ok(mut stt) = inner.lock() {
            finalize_utterance(&mut stt);
            stt.last_partial = Instant::now();
        }
    });
}

fn emit_partial(stt: &mut SttInner, text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    if !stt.user_speech_active {
        stt.user_speech_active = true;
        stt.bus.emit_voice(VoiceEvent::UserSpeechStarted);
    }
    let now = Instant::now();
    if now.duration_since(stt.last_emit) >= PARTIAL_THROTTLE {
        stt.last_emit = now;
        stt.bus.emit_voice(VoiceEvent::PartialTranscript {
            text: trimmed.to_string(),
        });
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::AnyThread;
    use objc2_avf_audio::AVAudioEngine;
    use objc2_foundation::{NSLocale, NSString};
    use objc2_speech::{
        SFSpeechAudioBufferRecognitionRequest, SFSpeechRecognitionTask, SFSpeechRecognizer,
    };

    use tracing::{info, warn};

    use bruno_core::VoiceEvent;

    use super::{
        emit_partial, finalize_utterance, schedule_finalize, EngineKind, SttInner,
        VAD_ENERGY_THRESHOLD, WHISPER_PARTIAL_INTERVAL,
    };
    use crate::speaker;
    use crate::whisper::WhisperEngine;

    /// Cap the captured buffer so a long open mic can't grow without bound
    /// (~30 s at 48 kHz mono).
    const MAX_BUFFER_SAMPLES: usize = 48_000 * 30;

    pub enum SttEngine {
        Apple(AppleEngine),
        Whisper(WhisperSttEngine),
    }

    impl SttEngine {
        pub fn new(
            inner: Arc<Mutex<SttInner>>,
            kind: EngineKind,
            whisper: Option<Arc<WhisperEngine>>,
        ) -> Option<Self> {
            match kind {
                EngineKind::Apple => AppleEngine::new(inner).map(SttEngine::Apple),
                EngineKind::Whisper => {
                    let whisper = whisper?;
                    Some(SttEngine::Whisper(WhisperSttEngine::new(inner, whisper)))
                }
            }
        }

        pub fn is_running(&self) -> bool {
            match self {
                SttEngine::Apple(e) => e.is_running(),
                SttEngine::Whisper(e) => e.is_running(),
            }
        }

        pub fn set_listening(&mut self, enabled: bool) {
            match self {
                SttEngine::Apple(e) => e.set_listening(enabled),
                SttEngine::Whisper(e) => e.set_listening(enabled),
            }
        }
    }

    fn rms(samples: &[f32]) -> f32 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum: f32 = samples.iter().map(|s| s * s).sum();
        (sum / samples.len() as f32).sqrt()
    }

    // ---- Whisper backend -------------------------------------------------

    pub struct WhisperSttEngine {
        inner: Arc<Mutex<SttInner>>,
        listening: Arc<AtomicBool>,
        audio_engine: Retained<AVAudioEngine>,
        whisper: Arc<WhisperEngine>,
    }

    impl WhisperSttEngine {
        fn new(inner: Arc<Mutex<SttInner>>, whisper: Arc<WhisperEngine>) -> Self {
            Self {
                inner,
                listening: Arc::new(AtomicBool::new(false)),
                audio_engine: unsafe { AVAudioEngine::new() },
                whisper,
            }
        }

        pub fn is_running(&self) -> bool {
            self.listening.load(Ordering::Relaxed)
        }

        pub fn set_listening(&mut self, enabled: bool) {
            if enabled {
                self.start();
            } else {
                self.stop();
            }
        }

        fn start(&mut self) {
            if self.listening.load(Ordering::Relaxed) {
                return;
            }
            info!("stt(whisper): starting audio engine");
            self.listening.store(true, Ordering::Relaxed);

            let input_node = unsafe { self.audio_engine.inputNode() };
            let format = unsafe { input_node.outputFormatForBus(0) };
            let sample_rate = unsafe { format.sampleRate() };
            let channels = unsafe { format.channelCount() } as usize;

            let inner_tap = self.inner.clone();
            let tap_block = RcBlock::new(
                move |buffer: NonNull<objc2_avf_audio::AVAudioPCMBuffer>,
                      _time: NonNull<objc2_avf_audio::AVAudioTime>| {
                    let frame_length = unsafe { buffer.as_ref().frameLength() } as usize;
                    if let Ok(mut stt) = inner_tap.try_lock() {
                        if sample_rate > 0.0 {
                            stt.device_sample_rate = sample_rate as f32;
                        }
                        let before = stt.audio_buffer.len();
                        speaker::append_pcm_buffer(
                            &mut stt.audio_buffer,
                            buffer.as_ptr(),
                            frame_length,
                            channels,
                        );
                        // Energy-based VAD: any voiced frame keeps the utterance alive.
                        let energy = rms(&stt.audio_buffer[before..]);
                        if energy > VAD_ENERGY_THRESHOLD {
                            if !stt.user_speech_active {
                                stt.user_speech_active = true;
                                stt.bus.emit_voice(VoiceEvent::UserSpeechStarted);
                            }
                            stt.last_partial = std::time::Instant::now();
                        }
                        if stt.audio_buffer.len() > MAX_BUFFER_SAMPLES {
                            let drop = stt.audio_buffer.len() - MAX_BUFFER_SAMPLES;
                            stt.audio_buffer.drain(..drop);
                        }
                    }
                },
            );
            let tap_ptr = RcBlock::as_ptr(&tap_block);

            unsafe {
                input_node.installTapOnBus_bufferSize_format_block(0, 1024, Some(&format), tap_ptr);
                self.audio_engine.prepare();
                if let Err(err) = self.audio_engine.startAndReturnError() {
                    warn!(
                        "stt(whisper) audio engine failed to start: {}",
                        err.localizedDescription().to_string()
                    );
                    self.listening.store(false, Ordering::Relaxed);
                    if let Ok(stt) = self.inner.lock() {
                        stt.bus
                            .emit_voice(VoiceEvent::ListeningChanged { enabled: false });
                    }
                    return;
                }
            }

            self.spawn_partial_worker();

            if let Ok(stt) = self.inner.lock() {
                if stt.enrolling && stt.enroll_step == 0 {
                    stt.bus.emit_voice(VoiceEvent::EnrollmentProgress {
                        step: 0,
                        total: speaker::ENROLL_SAMPLES,
                    });
                }
                stt.bus
                    .emit_voice(VoiceEvent::ListeningChanged { enabled: true });
            }
            info!("stt(whisper) listening started");
        }

        /// Periodically re-transcribe the growing buffer to produce live partials.
        fn spawn_partial_worker(&self) {
            let inner = self.inner.clone();
            let listening = self.listening.clone();
            let whisper = self.whisper.clone();
            thread::spawn(move || loop {
                thread::sleep(WHISPER_PARTIAL_INTERVAL);
                if !listening.load(Ordering::Relaxed) {
                    break;
                }
                let (samples, sample_rate, active) = match inner.lock() {
                    Ok(stt) => (
                        stt.audio_buffer.clone(),
                        stt.device_sample_rate,
                        stt.user_speech_active,
                    ),
                    Err(_) => continue,
                };
                if !active {
                    continue;
                }
                let pcm16 = speaker::resample_to_16k(&samples, sample_rate);
                if let Some(text) = whisper.transcribe(&pcm16) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        if let Ok(mut stt) = inner.lock() {
                            stt.partial_text = trimmed.to_string();
                            emit_partial(&mut stt, trimmed);
                        }
                    }
                }
            });
        }

        fn stop(&mut self) {
            if !self.listening.load(Ordering::Relaxed) {
                return;
            }
            self.listening.store(false, Ordering::Relaxed);
            unsafe {
                self.audio_engine.inputNode().removeTapOnBus(0);
                self.audio_engine.stop();
            }
            if let Ok(mut stt) = self.inner.lock() {
                if stt.user_speech_active || !stt.audio_buffer.is_empty() {
                    finalize_utterance(&mut stt);
                } else {
                    stt.reset_utterance();
                }
                stt.partial_text.clear();
                stt.bus
                    .emit_voice(VoiceEvent::ListeningChanged { enabled: false });
            }
            info!("stt(whisper) listening stopped");
        }
    }

    impl Drop for WhisperSttEngine {
        fn drop(&mut self) {
            self.stop();
        }
    }

    // ---- Apple backend (SFSpeechRecognizer) ------------------------------

    pub struct AppleEngine {
        inner: Arc<Mutex<SttInner>>,
        listening: Arc<AtomicBool>,
        recognizer: Retained<SFSpeechRecognizer>,
        audio_engine: Retained<AVAudioEngine>,
        request: Option<Retained<SFSpeechAudioBufferRecognitionRequest>>,
        task: Option<Retained<SFSpeechRecognitionTask>>,
    }

    impl AppleEngine {
        pub fn new(inner: Arc<Mutex<SttInner>>) -> Option<Self> {
            let locale = NSLocale::localeWithLocaleIdentifier(&NSString::from_str("en-US"));
            let recognizer = unsafe {
                SFSpeechRecognizer::initWithLocale(SFSpeechRecognizer::alloc(), &locale)?
            };
            if !unsafe { recognizer.isAvailable() } {
                warn!("speech recognizer unavailable");
                return None;
            }
            Some(Self {
                inner,
                listening: Arc::new(AtomicBool::new(false)),
                recognizer,
                audio_engine: unsafe { AVAudioEngine::new() },
                request: None,
                task: None,
            })
        }

        pub fn is_running(&self) -> bool {
            self.listening.load(Ordering::Relaxed)
        }

        pub fn set_listening(&mut self, enabled: bool) {
            if enabled {
                self.start();
            } else {
                self.stop();
            }
        }

        fn start(&mut self) {
            if self.listening.load(Ordering::Relaxed) {
                return;
            }

            info!("stt(apple): starting audio engine");
            self.listening.store(true, Ordering::Relaxed);

            let request = unsafe {
                let req = SFSpeechAudioBufferRecognitionRequest::new();
                req.setShouldReportPartialResults(true);
                req.setRequiresOnDeviceRecognition(false);
                req
            };

            let inner = self.inner.clone();
            let block = RcBlock::new(
                move |result: *mut objc2_speech::SFSpeechRecognitionResult,
                      error: *mut objc2_foundation::NSError| {
                    if !error.is_null() {
                        if let Some(err) = unsafe { error.as_ref() } {
                            warn!(
                                "stt recognition error: {}",
                                err.localizedDescription().to_string()
                            );
                        }
                    }
                    if result.is_null() {
                        return;
                    }
                    let result = unsafe { result.as_ref().unwrap() };
                    let best = unsafe { result.bestTranscription() };
                    let text = unsafe { best.formattedString().to_string() };
                    let is_final = unsafe { result.isFinal() };
                    if let Ok(mut stt) = inner.lock() {
                        stt.partial_text = text.clone();
                        stt.last_partial = std::time::Instant::now();
                        emit_partial(&mut stt, &text);
                        if is_final {
                            info!(text = %text.trim(), "stt final");
                            drop(stt);
                            schedule_finalize(inner.clone());
                        }
                    }
                },
            );

            let task = unsafe {
                self.recognizer
                    .recognitionTaskWithRequest_resultHandler(&request, &block)
            };

            let input_node = unsafe { self.audio_engine.inputNode() };
            let format = unsafe { input_node.outputFormatForBus(0) };
            let sample_rate = unsafe { format.sampleRate() };
            let channels = unsafe { format.channelCount() } as usize;
            let request_for_tap = request.clone();
            let inner_tap = self.inner.clone();
            let tap_block = RcBlock::new(
                move |buffer: NonNull<objc2_avf_audio::AVAudioPCMBuffer>,
                      _time: NonNull<objc2_avf_audio::AVAudioTime>| {
                    unsafe {
                        request_for_tap.appendAudioPCMBuffer(buffer.as_ref());
                    }
                    let frame_length = unsafe { buffer.as_ref().frameLength() } as usize;
                    if let Ok(mut stt) = inner_tap.try_lock() {
                        if sample_rate > 0.0 {
                            stt.device_sample_rate = sample_rate as f32;
                        }
                        speaker::append_pcm_buffer(
                            &mut stt.audio_buffer,
                            buffer.as_ptr(),
                            frame_length,
                            channels,
                        );
                    }
                },
            );
            let tap_ptr = RcBlock::as_ptr(&tap_block);

            unsafe {
                input_node.installTapOnBus_bufferSize_format_block(0, 1024, Some(&format), tap_ptr);
                self.audio_engine.prepare();
                if let Err(err) = self.audio_engine.startAndReturnError() {
                    warn!(
                        "stt audio engine failed to start: {}",
                        err.localizedDescription().to_string()
                    );
                    self.listening.store(false, Ordering::Relaxed);
                    if let Ok(stt) = self.inner.lock() {
                        stt.bus
                            .emit_voice(VoiceEvent::ListeningChanged { enabled: false });
                    }
                    return;
                }
            }

            self.request = Some(request);
            self.task = Some(task);
            if let Ok(stt) = self.inner.lock() {
                if stt.enrolling && stt.enroll_step == 0 {
                    stt.bus.emit_voice(VoiceEvent::EnrollmentProgress {
                        step: 0,
                        total: speaker::ENROLL_SAMPLES,
                    });
                }
                stt.bus
                    .emit_voice(VoiceEvent::ListeningChanged { enabled: true });
            }
            info!("stt listening started");
        }

        fn stop(&mut self) {
            if !self.listening.load(Ordering::Relaxed) {
                return;
            }
            self.listening.store(false, Ordering::Relaxed);
            unsafe {
                self.audio_engine.inputNode().removeTapOnBus(0);
                self.audio_engine.stop();
            }
            if let Some(task) = self.task.take() {
                unsafe { task.cancel() };
            }
            if let Some(request) = self.request.take() {
                unsafe { request.endAudio() };
            }
            if let Ok(mut stt) = self.inner.lock() {
                if !stt.partial_text.trim().is_empty() {
                    finalize_utterance(&mut stt);
                } else {
                    stt.reset_utterance();
                }
                stt.partial_text.clear();
                stt.bus
                    .emit_voice(VoiceEvent::ListeningChanged { enabled: false });
            }
            info!("stt listening stopped");
        }
    }

    impl Drop for AppleEngine {
        fn drop(&mut self) {
            self.stop();
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use std::sync::{Arc, Mutex};

    use super::{EngineKind, SttInner};
    use crate::whisper::WhisperEngine;

    pub struct SttEngine {
        _inner: Arc<Mutex<SttInner>>,
    }

    impl SttEngine {
        pub fn new(
            inner: Arc<Mutex<SttInner>>,
            _kind: EngineKind,
            _whisper: Option<Arc<WhisperEngine>>,
        ) -> Option<Self> {
            Some(Self { _inner: inner })
        }

        pub fn set_listening(&mut self, _enabled: bool) {}

        pub fn is_running(&self) -> bool {
            false
        }
    }
}

pub struct Stt {
    inner: Arc<Mutex<SttInner>>,
    engine: Mutex<Option<platform::SttEngine>>,
    permissions: Arc<PermissionGate>,
    listening: Arc<AtomicBool>,
    denied_notified: AtomicBool,
    config: SttConfig,
    whisper: Arc<Mutex<Option<Arc<WhisperEngine>>>>,
    whisper_status: Arc<AtomicU8>,
}

impl Stt {
    pub fn new(bus: BrunoBus) -> Self {
        let config = SttConfig::load();
        let inner = Arc::new(Mutex::new(SttInner {
            bus: bus.clone(),
            partial_text: String::new(),
            last_partial: Instant::now(),
            last_emit: Instant::now(),
            user_speech_active: false,
            audio_buffer: Vec::new(),
            device_sample_rate: 48_000.0,
            speaker: SpeakerGate::new(),
            enrolling: false,
            enroll_step: 0,
            whisper: None,
        }));

        let whisper = Arc::new(Mutex::new(None));
        let whisper_status = Arc::new(AtomicU8::new(WHISPER_LOADING));

        // Load whisper (and download the model if needed) off the main thread.
        if config.backend != SttBackend::Apple {
            let cfg = config.whisper.clone();
            let slot = whisper.clone();
            let status = whisper_status.clone();
            thread::spawn(move || match WhisperEngine::load(&cfg) {
                Ok(engine) => {
                    if let Ok(mut slot) = slot.lock() {
                        *slot = Some(Arc::new(engine));
                    }
                    status.store(WHISPER_READY, Ordering::Relaxed);
                    tracing::info!("whisper model ready");
                }
                Err(e) => {
                    tracing::warn!("whisper unavailable: {e}");
                    status.store(WHISPER_FAILED, Ordering::Relaxed);
                }
            });
        } else {
            whisper_status.store(WHISPER_FAILED, Ordering::Relaxed);
        }

        let stt = Self {
            inner: inner.clone(),
            engine: Mutex::new(None),
            permissions: PermissionGate::new(),
            listening: Arc::new(AtomicBool::new(false)),
            denied_notified: AtomicBool::new(false),
            config,
            whisper,
            whisper_status,
        };
        stt.spawn_silence_detector();

        if let Ok(mut stt) = inner.lock() {
            if !stt.speaker.has_profile() {
                stt.enrolling = true;
                stt.enroll_step = 0;
                stt.speaker.reset_enrollment();
            }
        }

        stt
    }

    pub fn set_listening(&self, enabled: bool) {
        self.listening.store(enabled, Ordering::Relaxed);
        if enabled {
            self.denied_notified.store(false, Ordering::Relaxed);
            tracing::info!("stt: requesting permissions");
            self.permissions.begin();
        }
    }

    pub fn is_running(&self) -> bool {
        self.engine
            .lock()
            .ok()
            .and_then(|engine| engine.as_ref().map(|e| e.is_running()))
            .unwrap_or(false)
    }

    pub fn wants_listening(&self) -> bool {
        self.listening.load(Ordering::Relaxed)
    }

    /// Resolve which engine to start, or `None` if whisper is still loading.
    fn resolve_engine(&self) -> Option<(EngineKind, Option<Arc<WhisperEngine>>)> {
        match self.config.backend {
            SttBackend::Apple => Some((EngineKind::Apple, None)),
            SttBackend::Whisper | SttBackend::Auto => {
                match self.whisper_status.load(Ordering::Relaxed) {
                    WHISPER_LOADING => None,
                    WHISPER_READY => {
                        let engine = self.whisper.lock().ok().and_then(|g| g.clone());
                        match engine {
                            Some(_) => Some((EngineKind::Whisper, engine)),
                            None => Some((EngineKind::Apple, None)),
                        }
                    }
                    _ => {
                        tracing::warn!("whisper unavailable; falling back to Apple STT");
                        Some((EngineKind::Apple, None))
                    }
                }
            }
        }
    }

    /// Must be called from the winit main thread each frame.
    pub fn poll_main(&self) {
        let want = self.listening.load(Ordering::Relaxed);
        let Ok(mut engine) = self.engine.lock() else {
            return;
        };

        if !want {
            if let Some(engine) = engine.as_mut() {
                if engine.is_running() {
                    engine.set_listening(false);
                }
            }
            return;
        }

        if engine.as_ref().is_some_and(|e| e.is_running()) {
            return;
        }

        match self.permissions.poll() {
            None => {
                tracing::trace!("stt: waiting for permissions");
            }
            Some(false) => {
                if !self.denied_notified.swap(true, Ordering::AcqRel) {
                    if let Ok(stt) = self.inner.lock() {
                        stt.bus.emit_voice(VoiceEvent::PermissionDenied);
                    }
                }
                self.listening.store(false, Ordering::Relaxed);
            }
            Some(true) => {
                if engine.is_none() {
                    let Some((kind, whisper)) = self.resolve_engine() else {
                        tracing::trace!("stt: waiting for whisper model");
                        return;
                    };
                    tracing::info!(?kind, "stt: permissions granted, starting engine");
                    if let Some(w) = whisper.clone() {
                        if let Ok(mut inner) = self.inner.lock() {
                            inner.whisper = Some(w);
                        }
                    }
                    *engine = platform::SttEngine::new(self.inner.clone(), kind, whisper);
                }
                if let Some(engine) = engine.as_mut() {
                    engine.set_listening(true);
                }
            }
        }
    }

    fn spawn_silence_detector(&self) {
        let inner = self.inner.clone();
        let listening = self.listening.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(100));
            if !listening.load(Ordering::Relaxed) {
                continue;
            }
            let should_finalize = inner.lock().ok().is_some_and(|stt| {
                !stt.partial_text.trim().is_empty()
                    && stt.last_partial.elapsed() >= SILENCE_THRESHOLD
            });
            if should_finalize {
                schedule_finalize(inner.clone());
            }
        });
    }
}
