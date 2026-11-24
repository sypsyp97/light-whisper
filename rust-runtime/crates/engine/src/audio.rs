use base64::{Engine, engine::general_purpose::STANDARD};
use libloading::Library;
use serde_json::Value;
use std::{
    ffi::c_void,
    io::{Cursor, Read, Seek, SeekFrom},
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr::NonNull,
};

type R<T> = Result<T, String>;
#[repr(C)]
#[derive(Default)]
struct Info {
    frames: i64,
    rate: i32,
    channels: i32,
    format: i32,
    sections: i32,
    seekable: i32,
}
#[repr(C)]
struct Callbacks {
    size: unsafe extern "C" fn(*mut c_void) -> i64,
    seek: unsafe extern "C" fn(i64, i32, *mut c_void) -> i64,
    read: unsafe extern "C" fn(*mut c_void, i64, *mut c_void) -> i64,
    write: unsafe extern "C" fn(*const c_void, i64, *mut c_void) -> i64,
    tell: unsafe extern "C" fn(*mut c_void) -> i64,
}
// libsndfile invokes these synchronously while the Cursor lives in decode().
type Input<'a> = Cursor<&'a [u8]>;
unsafe extern "C" fn size(p: *mut c_void) -> i64 {
    unsafe { (&*p.cast::<Input>()).get_ref().len() as i64 }
}
unsafe extern "C" fn tell(p: *mut c_void) -> i64 {
    unsafe { (&*p.cast::<Input>()).position() as i64 }
}
unsafe extern "C" fn seek(n: i64, whence: i32, p: *mut c_void) -> i64 {
    let position = match whence {
        0 if n >= 0 => SeekFrom::Start(n as u64),
        1 => SeekFrom::Current(n),
        2 => SeekFrom::End(n),
        _ => return -1,
    };
    unsafe {
        (&mut *p.cast::<Input>())
            .seek(position)
            .ok()
            .and_then(|x| i64::try_from(x).ok())
            .unwrap_or(-1)
    }
}
unsafe extern "C" fn read(out: *mut c_void, n: i64, p: *mut c_void) -> i64 {
    if out.is_null() || n <= 0 {
        return 0;
    }
    let reader = unsafe { &mut *p.cast::<Input>() };
    let available = (reader.get_ref().len() as u64).saturating_sub(reader.position());
    let len = available.min(n as u64) as usize;
    let buffer = unsafe { std::slice::from_raw_parts_mut(out.cast(), len) };
    reader.read(buffer).map(|x| x as i64).unwrap_or(0)
}
unsafe extern "C" fn write(_: *const c_void, _: i64, _: *mut c_void) -> i64 {
    0
}
static CALLBACKS: Callbacks = Callbacks {
    size,
    seek,
    read,
    write,
    tell,
};
type Close = unsafe extern "C" fn(*mut c_void) -> i32;
struct SoundFile(NonNull<c_void>, Close);
impl Drop for SoundFile {
    fn drop(&mut self) {
        unsafe {
            (self.1)(self.0.as_ptr());
        }
    }
}
enum Source<'a> {
    File(&'a Path),
    Memory(&'a [u8]),
}
fn decode(resources: &Path, source: Source) -> R<(Vec<f32>, u32)> {
    type Open = unsafe extern "C" fn(*const u16, i32, *mut Info) -> *mut c_void;
    type OpenMem =
        unsafe extern "C" fn(*const Callbacks, i32, *mut Info, *mut c_void) -> *mut c_void;
    type Frames = unsafe extern "C" fn(*mut c_void, *mut f32, i64) -> i64;
    type Error = unsafe extern "C" fn(*mut c_void) -> i32;
    // Native Windows file APIs need the extended absolute path when the
    // user's data directory and content-addressed cache exceed MAX_PATH.
    let dll = resources
        .join("_soundfile_data/libsndfile_x64.dll")
        .canonicalize()
        .map_err(|e| format!("Cannot locate the audio decoder: {e}"))?;
    // Only this packaged absolute path is used. All function pointers and handles die before the library.
    unsafe {
        let lib = Library::new(dll).map_err(|e| e.to_string())?;
        let close = *lib.get::<Close>(b"sf_close\0").map_err(|e| e.to_string())?;
        let frames = *lib
            .get::<Frames>(b"sf_readf_float\0")
            .map_err(|e| e.to_string())?;
        let error = *lib.get::<Error>(b"sf_error\0").map_err(|e| e.to_string())?;
        let mut info = Info::default();
        let mut cursor;
        let handle = match source {
            Source::File(path) => {
                let path = path
                    .canonicalize()
                    .map_err(|e| format!("Cannot open audio: {e}"))?;
                let mut wide = path.as_os_str().encode_wide().collect::<Vec<_>>();
                if wide.contains(&0) {
                    return Err("invalid audio path".into());
                }
                wide.push(0);
                lib.get::<Open>(b"sf_wchar_open\0")
                    .map_err(|e| e.to_string())?(wide.as_ptr(), 0x10, &mut info)
            }
            Source::Memory(bytes) => {
                cursor = Cursor::new(bytes);
                lib.get::<OpenMem>(b"sf_open_virtual\0")
                    .map_err(|e| e.to_string())?(
                    &CALLBACKS,
                    0x10,
                    &mut info,
                    (&mut cursor as *mut Input).cast(),
                )
            }
        };
        let file = SoundFile(NonNull::new(handle).ok_or("cannot open audio")?, close);
        let channels = usize::try_from(info.channels)
            .ok()
            .filter(|c| (1..=1024).contains(c))
            .ok_or("invalid channels")?;
        let rate = u32::try_from(info.rate)
            .ok()
            .filter(|x| *x > 0)
            .ok_or("invalid sample rate")?;
        let mut buffer = vec![0.; 4096 * channels];
        let mut pcm = Vec::new();
        loop {
            let n = frames(file.0.as_ptr(), buffer.as_mut_ptr(), 4096);
            if !(0..=4096).contains(&n) || error(file.0.as_ptr()) != 0 {
                return Err("audio decode failed".into());
            }
            if n == 0 {
                break;
            }
            pcm.extend(
                buffer[..n as usize * channels]
                    .chunks_exact(channels)
                    .map(|frame| frame.iter().sum::<f32>() / channels as f32),
            );
        }
        Ok((pcm, rate))
    }
}
pub fn pcm16(raw: &[u8]) -> R<Vec<f32>> {
    if !raw.len().is_multiple_of(2) {
        return Err("incomplete PCM16 sample".into());
    }
    Ok(raw
        .chunks_exact(2)
        .map(|x| i16::from_le_bytes([x[0], x[1]]) as f32 / 32768.)
        .collect())
}
pub fn bytes(cmd: &Value) -> R<Vec<u8>> {
    let s = cmd["audio_base64"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("missing audio")?;
    STANDARD.decode(s).map_err(|e| e.to_string())
}
pub fn memory(cmd: &Value) -> bool {
    cmd["audio_base64"].as_str().is_some_and(|s| !s.is_empty())
}
pub fn load(resources: &Path, cmd: &Value, qwen: bool) -> R<(Vec<f32>, f64)> {
    let (samples, rate) = if memory(cmd) {
        let raw = bytes(cmd)?;
        let format = cmd["audio_format"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("pcm_s16le");
        if format.eq_ignore_ascii_case("pcm_s16le") {
            let rate = cmd["sample_rate"]
                .as_f64()
                .filter(|n| n.is_finite() && *n > 0.)
                .ok_or("invalid PCM sample_rate")?;
            (pcm16(&raw)?, rate)
        } else if !qwen && format.eq_ignore_ascii_case("wav") {
            let (a, r) = decode(resources, Source::Memory(&raw))?;
            (a, r as f64)
        } else {
            return Err("unsupported inline audio format".into());
        }
    } else {
        let path = cmd["audio_path"].as_str().ok_or("missing audio path")?;
        let (a, r) = decode(resources, Source::File(Path::new(path)))?;
        (a, r as f64)
    };
    if !samples.iter().all(|x| x.is_finite()) {
        return Err("audio contains non-finite samples".into());
    }
    let duration = samples.len() as f64 / rate;
    if rate == 16000. || samples.is_empty() {
        return Ok((samples, duration));
    }
    let len = (duration * 16000.).round_ties_even() as usize;
    let mut out = Vec::new();
    out.try_reserve_exact(len)
        .map_err(|_| "resampled input exceeds available memory")?;
    let step = samples.len().saturating_sub(1) as f64 / len.saturating_sub(1).max(1) as f64;
    for i in 0..len {
        let x = if i > 0 && i + 1 == len {
            (samples.len() - 1) as f64
        } else {
            i as f64 * step
        };
        let l = x as usize;
        let a = samples[l] as f64;
        let b = samples[(l + 1).min(samples.len() - 1)] as f64;
        out.push((a + (b - a) * (x - l as f64)) as f32);
    }
    Ok((
        out,
        if qwen && !memory(cmd) {
            len as f64 / 16000.
        } else {
            duration
        },
    ))
}
