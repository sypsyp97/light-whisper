use std::{
    ffi::{CStr, c_char, c_void},
    ptr::{self, NonNull},
    slice,
};

type Status = *const c_char;
unsafe extern "C" {
    fn lw_vad_open(bytes: *const u8, len: usize, out: *mut *mut c_void) -> Status;
    fn lw_vad_close(model: *mut c_void);
    fn lw_vad_run(
        model: *mut c_void,
        features: *mut f32,
        frames: usize,
        receive: unsafe extern "C" fn(*mut c_void, *const f32, usize),
        dest: *mut c_void,
    ) -> Status;
    fn lw_fbank_open(out: *mut *mut c_void) -> Status;
    fn lw_fbank_close(bank: *mut c_void);
    fn lw_fbank_append(
        bank: *mut c_void,
        pcm: *const f32,
        len: i32,
        dest: *mut f32,
        new_frames: usize,
    ) -> Status;
}

fn check(status: Status) -> Result<(), String> {
    if status.is_null() {
        Ok(())
    } else {
        // The adapter owns this NUL-terminated error until the next call on this thread.
        Err(unsafe { CStr::from_ptr(status) }
            .to_string_lossy()
            .into_owned())
    }
}

pub(super) struct Model(NonNull<c_void>);
// These handles own ordinary native heap objects. Moving one across threads
// transfers its sole owner. Operations (including destruction) are exclusive.
unsafe impl Send for Model {}
impl Model {
    pub fn new(bytes: &[u8]) -> Result<Self, String> {
        let mut raw = ptr::null_mut();
        check(unsafe { lw_vad_open(bytes.as_ptr(), bytes.len(), &mut raw) })?;
        Ok(Self(
            NonNull::new(raw).expect("successful native model allocation"),
        ))
    }

    pub fn run(&mut self, features: &mut [f32]) -> Result<Vec<f32>, String> {
        unsafe extern "C" fn receive(dest: *mut c_void, values: *const f32, len: usize) {
            if len != 0 {
                // ORT owns this output for the duration of the synchronous callback.
                unsafe {
                    (&mut *dest.cast::<Vec<f32>>())
                        .extend_from_slice(slice::from_raw_parts(values, len))
                };
            }
        }
        let mut output = Vec::new();
        check(unsafe {
            lw_vad_run(
                self.0.as_ptr(),
                features.as_mut_ptr(),
                features.len() / super::BINS,
                receive,
                (&raw mut output).cast(),
            )
        })?;
        Ok(output)
    }
}
impl Drop for Model {
    fn drop(&mut self) {
        unsafe { lw_vad_close(self.0.as_ptr()) }
    }
}

pub(super) struct Fbank(NonNull<c_void>);
// The same exclusive ownership contract as Model.
unsafe impl Send for Fbank {}
impl Fbank {
    pub fn new() -> Result<Self, String> {
        let mut raw = ptr::null_mut();
        check(unsafe { lw_fbank_open(&mut raw) })?;
        Ok(Self(
            NonNull::new(raw).expect("successful native filterbank allocation"),
        ))
    }
    pub fn append(&mut self, pcm: &[f32], frames: &mut [f32]) -> Result<(), String> {
        let len = i32::try_from(pcm.len()).map_err(|_| "waveform exceeds Kaldi's sample limit")?;
        check(unsafe {
            lw_fbank_append(
                self.0.as_ptr(),
                pcm.as_ptr(),
                len,
                frames.as_mut_ptr(),
                frames.len() / super::BINS,
            )
        })
    }
}
impl Drop for Fbank {
    fn drop(&mut self) {
        unsafe { lw_fbank_close(self.0.as_ptr()) }
    }
}
