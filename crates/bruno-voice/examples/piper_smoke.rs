//! Verify libpiper synthesis. Requires the `piper` feature + PIPER_DIR.
//! Run:
//!   PIPER_DIR=~/.config/bruno/piper cargo run -p bruno-voice \
//!     --features piper --example piper_smoke -- <install_dir> <model.onnx> [out.wav]

#[cfg(not(feature = "piper"))]
fn main() {
    eprintln!("build with --features piper");
}

#[cfg(feature = "piper")]
fn main() {
    use bruno_voice::config::PiperConfig;
    use bruno_voice::piper::PiperTts;

    let mut args = std::env::args().skip(1);
    let install_dir = args.next().expect("usage: piper_smoke <install_dir> <model.onnx> [out.wav]");
    let model_path = args.next().expect("model path required");
    let out = args.next().unwrap_or_else(|| "/tmp/piper_smoke.wav".to_string());

    let cfg = PiperConfig {
        install_dir,
        model_path,
        config_path: String::new(),
        length_scale: 1.0,
        noise_scale: 0.667,
        noise_w_scale: 0.8,
        speaker_id: 0,
    };

    let tts = PiperTts::new(&cfg).expect("piper init");
    let (samples, sr) = tts.synthesize("Hello, I am Bruno. Whisper hears you and now I speak.").expect("synth");
    println!("{} samples @ {} Hz ({:.2}s)", samples.len(), sr, samples.len() as f32 / sr as f32);

    let spec = hound::WavSpec { channels: 1, sample_rate: sr, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(&out, spec).unwrap();
    for s in samples { w.write_sample(s).unwrap(); }
    w.finalize().unwrap();
    println!("wrote {out}");
}
