fn main() {
    println!("cargo:rerun-if-changed=native/pocketfft.c");
    cc::Build::new()
        .file("native/pocketfft.c")
        .std("c11")
        .compile("reuse_pocketfft");
}
