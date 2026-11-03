pub(crate) fn frame_secs(frame: usize, sample_rate: u32) -> f32 {
    if sample_rate == 0 {
        0.0
    } else {
        frame as f32 / sample_rate as f32
    }
}

pub(crate) fn format_secs(secs: f32) -> String {
    format!("{secs:.2}s")
}

pub(crate) fn format_clock(secs: f32) -> String {
    let secs = secs.max(0.0);
    let mins = (secs / 60.0) as u32;
    let rem = secs - mins as f32 * 60.0;
    format!("{mins}:{rem:04.1}")
}

pub(crate) fn format_rate(sample_rate: u32) -> String {
    if sample_rate % 1000 == 0 {
        format!("{} kHz", sample_rate / 1000)
    } else {
        format!("{sample_rate} Hz")
    }
}

pub(crate) fn channel_label(channels: u16) -> String {
    match channels {
        1 => "mono".into(),
        2 => "stereo".into(),
        _ => format!("{channels} ch"),
    }
}
