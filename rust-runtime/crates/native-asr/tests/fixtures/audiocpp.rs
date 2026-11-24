//! Test DLL for the production loader. The child process selects one native failure.
use std::ffi::{c_char, c_void, CStr};
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

type Handle = *mut c_void;
type Str = *const c_char;
static NEXT: AtomicUsize = AtomicUsize::new(1);
static PUSHES: AtomicUsize = AtomicUsize::new(0);
const BORROWED: usize = 0x10000;

fn scenario() -> String {
    std::env::var("LW_FIXTURE_CASE").unwrap()
}
fn trace(value: impl AsRef<str>) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::env::var_os("LW_FIXTURE_TRACE").unwrap())
        .unwrap();
    writeln!(file, "{}", value.as_ref()).unwrap();
}
fn allocate(kind: &str) -> Handle {
    let id = NEXT.fetch_add(1, SeqCst);
    trace(format!("alloc {kind} {id}"));
    id as Handle
}
unsafe fn string(p: Str) -> String {
    if p.is_null() {
        return "<null>".into();
    }
    CStr::from_ptr(p).to_string_lossy().into_owned()
}
fn status(op: &str) -> i32 {
    if scenario() == format!("fail_{op}") {
        19
    } else {
        0
    }
}

#[repr(C)]
pub struct ModelConfig {
    family: Str,
    config: Str,
    weight: Str,
    spec: Str,
}
#[repr(C)]
pub struct BackendConfig {
    backend: Str,
    device: i32,
    threads: i32,
}

#[no_mangle]
pub extern "C" fn audiocpp_abi_version() -> u32 {
    std::env::var("LW_FIXTURE_ABI").unwrap().parse().unwrap()
}
#[no_mangle]
pub extern "C" fn audiocpp_last_error() -> Str {
    c"fixture detail".as_ptr()
}
#[no_mangle]
pub unsafe extern "C" fn audiocpp_registry_create(_: Str, out: *mut Handle) -> i32 {
    *out = allocate("registry");
    status("registry_create")
}
#[no_mangle]
pub unsafe extern "C" fn audiocpp_model_load(
    _: Handle,
    path: Str,
    cfg: *const ModelConfig,
    _: Handle,
    out: *mut Handle,
) -> i32 {
    trace(format!("model {} {}", string((*cfg).family), string(path)));
    *out = allocate("model");
    status("model_load")
}
#[no_mangle]
pub extern "C" fn audiocpp_options_create() -> Handle {
    if scenario() == "null_options" {
        std::ptr::null_mut()
    } else {
        allocate("options")
    }
}
#[no_mangle]
pub unsafe extern "C" fn audiocpp_options_set(_: Handle, k: Str, v: Str) -> i32 {
    trace(format!("option {} {}", string(k), string(v)));
    status("options_set")
}
#[no_mangle]
pub unsafe extern "C" fn audiocpp_session_create(
    _: Handle,
    task: Str,
    mode: Str,
    backend: *const BackendConfig,
    _: Handle,
    out: *mut Handle,
) -> i32 {
    let b = &*backend;
    trace(format!(
        "session {} {} {} {} {}",
        string(task),
        string(mode),
        string(b.backend),
        b.device,
        b.threads
    ));
    *out = allocate("session");
    status("session_create")
}
#[no_mangle]
pub extern "C" fn audiocpp_request_create() -> Handle {
    if scenario() == "null_request" {
        std::ptr::null_mut()
    } else {
        allocate("request")
    }
}
#[no_mangle]
pub unsafe extern "C" fn audiocpp_request_set_text(_: Handle, context: Str, language: Str) -> i32 {
    trace(format!("text {} {}", string(context), string(language)));
    status("request_set_text")
}
#[no_mangle]
pub unsafe extern "C" fn audiocpp_request_set_option(_: Handle, k: Str, v: Str) -> i32 {
    trace(format!("request-option {} {}", string(k), string(v)));
    status("request_set_option")
}
#[no_mangle]
pub unsafe extern "C" fn audiocpp_request_set_audio(
    _: Handle,
    pcm: *const f32,
    count: usize,
    rate: i32,
    channels: i32,
) -> i32 {
    trace(format!(
        "audio {count} {rate} {channels} {} {}",
        *pcm,
        *pcm.add(count - 1)
    ));
    status("request_set_audio")
}
#[no_mangle]
pub unsafe extern "C" fn audiocpp_session_run(_: Handle, _: Handle, out: *mut Handle) -> i32 {
    *out = allocate("result");
    status("session_run")
}
#[no_mangle]
pub extern "C" fn audiocpp_stream_start(_: Handle, _: Handle) -> i32 {
    PUSHES.store(0, SeqCst);
    trace("start");
    status("stream_start")
}
#[no_mangle]
pub unsafe extern "C" fn audiocpp_stream_push(
    _: Handle,
    data: *const f32,
    count: usize,
    rate: i32,
    channels: i32,
    offset: i64,
    out: *mut Handle,
) -> i32 {
    let blank = std::slice::from_raw_parts(data, count)
        .iter()
        .all(|&v| v == 0.0);
    trace(format!("push {offset} {count} {rate} {channels} {blank}"));
    PUSHES.fetch_add(1, SeqCst);
    if scenario() != "null_event" {
        *out = allocate("event");
    }
    status("stream_push")
}
#[no_mangle]
pub unsafe extern "C" fn audiocpp_stream_finish(_: Handle, out: *mut Handle) -> i32 {
    trace("finish");
    *out = allocate("result");
    status("stream_finish")
}
#[no_mangle]
pub extern "C" fn audiocpp_stream_reset(_: Handle) -> i32 {
    trace("reset");
    status("stream_reset")
}
#[no_mangle]
pub extern "C" fn audiocpp_event_as_result(_: Handle) -> Handle {
    BORROWED as Handle
}
#[no_mangle]
pub unsafe extern "C" fn audiocpp_result_text(h: Handle, text: *mut Str, lang: *mut Str) -> i32 {
    let borrowed = h as usize == BORROWED;
    let push = PUSHES.load(SeqCst);
    if borrowed && push > 2 || scenario() == "no_result_text" {
        return 7;
    }
    *text = if !borrowed {
        c"原文"
    } else if push == 1 {
        c"A"
    } else {
        c"B"
    }
    .as_ptr();
    *lang = if !borrowed && scenario() != "qwen" || borrowed && push == 1 {
        std::ptr::null()
    } else {
        c"Chinese".as_ptr()
    };
    status("result_text")
}
#[no_mangle]
pub unsafe extern "C" fn audiocpp_result_preview_text(_: Handle, preview: *mut Str) -> i32 {
    let push = PUSHES.load(SeqCst);
    if push > 2 {
        return 7;
    }
    *preview = if scenario() == "conflict" {
        c"wrong"
    } else if push == 1 {
        c"A?"
    } else {
        c"AB?"
    }
    .as_ptr();
    status("result_preview_text")
}

macro_rules! free {
    ($name:ident, $kind:literal) => {
        #[no_mangle]
        pub extern "C" fn $name(h: Handle) {
            if !h.is_null() {
                trace(format!("free {} {}", $kind, h as usize));
            }
        }
    };
}
free!(audiocpp_registry_free, "registry");
free!(audiocpp_model_free, "model");
free!(audiocpp_options_free, "options");
free!(audiocpp_session_free, "session");
free!(audiocpp_request_free, "request");
free!(audiocpp_event_free, "event");
free!(audiocpp_result_free, "result");
