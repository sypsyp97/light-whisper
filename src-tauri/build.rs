fn main() {
    // 确保引擎归档占位文件存在，避免 dev 模式编译报错
    let engine_archive = std::path::Path::new("resources/engine.tar.xz");
    if !engine_archive.exists() {
        let _ = std::fs::File::create(engine_archive);
    }

    tauri_build::build()
}
