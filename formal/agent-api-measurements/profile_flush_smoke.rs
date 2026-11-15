//! Manual ABI diagnostic. Export `single_hit` and require exactly one execution.

#[inline(never)]
fn single_hit() -> u32 {
    std::hint::black_box(7)
}

fn main() {
    assert_eq!(single_hit(), 7);
    #[cfg(coverage)]
    {
        unsafe extern "C" {
            fn __llvm_profile_write_file() -> std::os::raw::c_int;
        }
        // SAFETY: the instrumented binary links LLVM's profiling runtime with this C ABI.
        let status = unsafe { __llvm_profile_write_file() };
        eprintln!("LLVM_PROFILE_WRITE_STATUS={status}");
        assert_eq!(status, 0);
        if std::env::args().nth(1).as_deref() == Some("exit") {
            std::process::exit(status);
        }
    }
}
