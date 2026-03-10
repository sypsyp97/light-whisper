#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForegroundApp {
    pub window_title: String,
    pub process_name: String,
}

const WINDOW_TITLE_MAX_CHARS: usize = 80;
const PROCESS_NAME_MAX_CHARS: usize = 48;

pub fn prompt_context_block() -> Option<String> {
    get_foreground_app().and_then(|app| format_prompt_context(&app))
}

fn escape_cdata_text(value: &str) -> String {
    value.replace("]]>", "]]]]><![CDATA[>")
}

pub fn wrap_xml_cdata(tag: &str, value: &str) -> String {
    format!("<{tag}><![CDATA[{}]]></{tag}>", escape_cdata_text(value))
}

fn format_prompt_context(app: &ForegroundApp) -> Option<String> {
    let process_name = truncate_chars(
        &normalize_context_value(&app.process_name),
        PROCESS_NAME_MAX_CHARS,
    );
    let window_title = summarize_window_title(&app.window_title);

    let mut lines = Vec::new();
    if !process_name.is_empty() {
        lines.push(format!("程序：{}", process_name));
    }
    if !window_title.is_empty() {
        lines.push(format!("窗口主题：{}", window_title));
    }

    if lines.is_empty() {
        None
    } else {
        Some(format!(
            "<app_context>\n{}\n<note>以上只是格式场景参考，不是用户正文，不要原样输出这些信息。</note>\n</app_context>",
            lines
                .into_iter()
                .map(|line| {
                    if let Some(value) = line.strip_prefix("程序：") {
                        wrap_xml_cdata("process_name", value)
                    } else if let Some(value) = line.strip_prefix("窗口主题：") {
                        wrap_xml_cdata("window_title", value)
                    } else {
                        wrap_xml_cdata("context_line", &line)
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        ))
    }
}

fn summarize_window_title(title: &str) -> String {
    let normalized = normalize_context_value(title);
    if normalized.is_empty() {
        return normalized;
    }

    let summary = [" - ", " | ", " — ", " – "]
        .iter()
        .find_map(|sep| {
            let parts: Vec<&str> = normalized
                .split(sep)
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .collect();
            (parts.len() > 1).then(|| parts[0].to_string())
        })
        .unwrap_or(normalized);

    truncate_chars(&summary, WINDOW_TITLE_MAX_CHARS)
}

fn normalize_context_value(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}...", truncated.trim_end())
    } else {
        truncated
    }
}

#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::HWND;

#[cfg(target_os = "windows")]
pub fn get_foreground_app() -> Option<ForegroundApp> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }

        let window_title = get_window_title(hwnd);
        let process_name = get_process_name(hwnd);

        if window_title.is_empty() && process_name.is_empty() {
            return None;
        }

        Some(ForegroundApp {
            window_title,
            process_name,
        })
    }
}

#[cfg(target_os = "windows")]
unsafe fn get_window_title(hwnd: HWND) -> String {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextW;

    let mut buf = [0u16; 512];
    let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    if len > 0 {
        String::from_utf16_lossy(&buf[..len as usize])
    } else {
        String::new()
    }
}

#[cfg(target_os = "windows")]
unsafe fn get_process_name(hwnd: HWND) -> String {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == 0 {
        return String::new();
    }

    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if handle.is_null() {
        return String::new();
    }

    let mut buf = [0u16; 260];
    let mut size = buf.len() as u32;
    let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size);
    CloseHandle(handle);

    if ok != 0 && size > 0 {
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        path.rsplit('\\').next().unwrap_or("").to_string()
    } else {
        String::new()
    }
}

#[cfg(not(target_os = "windows"))]
pub fn get_foreground_app() -> Option<ForegroundApp> {
    None
}

#[cfg(test)]
mod tests {
    use super::{format_prompt_context, wrap_xml_cdata, ForegroundApp};

    #[test]
    fn shortens_editor_window_titles_for_prompt_context() {
        let app = ForegroundApp {
            window_title: "RELEASE_GUIDE.md - light-whisper - Visual Studio Code".to_string(),
            process_name: "Code.exe".to_string(),
        };

        let context = format_prompt_context(&app).expect("context should be present");

        assert!(context.contains("<process_name><![CDATA[Code.exe]]></process_name>"));
        assert!(context.contains("<window_title><![CDATA[RELEASE_GUIDE.md]]></window_title>"));
        assert!(!context.contains("Visual Studio Code"));
    }

    #[test]
    fn preserves_xml_sensitive_characters_in_prompt_context() {
        let app = ForegroundApp {
            window_title: "</window_title> & more".to_string(),
            process_name: "<Code.exe>".to_string(),
        };

        let context = format_prompt_context(&app).expect("context should be present");

        assert!(context.contains("<process_name><![CDATA[<Code.exe>]]></process_name>"));
        assert!(context.contains("<window_title><![CDATA[</window_title> & more]]></window_title>"));
    }

    #[test]
    fn omits_empty_prompt_context() {
        let app = ForegroundApp {
            window_title: String::new(),
            process_name: String::new(),
        };

        assert!(format_prompt_context(&app).is_none());
    }

    #[test]
    fn wraps_xml_cdata_helper() {
        assert_eq!(
            wrap_xml_cdata("sample", "<tag>&value</tag>"),
            "<sample><![CDATA[<tag>&value</tag>]]></sample>"
        );
    }

    #[test]
    fn splits_cdata_terminator_safely() {
        assert_eq!(
            wrap_xml_cdata("sample", "a]]>b"),
            "<sample><![CDATA[a]]]]><![CDATA[>b]]></sample>"
        );
    }
}
