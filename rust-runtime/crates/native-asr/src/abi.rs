//! Dynamically bound subset of audio.cpp's C ABI (audiocpp.h).
use std::ffi::{CStr, c_char, c_int, c_void};
use std::path::Path;

pub(super) type H = *mut c_void;
pub(super) type S = *const c_char;
pub(super) type Free = unsafe extern "C" fn(H);
pub(super) type R<T> = Result<T, String>;

#[repr(C)]
pub(super) struct ModelConfig {
    pub family: S,
    pub config: S,
    pub weight: S,
    pub spec: S,
}

#[repr(C)]
pub(super) struct BackendConfig {
    pub backend: S,
    pub device: c_int,
    pub threads: c_int,
}

macro_rules! api {
    ($($name:ident : fn($($arg:ty),*) $(-> $ret:ty)?),+ $(,)?) => {
        pub(super) struct Api {
            $(pub $name: unsafe extern "C" fn($($arg),*) $(-> $ret)?,)+
            _library: libloading::Library,
        }
        impl Api {
            pub fn load(path: &Path) -> R<Self> {
                let path = path.canonicalize().map_err(|e| e.to_string())?;
                // SAFETY: the caller selects the trusted audio.cpp distribution. Symbols and
                // repr(C) layouts below match its public header. The library outlives them.
                unsafe {
                    #[cfg(windows)]
                    let library: libloading::Library = {
                        use libloading::os::windows::{Library, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS};
                        Library::load_with_flags(&path, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS)
                            .map_err(|e| format!("load {}: {e}", path.display()))?.into()
                    };
                    #[cfg(not(windows))]
                    let library = libloading::Library::new(&path).map_err(|e| e.to_string())?;
                    let version = library.get::<unsafe extern "C" fn() -> u32>(b"audiocpp_abi_version\0")
                        .map_err(|e| e.to_string())?();
                    if version >> 16 != 0 || (version >> 8) & 255 < 3 {
                        return Err(format!("Unsupported audio.cpp ABI {version:#x}"));
                    }
                    Ok(Self { $($name: *library.get(concat!("audiocpp_",stringify!($name),"\0").as_bytes())
                        .map_err(|e| format!("audio.cpp {}: {e}",stringify!($name)))?,)+ _library: library })
                }
            }
        }
    };
}

api! {
    abi_version: fn() -> u32,
    last_error: fn() -> S,
    registry_create: fn(S,*mut H) -> c_int,
    model_load: fn(H,S,*const ModelConfig,H,*mut H) -> c_int,
    options_create: fn() -> H,
    options_set: fn(H,S,S) -> c_int,
    session_create: fn(H,S,S,*const BackendConfig,H,*mut H) -> c_int,
    request_create: fn() -> H,
    request_set_text: fn(H,S,S) -> c_int,
    request_set_option: fn(H,S,S) -> c_int,
    request_set_audio: fn(H,*const f32,usize,c_int,c_int) -> c_int,
    session_run: fn(H,H,*mut H) -> c_int,
    stream_start: fn(H,H) -> c_int,
    stream_push: fn(H,*const f32,usize,c_int,c_int,i64,*mut H) -> c_int,
    stream_finish: fn(H,*mut H) -> c_int,
    stream_reset: fn(H) -> c_int,
    event_as_result: fn(H) -> H,
    result_text: fn(H,*mut S,*mut S) -> c_int,
    result_preview_text: fn(H,*mut S) -> c_int,
    registry_free: fn(H), model_free: fn(H), session_free: fn(H), options_free: fn(H),
    request_free: fn(H), result_free: fn(H), event_free: fn(H),
}

pub(super) struct Owned(pub H, pub Free);

impl Owned {
    pub fn empty(free: Free) -> Self {
        Self(std::ptr::null_mut(), free)
    }
    pub fn require(&self, kind: &str) -> R<H> {
        if self.0.is_null() {
            return Err(format!("audio.cpp returned a null {kind}"));
        }
        Ok(self.0)
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: each returned handle has exactly one owner and its matching free function.
            unsafe { (self.1)(self.0) };
        }
    }
}

impl Api {
    pub fn check(&self, op: &str, status: c_int) -> R<()> {
        if status == 0 {
            return Ok(());
        }
        // SAFETY: on failure the C ABI provides a borrowed, thread-local NUL-terminated error.
        let detail = unsafe {
            let p = (self.last_error)();
            (!p.is_null()).then(|| CStr::from_ptr(p).to_string_lossy().into_owned())
        };
        Err(format!(
            "{op} failed ({status}): {}",
            detail.as_deref().unwrap_or("unknown error")
        ))
    }

    pub fn text(&self, result: H) -> R<(String, Option<String>)> {
        if result.is_null() {
            return Err("audio.cpp returned a null result".into());
        }
        let (mut text, mut lang) = (std::ptr::null(), std::ptr::null());
        // SAFETY: caller retains the event/result handle while we immediately copy its strings.
        let status = unsafe { (self.result_text)(result, &mut text, &mut lang) };
        if status == 7 {
            return Ok((String::new(), None));
        }
        self.check("result_text", status)?;
        let text = unsafe { copied(text)? };
        let lang = unsafe { copied(lang)? };
        Ok((text, (!lang.is_empty()).then_some(lang)))
    }
}

/// Caller must retain the C owner and pass a borrowed NUL-terminated string or NULL.
pub(super) unsafe fn copied(p: S) -> R<String> {
    if p.is_null() {
        return Ok(String::new());
    }
    // SAFETY: inherited from this function's contract; output is detached from the C owner.
    unsafe { CStr::from_ptr(p) }
        .to_str()
        .map(str::to_owned)
        .map_err(|e| e.to_string())
}
