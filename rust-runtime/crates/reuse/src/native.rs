use libloading::os::windows::{
    LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, Library,
};
use light_whisper_native_asr::Backend;
use std::sync::OnceLock;
use std::{
    ffi::{CStr, CString, c_char, c_void},
    path::Path,
    ptr::NonNull,
};

// VCOMP keeps an OpenMP worker parked after inference. Retain its module
// until process exit, while every model and its own DLL are released normally.
static OPENMP_RUNTIME: OnceLock<Library> = OnceLock::new();
type Run = unsafe extern "C" fn(
    *mut c_void,
    *const f32,
    *const f32,
    i32,
    *mut f32,
    *mut f32,
    *mut f32,
) -> i32;
type Reset = unsafe extern "C" fn(*mut c_void) -> i32;
type Free = unsafe extern "C" fn(*mut c_void);
type Error = unsafe extern "C" fn() -> *const c_char;
type Create = unsafe extern "C" fn(*const c_char, *const c_char, i32, i32, i32) -> *mut c_void;
pub struct Native {
    ptr: NonNull<c_void>,
    run: Run,
    reset: Reset,
    free: Free,
    error: Error,
    _lib: Library,
}
fn message(error: Error) -> String {
    let p = unsafe { error() };
    if p.is_null() {
        "unknown RE-USE error".into()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}
impl Native {
    pub fn open(resources: &Path, model: &Path, b: Backend, threads: i32) -> Result<Self, String> {
        let backend = match b {
            Backend::Cuda => "cuda",
            Backend::Vulkan => "vulkan",
            Backend::Cpu => "cpu",
        };
        let dll = resources
            .join("r2t2-native")
            .join(backend)
            .join("reuse.dll")
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let model =
            CString::new(model.to_str().ok_or("invalid model path")?).map_err(|e| e.to_string())?;
        let backend = CString::new(backend).unwrap();
        // The library owns the error string and the opaque session. Resolve all symbols before allocating the session.
        unsafe {
            let lib = Library::load_with_flags(
                dll,
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            )
            .map_err(|e| e.to_string())?;
            if OPENMP_RUNTIME.get().is_none()
                && let Ok(runtime) = Library::open_already_loaded("vcomp140.dll")
            {
                let _ = OPENMP_RUNTIME.set(runtime);
            }
            let create = *lib
                .get::<Create>(b"reuse_rt_create\0")
                .map_err(|e| e.to_string())?;
            let run = *lib
                .get::<Run>(b"reuse_rt_run\0")
                .map_err(|e| e.to_string())?;
            let reset = *lib
                .get::<Reset>(b"reuse_rt_reset\0")
                .map_err(|e| e.to_string())?;
            let free = *lib
                .get::<Free>(b"reuse_rt_free\0")
                .map_err(|e| e.to_string())?;
            let error = *lib
                .get::<Error>(b"reuse_rt_last_error\0")
                .map_err(|e| e.to_string())?;
            let ptr = NonNull::new(create(model.as_ptr(), backend.as_ptr(), threads, 8, 1))
                .ok_or_else(|| message(error))?;
            Ok(Self {
                ptr,
                run,
                reset,
                free,
                error,
                _lib: lib,
            })
        }
    }
    pub fn run(&mut self, mag: &[f32], phase: &[f32]) -> Result<[[f32; 321]; 3], String> {
        if mag.len() != phase.len() || ![321, 642].contains(&mag.len()) {
            return Err("invalid RE-USE spectrum size".into());
        }
        let mut out = [[0.; 321]; 3];
        let [m, r, i] = &mut out;
        let status = unsafe {
            (self.run)(
                self.ptr.as_ptr(),
                mag.as_ptr(),
                phase.as_ptr(),
                (mag.len() / 321) as i32,
                m.as_mut_ptr(),
                r.as_mut_ptr(),
                i.as_mut_ptr(),
            )
        };
        if status != 0 {
            return Err(message(self.error));
        }
        if out.iter().flatten().any(|x| !x.is_finite()) {
            return Err("nonfinite RE-USE output".into());
        }
        Ok(out)
    }
    pub fn reset(&mut self) -> Result<(), String> {
        if unsafe { (self.reset)(self.ptr.as_ptr()) } == 0 {
            Ok(())
        } else {
            Err(message(self.error))
        }
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        unsafe {
            (self.free)(self.ptr.as_ptr());
        }
    }
}
