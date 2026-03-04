pub struct ForegroundApp {
    pub window_title: String,
    pub process_name: String,
}

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
unsafe fn get_window_title(hwnd: *mut core::ffi::c_void) -> String {
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
unsafe fn get_process_name(hwnd: *mut core::ffi::c_void) -> String {
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
