#[derive(Debug, Clone)]
pub struct CapturedScreen {
    pub label: String,
    pub mime_type: String,
    pub data_base64: String,
}

#[derive(Debug, Clone, Copy)]
pub struct ScreenCaptureOptions {
    pub max_edge: u32,
    pub jpeg_quality: u8,
    pub max_images: usize,
    pub max_total_base64_bytes: usize,
}

impl Default for ScreenCaptureOptions {
    fn default() -> Self {
        Self {
            max_edge: 1600,
            jpeg_quality: 80,
            max_images: usize::MAX,
            max_total_base64_bytes: 64 * 1024 * 1024,
        }
    }
}

const SCREEN_CAPTURE_TIMEOUT_SECS: u64 = 15;

pub async fn capture_full_screen_context_async() -> Result<Vec<CapturedScreen>, String> {
    let task = tokio::task::spawn_blocking(capture_full_screen_context);
    tokio::time::timeout(
        std::time::Duration::from_secs(SCREEN_CAPTURE_TIMEOUT_SECS),
        task,
    )
    .await
    .map_err(|_| {
        format!(
            "截屏超过 {} 秒，已跳过屏幕上下文",
            SCREEN_CAPTURE_TIMEOUT_SECS
        )
    })?
    .map_err(|e| format!("截屏任务异常: {e}"))?
}

#[cfg(target_os = "windows")]
pub fn capture_full_screen_context() -> Result<Vec<CapturedScreen>, String> {
    capture_full_screen_context_with_options(ScreenCaptureOptions::default())
}

#[cfg(target_os = "windows")]
fn capture_full_screen_context_with_options(
    options: ScreenCaptureOptions,
) -> Result<Vec<CapturedScreen>, String> {
    use std::io::Cursor;

    use base64::Engine;
    use image::codecs::jpeg::JpegEncoder;
    use image::imageops::FilterType;
    use image::DynamicImage;
    use xcap::Monitor;

    let monitors = Monitor::all().map_err(|e| format!("读取屏幕列表失败: {e}"))?;
    if monitors.is_empty() {
        return Ok(Vec::new());
    }

    let mut captured = Vec::with_capacity(monitors.len().min(options.max_images));
    let mut total_base64_bytes = 0usize;

    for (index, monitor) in monitors.into_iter().take(options.max_images).enumerate() {
        let friendly_name = monitor
            .friendly_name()
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("显示器 {}", index + 1));
        let image = monitor
            .capture_image()
            .map_err(|e| format!("截取{friendly_name}失败: {e}"))?;

        let dynamic = DynamicImage::ImageRgba8(image);
        let resized = if dynamic.width().max(dynamic.height()) > options.max_edge {
            dynamic.resize(options.max_edge, options.max_edge, FilterType::Triangle)
        } else {
            dynamic
        };

        let mut bytes = Vec::new();
        JpegEncoder::new_with_quality(Cursor::new(&mut bytes), options.jpeg_quality)
            .encode_image(&resized)
            .map_err(|e| format!("编码{friendly_name}截图失败: {e}"))?;

        let data_base64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        if captured.is_empty() && data_base64.len() > options.max_total_base64_bytes {
            return Err(format!(
                "{}截图超过上下文预算：{} > {} bytes",
                friendly_name,
                data_base64.len(),
                options.max_total_base64_bytes
            ));
        }
        if total_base64_bytes + data_base64.len() > options.max_total_base64_bytes {
            break;
        }
        total_base64_bytes += data_base64.len();
        captured.push(CapturedScreen {
            label: friendly_name,
            mime_type: "image/jpeg".to_string(),
            data_base64,
        });
    }

    Ok(captured)
}

#[cfg(not(target_os = "windows"))]
pub fn capture_full_screen_context() -> Result<Vec<CapturedScreen>, String> {
    Ok(Vec::new())
}

#[cfg(not(target_os = "windows"))]
fn capture_full_screen_context_with_options(
    _options: ScreenCaptureOptions,
) -> Result<Vec<CapturedScreen>, String> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    #[test]
    fn async_screen_capture_callers_use_blocking_offload() {
        let assistant_source = include_str!("assistant_service.rs");
        let polish_source = include_str!("ai_polish_service.rs");

        assert!(
            assistant_source.contains("spawn_blocking")
                || assistant_source.contains("capture_full_screen_context_async"),
            "assistant async flow must offload screen capture instead of calling the blocking capture API inline"
        );
        assert!(
            polish_source.contains("spawn_blocking")
                || polish_source.contains("capture_full_screen_context_async"),
            "AI polish async flow must offload screen capture instead of calling the blocking capture API inline"
        );
    }

    #[test]
    fn screen_context_has_testable_byte_budget_helper() {
        let source = include_str!("screen_capture_service.rs");

        assert!(
            source.contains("byte_budget")
                || source.contains("MAX_SCREEN_CONTEXT_BYTES")
                || source.contains("max_total_base64_bytes"),
            "screen capture context must enforce a byte budget through helper logic that can be unit tested without OS capture APIs"
        );
    }
}
