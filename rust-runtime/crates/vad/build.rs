use std::{env, path::PathBuf};

// LW_ORT_SDK is the extracted Microsoft.ML.OnnxRuntime 1.24.1 NuGet root.
// LW_KALDI_SDK contains include/ and lib/ from kaldi-native-fbank 1.22.3.
// Ship their native DLLs alongside the consuming executable.
fn sdk(variable: &str) -> PathBuf {
    println!("cargo:rerun-if-env-changed={variable}");
    env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("Set {variable} to the native SDK root (see vad/build.rs)"))
}

fn main() {
    let ort = sdk("LW_ORT_SDK");
    let kaldi = sdk("LW_KALDI_SDK");
    println!("cargo:rerun-if-changed=native/adapter.cpp");
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .flag_if_supported("/EHsc")
        .include(ort.join("build/native/include"))
        .include(kaldi.join("include"))
        .file("native/adapter.cpp")
        .compile("light_whisper_vad_adapter");
    println!(
        "cargo:rustc-link-search=native={}",
        ort.join("runtimes/win-x64/native").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        kaldi.join("lib").display()
    );
    println!("cargo:rustc-link-lib=dylib=onnxruntime");
    println!("cargo:rustc-link-lib=dylib=kaldi-native-fbank-core");
}
