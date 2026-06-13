//! Speaker verification: enroll profile, verify utterances.

mod embed;
#[cfg(target_os = "macos")]
mod pcm;
mod profile;

use std::path::PathBuf;

pub use profile::VoiceProfile;

pub const ENROLL_SAMPLES: u8 = 3;
pub const VERIFY_THRESHOLD: f32 = 0.72;
const MAX_AUDIO_SECS: f32 = 1.2;

#[cfg(target_os = "macos")]
pub use pcm::append_pcm_buffer;

#[cfg(not(target_os = "macos"))]
pub fn append_pcm_buffer(
    buffer: &mut Vec<f32>,
    _pcm: *const (),
    _frame_length: usize,
    _channels: usize,
) {
    let _ = buffer;
}

pub struct SpeakerGate {
    profile: Option<VoiceProfile>,
    enroll_chunks: Vec<Vec<f32>>,
    profile_path: PathBuf,
}

impl SpeakerGate {
    pub fn new() -> Self {
        let profile_path = profile_dir().join("voice_profile.bin");
        let profile = VoiceProfile::load(&profile_path).ok().flatten();
        Self {
            profile,
            enroll_chunks: Vec::new(),
            profile_path,
        }
    }

    pub fn has_profile(&self) -> bool {
        self.profile.is_some()
    }

    pub fn reset_enrollment(&mut self) {
        self.enroll_chunks.clear();
    }

    pub fn enroll_chunk(&mut self, audio: &[f32], sample_rate: f32) -> Result<(), String> {
        let audio = resample_to_16k(audio, sample_rate);
        if audio.len() < 1600 {
            return Err("audio too short".into());
        }
        let clipped = trim_audio(&audio, MAX_AUDIO_SECS);
        self.enroll_chunks.push(clipped);
        Ok(())
    }

    pub fn finish_enrollment(&mut self) -> Result<(), String> {
        if self.enroll_chunks.len() < ENROLL_SAMPLES as usize {
            return Err("not enough enrollment samples".into());
        }
        let embeddings: Vec<Vec<f32>> = self
            .enroll_chunks
            .iter()
            .map(|chunk| embed::embedding(chunk))
            .collect::<Result<Vec<_>, _>>()?;
        let dim = embeddings[0].len();
        let mut avg = vec![0.0f32; dim];
        for emb in &embeddings {
            for (i, v) in emb.iter().enumerate() {
                avg[i] += v;
            }
        }
        for v in &mut avg {
            *v /= embeddings.len() as f32;
        }
        embed::l2_normalize(&mut avg);
        let profile = VoiceProfile { embedding: avg };
        profile.save(&self.profile_path)?;
        self.profile = Some(profile);
        self.enroll_chunks.clear();
        Ok(())
    }

    pub fn verify(&self, audio: &[f32], sample_rate: f32) -> Result<f32, String> {
        let profile = self.profile.as_ref().ok_or("no profile")?;
        let audio = resample_to_16k(audio, sample_rate);
        let clipped = trim_audio(&audio, MAX_AUDIO_SECS);
        if clipped.len() < 1600 {
            return Err("audio too short".into());
        }
        let mut emb = embed::embedding(&clipped)?;
        embed::l2_normalize(&mut emb);
        Ok(embed::cosine_similarity(&profile.embedding, &emb))
    }

    pub fn clear_profile(&mut self) -> Result<(), String> {
        if self.profile_path.exists() {
            std::fs::remove_file(&self.profile_path).map_err(|e| e.to_string())?;
        }
        self.profile = None;
        self.enroll_chunks.clear();
        Ok(())
    }
}

fn profile_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".bruno")
}

fn trim_audio(audio: &[f32], max_secs: f32) -> Vec<f32> {
    let max_samples = (16_000.0 * max_secs) as usize;
    if audio.len() <= max_samples {
        audio.to_vec()
    } else {
        audio[audio.len() - max_samples..].to_vec()
    }
}

/// Resample mono PCM to 16 kHz (whisper's required input rate).
pub fn resample_to_16k(audio: &[f32], sample_rate: f32) -> Vec<f32> {
    if audio.is_empty() {
        return Vec::new();
    }
    if (sample_rate - 16_000.0).abs() < 1.0 {
        return audio.to_vec();
    }
    let mut out = audio.to_vec();
    resample_in_place(&mut out, sample_rate, 16_000.0);
    out
}

fn resample_in_place(samples: &mut Vec<f32>, from_rate: f32, to_rate: f32) {
    if (from_rate - to_rate).abs() < 1.0 || samples.is_empty() {
        return;
    }
    let ratio = from_rate / to_rate;
    let out_len = (samples.len() as f32 / ratio) as usize;
    if out_len == 0 {
        samples.clear();
        return;
    }
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f32 * ratio;
        let idx = src as usize;
        let frac = src - idx as f32;
        let a = samples.get(idx).copied().unwrap_or(0.0);
        let b = samples.get(idx + 1).copied().unwrap_or(a);
        out.push(a + (b - a) * frac);
    }
    *samples = out;
}
