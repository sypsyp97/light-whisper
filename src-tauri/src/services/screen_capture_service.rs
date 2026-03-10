#[derive(Debug, Clone)]
pub struct CapturedScreen {
    pub label: String,
    pub mime_type: String,
    pub data_base64: String,
}

#[cfg(target_os = "windows")]
pub fn capture_full_screen_context() -> Result<Vec<CapturedScreen>, String> {
    use std::io::Cursor;

    use base64::Engine;
    use image::codecs::jpeg::JpegEncoder;
    use image::imageops::FilterType;
    use image::DynamicImage;
    use xcap::Monitor;

    const MAX_EDGE: u32 = 1600;
    const JPEG_QUALITY: u8 = 80;

    let monitors = Monitor::all().map_err(|e| format!("读取屏幕列表失败: {e}"))?;
    if monitors.is_empty() {
        return Ok(Vec::new());
    }

    let mut captured = Vec::with_capacity(monitors.len());

    for (index, monitor) in monitors.into_iter().enumerate() {
        let friendly_name = monitor
            .friendly_name()
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("显示器 {}", index + 1));
        let image = monitor
            .capture_image()
            .map_err(|e| format!("截取{friendly_name}失败: {e}"))?;

        let dynamic = DynamicImage::ImageRgba8(image);
        let resized = if dynamic.width().max(dynamic.height()) > MAX_EDGE {
            dynamic.resize(MAX_EDGE, MAX_EDGE, FilterType::Triangle)
        } else {
            dynamic
        };

        let mut bytes = Vec::new();
        JpegEncoder::new_with_quality(Cursor::new(&mut bytes), JPEG_QUALITY)
            .encode_image(&resized)
            .map_err(|e| format!("编码{friendly_name}截图失败: {e}"))?;

        captured.push(CapturedScreen {
            label: friendly_name,
            mime_type: "image/jpeg".to_string(),
            data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        });
    }

    Ok(captured)
}

#[cfg(not(target_os = "windows"))]
pub fn capture_full_screen_context() -> Result<Vec<CapturedScreen>, String> {
    Ok(Vec::new())
}
