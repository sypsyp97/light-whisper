use light_whisper_native_asr::Family;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

pub(super) const MANIFEST: &str = ".light_whisper_complete.json";
const MIN_WEIGHT: u64 = 1_000_000;

pub(super) struct Model {
    pub repo: &'static str,
    pub revision: &'static str,
    pub filename: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

pub(super) fn model(family: Family) -> Model {
    match family {
        Family::Qwen => Model {
            repo: "audio-cpp/audio.cpp-gguf",
            revision: "f861648c0c1f4da9f18e9a0ce5e217b0679bb2f5",
            filename: "Qwen3-ASR-0.6B-GGUF/qwen3-asr-0.6b-q8_0.gguf",
            size: 1_151_272_416,
            sha256: "6c44ec2fb4cee513892d7863c1fcc3ea6b699ffa4d899b0ef4ab19956d9544f7",
        },
        Family::R2t2 => Model {
            repo: "davidxifeng/Confucius4-R2T2-gguf",
            revision: "a8e6b385d7df7eae9519363e07034a209004797a",
            filename: "r2t2-q8_0.gguf",
            size: 2_477_512_064,
            sha256: "19f5ccd624484bcb5d44301437de41560b0ecc40c430e8850dfeefefbe82ccf5",
        },
    }
}

impl Model {
    pub fn repo_dir(&self) -> Result<PathBuf, String> {
        let variable = |key| {
            env::var_os(key)
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        };
        let cache = variable("HF_HUB_CACHE")
            .or_else(|| variable("HF_HOME").map(|p| p.join("hub")))
            .or_else(|| env::home_dir().map(|p| p.join(".cache/huggingface/hub")))
            .ok_or("Hugging Face cache directory unavailable")?;
        Ok(cache.join(format!("models--{}", self.repo.replace('/', "--"))))
    }

    pub fn verify(&self, path: &Path) -> io::Result<()> {
        let metadata = fs::metadata(path)?;
        if !metadata.is_file() || metadata.len() != self.size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{} 文件大小校验失败: got={}, expected={}",
                    self.filename,
                    metadata.len(),
                    self.size
                ),
            ));
        }
        let digest = sha256(path)?;
        if digest != self.sha256 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{} SHA256 校验失败: got={digest}, expected={}",
                    self.filename, self.sha256
                ),
            ));
        }
        Ok(())
    }
}

fn sha256(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buf = vec![0; 1024 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        digest.update(&buf[..n]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn manifest_ready(snapshot: &Path, manifest: &Path, filename: &str) -> Option<bool> {
    let data: Value = serde_json::from_reader(fs::File::open(manifest).ok()?).ok()?;
    let mut required = false;
    for item in data.get("files")?.as_array()? {
        let name = item.get("path")?.as_str()?;
        let size = item.get("size")?.as_u64()?;
        let relative = Path::new(name);
        if name.is_empty()
            || relative
                .components()
                .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Some(false);
        }
        let path = snapshot.join(relative);
        let metadata = fs::metadata(&path).ok()?;
        if !metadata.is_file() || metadata.len() != size {
            return Some(false);
        }
        if let Some(expected) = item.get("sha256").filter(|v| !v.is_null()) {
            let hex = expected.as_str()?;
            if hex.len() != 64
                || !hex.bytes().all(|b| b.is_ascii_hexdigit())
                || !hex.eq_ignore_ascii_case(&sha256(&path).ok()?)
            {
                return Some(false);
            }
        }
        required |= name == filename;
    }
    // The required file is a GGUF whose minimum size was checked by the caller.
    Some(required)
}

fn legacy_ready(snapshot: &Path) -> bool {
    let mut dirs = vec![snapshot.to_owned()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = dir.read_dir() else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if entry.file_type().is_ok_and(|t| !t.is_symlink()) {
                    dirs.push(path)
                }
            } else if entry.file_name().to_string_lossy().ends_with(".incomplete")
                || path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|e| matches!(e, "pt" | "bin" | "safetensors" | "onnx" | "gguf"))
                    && fs::metadata(path).is_err()
            {
                return false;
            }
        }
    }
    true
}

pub fn resolve(family: Family) -> Result<PathBuf, String> {
    let spec = model(family);
    let repo = spec.repo_dir()?;
    let snapshots = repo.join("snapshots");
    if family == Family::R2t2 {
        let path = snapshots.join(spec.revision).join(spec.filename);
        return spec
            .verify(&path)
            .map(|()| path)
            .map_err(|_| "R2T2 Q8 模型未下载或校验失败".into());
    }
    let preferred = fs::read_to_string(repo.join("refs/main")).unwrap_or_default();
    let mut dirs: Vec<_> = snapshots
        .read_dir()
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .collect();
    dirs.sort_by_key(|d| (d.file_name() != preferred.trim(), d.file_name()));
    for entry in dirs {
        let snapshot = entry.path();
        let path = snapshot.join(spec.filename);
        if !fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() >= MIN_WEIGHT) {
            continue;
        }
        let manifest = snapshot.join(MANIFEST);
        let ready = match manifest.symlink_metadata() {
            Ok(_) => manifest_ready(&snapshot, &manifest, spec.filename) == Some(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => legacy_ready(&snapshot),
            Err(_) => false,
        };
        if ready {
            return Ok(path);
        }
    }
    Err(format!("Qwen3-ASR Q8 模型未下载: {}", spec.filename))
}
