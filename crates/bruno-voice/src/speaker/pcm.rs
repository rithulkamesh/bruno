#[cfg(target_os = "macos")]
pub fn append_pcm_buffer(
    buffer: &mut Vec<f32>,
    pcm: *const objc2_avf_audio::AVAudioPCMBuffer,
    frame_length: usize,
    channels: usize,
) {
    if pcm.is_null() || frame_length == 0 {
        return;
    }
    let pcm = unsafe { &*pcm };
    let float_data = unsafe { pcm.floatChannelData() };
    if float_data.is_null() {
        return;
    }
    let ch0_ptr = unsafe { *float_data }.as_ptr();
    if ch0_ptr.is_null() {
        return;
    }
    let ch1_ptr = if channels > 1 {
        Some(unsafe { *float_data.add(1) }.as_ptr())
    } else {
        None
    };

    for i in 0..frame_length {
        let mut sample = unsafe { *ch0_ptr.add(i) };
        if let Some(ch1) = ch1_ptr {
            sample = (sample + unsafe { *ch1.add(i) }) * 0.5;
        }
        buffer.push(sample);
    }
}
