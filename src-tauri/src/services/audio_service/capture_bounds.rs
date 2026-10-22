pub(crate) fn capped_frame_count(
    input_len: usize,
    channels: usize,
    used: usize,
    cap: usize,
) -> usize {
    let chans = channels.max(1);
    let allowed_mono_samples = cap.saturating_sub(used);
    let frame_count = input_len / chans;
    frame_count.min(allowed_mono_samples)
}
