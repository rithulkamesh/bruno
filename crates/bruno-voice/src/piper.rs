//! Piper neural TTS via the `libpiper` C API (`OHF-Voice/piper1-gpl`).
//!
//! Gated behind the `piper` cargo feature. Build `libpiper` first (its cmake
//! installs `libpiper`, `libonnxruntime`, and `espeak-ng-data`), point
//! `[tts.piper].install_dir` at the install prefix, and set `PIPER_DIR` to the
//! same prefix when building so `build.rs` can link the libraries.
//!
//! NOTE: libpiper / espeak-ng are GPL-3.0. Linking this in makes the distributed
//! binary GPL-3.0.

use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use std::path::Path;

use crate::config::PiperConfig;

#[repr(C)]
struct PiperSynthesizer {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PiperSynthesizeOptions {
    speaker_id: c_int,
    length_scale: f32,
    noise_scale: f32,
    noise_w_scale: f32,
}

#[repr(C)]
struct PiperAudioChunk {
    samples: *const f32,
    num_samples: usize,
    sample_rate: c_int,
    is_last: bool,
    phonemes: *const u32,
    num_phonemes: usize,
    phoneme_ids: *const c_int,
    num_phoneme_ids: usize,
    alignments: *const c_int,
    num_alignments: usize,
}

const PIPER_DONE: c_int = 1;

unsafe extern "C" {
    fn piper_create(
        model_path: *const c_char,
        config_path: *const c_char,
        espeak_data_path: *const c_char,
    ) -> *mut PiperSynthesizer;
    fn piper_free(synth: *mut PiperSynthesizer);
    fn piper_default_synthesize_options(synth: *mut PiperSynthesizer) -> PiperSynthesizeOptions;
    fn piper_synthesize_start(
        synth: *mut PiperSynthesizer,
        text: *const c_char,
        options: *const PiperSynthesizeOptions,
    ) -> c_int;
    fn piper_synthesize_next(synth: *mut PiperSynthesizer, chunk: *mut PiperAudioChunk) -> c_int;
}

/// Safe wrapper around a libpiper synthesizer.
///
/// Not `Sync`: all calls mutate the synthesizer's internal state, so callers must
/// serialize use (we keep it behind a `Mutex`). It is `Send` so it can live on a
/// worker thread.
pub struct PiperTts {
    synth: *mut PiperSynthesizer,
    options: PiperSynthesizeOptions,
}

// SAFETY: the synthesizer pointer is owned solely by this struct; we never share
// it across threads without external synchronization.
unsafe impl Send for PiperTts {}

impl PiperTts {
    pub fn new(cfg: &PiperConfig) -> Result<Self, String> {
        if cfg.model_path.is_empty() {
            return Err("tts.piper.model_path is not set".into());
        }
        if cfg.install_dir.is_empty() {
            return Err("tts.piper.install_dir is not set".into());
        }

        let model = cstr(&cfg.model_path)?;
        let config = if cfg.config_path.is_empty() {
            None
        } else {
            Some(cstr(&cfg.config_path)?)
        };
        let espeak_dir = Path::new(&cfg.install_dir).join("espeak-ng-data");
        let espeak = cstr(&espeak_dir.to_string_lossy())?;

        let synth = unsafe {
            piper_create(
                model.as_ptr(),
                config.as_ref().map(|c| c.as_ptr()).unwrap_or(std::ptr::null()),
                espeak.as_ptr(),
            )
        };
        if synth.is_null() {
            return Err(format!("piper_create failed for {}", cfg.model_path));
        }

        let mut options = unsafe { piper_default_synthesize_options(synth) };
        options.length_scale = cfg.length_scale;
        options.noise_scale = cfg.noise_scale;
        options.noise_w_scale = cfg.noise_w_scale;
        options.speaker_id = cfg.speaker_id;

        Ok(Self { synth, options })
    }

    /// Synthesize `text` to mono f32 samples. Returns the samples and sample rate.
    pub fn synthesize(&self, text: &str) -> Result<(Vec<f32>, u32), String> {
        let c_text = cstr(text)?;
        let rc = unsafe { piper_synthesize_start(self.synth, c_text.as_ptr(), &self.options) };
        if rc < 0 {
            return Err(format!("piper_synthesize_start failed ({rc})"));
        }

        let mut samples = Vec::new();
        let mut sample_rate = 22_050u32;
        loop {
            let mut chunk = PiperAudioChunk {
                samples: std::ptr::null(),
                num_samples: 0,
                sample_rate: 0,
                is_last: false,
                phonemes: std::ptr::null(),
                num_phonemes: 0,
                phoneme_ids: std::ptr::null(),
                num_phoneme_ids: 0,
                alignments: std::ptr::null(),
                num_alignments: 0,
            };
            let rc = unsafe { piper_synthesize_next(self.synth, &mut chunk) };
            if rc < 0 {
                return Err(format!("piper_synthesize_next failed ({rc})"));
            }
            if chunk.sample_rate > 0 {
                sample_rate = chunk.sample_rate as u32;
            }
            if !chunk.samples.is_null() && chunk.num_samples > 0 {
                let slice = unsafe { std::slice::from_raw_parts(chunk.samples, chunk.num_samples) };
                samples.extend_from_slice(slice);
            }
            if rc == PIPER_DONE {
                break;
            }
        }

        Ok((samples, sample_rate))
    }
}

impl Drop for PiperTts {
    fn drop(&mut self) {
        if !self.synth.is_null() {
            unsafe { piper_free(self.synth) };
            self.synth = std::ptr::null_mut();
        }
    }
}

fn cstr(s: &str) -> Result<CString, String> {
    CString::new(s).map_err(|_| "string contains an interior NUL byte".to_string())
}
