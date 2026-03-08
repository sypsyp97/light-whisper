fn main() {
    // 确保 engine.zip 占位文件存在，避免 dev 模式编译报错
    let engine_zip = std::path::Path::new("resources/engine.zip");
    if !engine_zip.exists() {
        let _ = std::fs::File::create(engine_zip);
    }

    tauri_build::build()
}
