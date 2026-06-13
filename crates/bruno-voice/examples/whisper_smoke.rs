//! End-to-end whisper check: transcribe a 16 kHz mono float WAV.
//! Generate one first, e.g.:
//!   say -v Samantha "testing whisper one two three" -o /tmp/say.aiff
//!   afconvert /tmp/say.aiff /tmp/say.wav -d LEF32@16000 -f WAVE -c 1
//! Run: cargo run -p bruno-voice --example whisper_smoke -- /tmp/say.wav [model]

use bruno_voice::config::WhisperConfig;
use bruno_voice::whisper::WhisperEngine;

fn main() {
    tracing_subscriber::fmt::init();
    let mut args = std::env::args().skip(1);
    let wav = args.next().expect("usage: whisper_smoke <wav> [model]");
    let model = args.next().unwrap_or_else(|| "tiny.en".to_string());

    let reader = hound::WavReader::open(&wav).expect("open wav");
    let spec = reader.spec();
    println!("wav: {} Hz, {} ch, {:?}", spec.sample_rate, spec.channels, spec.sample_format);
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.into_samples::<f32>().filter_map(Result::ok).collect(),
        hound::SampleFormat::Int => reader
            .into_samples::<i32>()
            .filter_map(Result::ok)
            .map(|s| s as f32 / i32::MAX as f32)
            .collect(),
    };
    println!("{} samples (~{:.1}s)", samples.len(), samples.len() as f32 / 16_000.0);

    let cfg = WhisperConfig {
        model,
        model_path: String::new(),
        language: "en".to_string(),
    };
    let engine = WhisperEngine::load(&cfg).expect("load whisper");
    match engine.transcribe(&samples) {
        Some(text) => println!("TRANSCRIPT: {:?}", text.trim()),
        None => println!("(no transcript)"),
    }
}
