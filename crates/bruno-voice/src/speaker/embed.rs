//! Log-mel spectral embedding for on-device speaker verification.

const TARGET_SR: usize = 16_000;
const N_MELS: usize = 40;
const N_FFT: usize = 512;
const HOP: usize = 160;
/// Cap embedding work to ~1s of audio so verify stays fast.
const MAX_EMBED_SAMPLES: usize = 16_000;

pub fn embedding(audio: &[f32]) -> Result<Vec<f32>, String> {
    let audio = clip_for_embed(audio);
    if audio.len() < N_FFT {
        return Err("audio too short".into());
    }
    Ok(spectral_embedding(&audio))
}

fn clip_for_embed(audio: &[f32]) -> Vec<f32> {
    if audio.len() <= MAX_EMBED_SAMPLES {
        audio.to_vec()
    } else {
        audio[audio.len() - MAX_EMBED_SAMPLES..].to_vec()
    }
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
    }
    dot.clamp(-1.0, 1.0)
}

pub fn l2_normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-6 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

fn spectral_embedding(audio: &[f32]) -> Vec<f32> {
    mel_spectrogram_mean(audio)
}

fn mel_spectrogram_mean(audio: &[f32]) -> Vec<f32> {
    let frames = frame_count(audio.len());
    if frames == 0 {
        return vec![0.0; N_MELS];
    }
    let mut mel_sum = vec![0.0f32; N_MELS];
    for f in 0..frames {
        let start = f * HOP;
        let frame: Vec<f32> = (0..N_FFT)
            .map(|i| {
                let idx = start + i;
                if idx < audio.len() {
                    let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N_FFT as f32).cos();
                    audio[idx] * w
                } else {
                    0.0
                }
            })
            .collect();
        let power = power_spectrum(&frame);
        let mel = apply_mel_filterbank(&power);
        for (i, v) in mel.iter().enumerate() {
            mel_sum[i] += v.ln_1p();
        }
    }
    for v in &mut mel_sum {
        *v /= frames as f32;
    }
    l2_normalize(&mut mel_sum);
    mel_sum
}

fn frame_count(len: usize) -> usize {
    if len < N_FFT {
        0
    } else {
        1 + (len - N_FFT) / HOP
    }
}

fn power_spectrum(frame: &[f32]) -> Vec<f32> {
    let n = frame.len();
    let mut re = frame.to_vec();
    let mut im = vec![0.0f32; n];
    fft_in_place(&mut re, &mut im);
    re.iter()
        .zip(im.iter())
        .take(n / 2 + 1)
        .map(|(r, i)| r * r + i * i)
        .collect()
}

fn apply_mel_filterbank(power: &[f32]) -> Vec<f32> {
    let n_bins = power.len();
    let mut out = vec![0.0f32; N_MELS];
    for m in 0..N_MELS {
        let mel_low = hz_to_mel(m as f32 * 2595.0 / N_MELS as f32);
        let mel_high = hz_to_mel((m + 2) as f32 * 2595.0 / N_MELS as f32);
        let bin_low = ((n_bins as f32) * mel_low / (TARGET_SR as f32 / 2.0)) as usize;
        let bin_high = ((n_bins as f32) * mel_high / (TARGET_SR as f32 / 2.0))
            .min(n_bins as f32) as usize;
        for b in bin_low..bin_high {
            if b < power.len() {
                out[m] += power[b];
            }
        }
    }
    out
}

fn hz_to_mel(hz: f32) -> f32 {
    700.0 * ((hz / 2595.0).exp() - 1.0)
}

fn fft_in_place(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    if n <= 1 {
        return;
    }
    let half = n / 2;
    let mut even_re: Vec<f32> = re.iter().step_by(2).copied().collect();
    let mut even_im: Vec<f32> = im.iter().step_by(2).copied().collect();
    let mut odd_re: Vec<f32> = re.iter().skip(1).step_by(2).copied().collect();
    let mut odd_im: Vec<f32> = im.iter().skip(1).step_by(2).copied().collect();
    fft_in_place(&mut even_re, &mut even_im);
    fft_in_place(&mut odd_re, &mut odd_im);
    for k in 0..half {
        let angle = -2.0 * std::f32::consts::PI * k as f32 / n as f32;
        let (wr, wi) = (angle.cos(), angle.sin());
        let tr = wr * odd_re[k] - wi * odd_im[k];
        let ti = wr * odd_im[k] + wi * odd_re[k];
        re[k] = even_re[k] + tr;
        im[k] = even_im[k] + ti;
        re[k + half] = even_re[k] - tr;
        im[k + half] = even_im[k] - ti;
    }
}
