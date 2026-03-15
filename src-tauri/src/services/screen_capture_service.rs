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

#[cfg(target_os = "macos")]
pub fn capture_full_screen_context() -> Result<Vec<CapturedScreen>, String> {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use base64::Engine;

    let temp_file = std::env::temp_dir().join(format!(
        "light-whisper-screen-{}.jpg",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default()
    ));

    let status = std::process::Command::new("screencapture")
        .arg("-x")
        .arg("-t")
        .arg("jpg")
        .arg(&temp_file)
        .status()
        .map_err(|e| format!("启动 screencapture 失败: {e}"))?;

    if !status.success() {
        let _ = fs::remove_file(&temp_file);
        return Err(
            "截屏失败，请在 系统设置 > 隐私与安全性 > 屏幕录制 中允许本应用后重试".to_string(),
        );
    }

    let bytes = fs::read(&temp_file).map_err(|e| format!("读取截图文件失败: {e}"))?;
    let _ = fs::remove_file(&temp_file);

    if bytes.is_empty() {
        return Ok(Vec::new());
    }

    Ok(vec![CapturedScreen {
        label: "当前屏幕".to_string(),
        mime_type: "image/jpeg".to_string(),
        data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
    }])
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
pub fn capture_full_screen_context() -> Result<Vec<CapturedScreen>, String> {
    Ok(Vec::new())
}
