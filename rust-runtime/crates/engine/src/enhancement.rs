use crate::models::Model;
use light_whisper_native_asr::Backend;
use light_whisper_reuse::{MODEL_BYTES, MODEL_SHA, RealtimeEnhancer};
use reqwest::blocking::Client;
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    time::Duration,
};
const MODEL: Model = Model {
    repo: "nvidia/Real-time_RE-USE",
    revision: "4c0c186272ca853cd18d832fc5c28777b62f32d1",
    filename: "model.safetensors",
    size: MODEL_BYTES,
    sha256: MODEL_SHA,
};
fn ensure_model() -> Result<PathBuf, String> {
    let dir = MODEL.repo_dir()?.join("snapshots").join(MODEL.revision);
    let path = dir.join(MODEL.filename);
    if MODEL.verify(&path).is_ok() {
        return Ok(path);
    }
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut temp = tempfile::NamedTempFile::new_in(&dir).map_err(|e| e.to_string())?;
    let url = format!(
        "https://huggingface.co/{}/resolve/{}/{}",
        MODEL.repo, MODEL.revision, MODEL.filename
    );
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?;
    let size = io::copy(&mut response.take(MODEL.size + 1), temp.as_file_mut())
        .map_err(|e| e.to_string())?;
    if size != MODEL.size {
        return Err("RE-USE 下载大小与固定版本不符".into());
    }
    MODEL.verify(temp.path()).map_err(|e| e.to_string())?;
    temp.persist(&path).map_err(|e| e.error.to_string())?;
    Ok(path)
}
pub fn open(resources: &Path) -> Result<RealtimeEnhancer, String> {
    let path = ensure_model()?;
    let mut errors = vec![];
    for backend in [Backend::Cuda, Backend::Cpu] {
        match RealtimeEnhancer::open(resources, &path, backend, 2) {
            Ok(enhancer) => return Ok(enhancer),
            Err(e) => errors.push(e),
        }
    }
    Err(errors.join(", "))
}
