use std::{ffi::c_void, ptr::NonNull};

unsafe extern "C" {
    fn lw_reuse_fft_create() -> *mut c_void;
    fn lw_reuse_fft_free(plan: *mut c_void);
    fn lw_reuse_rfft(plan: *mut c_void, data: *mut f64) -> i32;
    fn lw_reuse_irfft(plan: *mut c_void, data: *mut f64) -> i32;
}

pub(super) struct RealFft(NonNull<c_void>);

impl RealFft {
    pub fn new() -> Result<Self, String> {
        NonNull::new(unsafe { lw_reuse_fft_create() })
            .map(Self)
            .ok_or_else(|| "unable to allocate RE-USE FFT plan".into())
    }
    pub fn forward(&self, data: &mut [f64; super::FFT]) -> Result<(), String> {
        if unsafe { lw_reuse_rfft(self.0.as_ptr(), data.as_mut_ptr()) } == 0 {
            Ok(())
        } else {
            Err("RE-USE forward FFT failed".into())
        }
    }
    pub fn inverse(&self, data: &mut [f64; super::FFT]) -> Result<(), String> {
        if unsafe { lw_reuse_irfft(self.0.as_ptr(), data.as_mut_ptr()) } == 0 {
            Ok(())
        } else {
            Err("RE-USE inverse FFT failed".into())
        }
    }
}

impl Drop for RealFft {
    fn drop(&mut self) {
        unsafe { lw_reuse_fft_free(self.0.as_ptr()) };
    }
}
