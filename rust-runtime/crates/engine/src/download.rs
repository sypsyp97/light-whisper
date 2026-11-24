use super::models::{self, MANIFEST, Model};
use light_whisper_native_asr::Family;
use reqwest::{
    blocking::Client,
    header::{ACCEPT_ENCODING, CONTENT_RANGE, RANGE},
};
use serde_json::{Value, json};
use std::{
    env,
    error::Error,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
    time::Duration,
};
use tempfile::NamedTempFile;

type DownloadResult<T> = Result<T, Box<dyn Error>>;

fn progress(stage: &str, percent: u64, message: String, error: Option<&str>) {
    let mut row = json!({"stage":stage, "model":"asr", "progress":percent, "overall_progress":percent,
        "completed": u8::from(stage != "downloading"), "total":1, "message":message});
    if let Some(error) = error {
        row["error"] = json!(error);
    }
    println!("{row}");
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut temp = NamedTempFile::new_in(path.parent().expect("snapshot or refs directory"))?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path)?;
    Ok(())
}

fn quarantine(path: &Path) -> io::Result<()> {
    let parent = path.parent().expect("snapshot directory");
    let prefix = format!("{}.invalid-", path.file_name().unwrap().to_string_lossy());
    let temp = tempfile::Builder::new()
        .prefix(&prefix)
        .tempfile_in(parent)?;
    let (_, destination) = temp.keep()?;
    fs::rename(path, destination)
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

fn endpoints() -> Vec<String> {
    let main = env::var("HF_ENDPOINT");
    let fallback =
        env::var("HF_FALLBACK_ENDPOINT").unwrap_or_else(|_| "https://hf-mirror.com".into());
    let fallback = fallback.trim_end_matches('/');
    let explicit = main.is_ok();
    let primary = main
        .unwrap_or_else(|_| "https://huggingface.co".into())
        .trim_end_matches('/')
        .to_owned();
    let mut list = vec![primary];
    if !explicit && !fallback.is_empty() && fallback != list[0] {
        list.push(fallback.into());
    }
    list
}

fn range_end(header: &str, offset: u64, size: u64) -> Option<u64> {
    let (span, total) = header.trim().strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = span.split_once('-')?;
    let number = |s: &str| {
        s.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| s.parse::<u64>().ok())
            .flatten()
    };
    let end = number(end)?;
    (number(start)? == offset
        && end >= offset
        && end < size
        && (total == "*" || number(total) == Some(size)))
    .then_some(end)
}

fn transfer(client: &Client, url: &str, spec: &Model, partial: &Path) -> DownloadResult<()> {
    let mut offset = match partial.symlink_metadata() {
        Ok(info) if !info.is_file() => return Err("download partial must be a regular file".into()),
        Ok(info) if info.len() > spec.size => {
            fs::remove_file(partial)?;
            0
        }
        Ok(info) => info.len(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => 0,
        Err(e) => return Err(e.into()),
    };
    for attempt in 0..2 {
        let mut request = client.get(url).header(ACCEPT_ENCODING, "identity");
        if offset != 0 {
            request = request.header(RANGE, format!("bytes={offset}-"));
        }
        let mut response = request.send()?;
        let status = response.status();
        let range = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .trim();
        if status.as_u16() == 416 {
            let remote_size = range
                .strip_prefix("bytes */")
                .and_then(|n| n.parse::<u64>().ok());
            if offset == spec.size && remote_size.is_none_or(|v| v == spec.size) {
                return Ok(());
            }
            remove_if_present(partial)?;
            offset = 0;
            if attempt == 0 {
                continue;
            }
            return Err(format!("{} 服务器拒绝完整下载请求", spec.filename).into());
        }
        response.error_for_status_ref()?;
        let end = match status.as_u16() {
            200 => {
                offset = 0;
                None
            }
            206 => match range_end(range, offset, spec.size) {
                Some(n) => Some(n),
                None => {
                    remove_if_present(partial)?;
                    offset = 0;
                    if attempt == 0 {
                        continue;
                    }
                    return Err(
                        format!("{} 服务器返回了无效的 Content-Range", spec.filename).into(),
                    );
                }
            },
            _ => return Err(format!("{} 下载返回异常状态码: {status}", spec.filename).into()),
        };
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(offset > 0)
            .truncate(offset == 0)
            .open(partial)?;
        let mut current = offset;
        let mut last_percent = None;
        let mut buf = vec![0; 1024 * 1024];
        loop {
            let n = response.read(&mut buf)?;
            if n == 0 {
                break;
            }
            current += n as u64;
            if current > spec.size {
                drop(file);
                remove_if_present(partial)?;
                return Err(format!("{} 下载超出预期大小 {}", spec.filename, spec.size).into());
            }
            file.write_all(&buf[..n])?;
            let percent = current * 100 / spec.size;
            if last_percent != Some(percent) {
                progress(
                    "downloading",
                    percent,
                    format!("[1/1] {} {percent}%", spec.filename),
                    None,
                );
                last_percent = Some(percent);
            }
        }
        file.sync_all()?;
        drop(file);
        if end.is_some_and(|n| n + 1 != current) {
            remove_if_present(partial)?;
            offset = 0;
            if attempt == 0 {
                continue;
            }
            return Err(format!("{} Content-Range 与响应长度不一致", spec.filename).into());
        }
        if current != spec.size {
            return Err(format!(
                "{} 下载不完整: got={current}, expected={}",
                spec.filename, spec.size
            )
            .into());
        }
        return Ok(());
    }
    unreachable!("range recovery is attempted only once")
}

fn download(spec: &Model) -> DownloadResult<Option<String>> {
    let repo = spec.repo_dir()?;
    let snapshot = repo.join("snapshots").join(spec.revision);
    let target = snapshot.join(spec.filename);
    let partial = snapshot.join(format!("{}.incomplete", spec.filename));
    let refs = repo.join("refs/main");
    fs::create_dir_all(target.parent().unwrap())?;
    fs::create_dir_all(refs.parent().unwrap())?;
    // Both the incomplete file and the manifest have one writer per repository.
    let lock = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(repo.join(".light_whisper_download.lock"))?;
    lock.lock()?;
    let manifest = json!({"repo_id":spec.repo,"commit_hash":spec.revision,
        "files":[{"path":spec.filename,"size":spec.size,"sha256":spec.sha256}]});
    let publish_metadata = || -> io::Result<()> {
        atomic_write(&snapshot.join(MANIFEST), manifest.to_string().as_bytes())?;
        atomic_write(&refs, spec.revision.as_bytes())
    };
    if target.is_file() {
        match spec.verify(&target) {
            Ok(()) => {
                publish_metadata()?;
                return Ok(None);
            }
            Err(e) if e.kind() == io::ErrorKind::InvalidData => quarantine(&target)?,
            Err(e) => return Err(e.into()),
        }
    }
    atomic_write(&refs, spec.revision.as_bytes())?;
    let client = Client::builder().timeout(Duration::from_secs(60)).build()?;
    let mut last = None;
    for (i, endpoint) in endpoints().into_iter().enumerate() {
        let message = if i == 0 {
            format!("正在获取 {} 文件列表...", spec.repo)
        } else {
            format!("主站不可用，正在切换镜像 {endpoint} ...")
        };
        progress("downloading", 0, message, None);
        let url = format!(
            "{endpoint}/{}/resolve/{}/{}",
            spec.repo, spec.revision, spec.filename
        );
        let attempt = transfer(&client, &url, spec, &partial).and_then(|()| {
            if let Err(e) = spec.verify(&partial) {
                if e.kind() == io::ErrorKind::InvalidData {
                    quarantine(&partial)?;
                }
                return Err(e.into());
            }
            fs::rename(&partial, &target)?;
            Ok(())
        });
        match attempt {
            Ok(()) => {
                publish_metadata()?;
                return Ok(Some(endpoint));
            }
            Err(e) => last = Some(e),
        }
    }
    Err(last.expect("at least one endpoint"))
}

pub fn run(family: Family) -> Result<(), String> {
    let spec = models::model(family);
    match download(&spec) {
        Ok(endpoint) => {
            let mut result = json!({"success":true,"model":"asr"});
            let message = if let Some(url) = endpoint {
                result["endpoint"] = Value::String(url);
                format!("{} 下载完成", spec.repo)
            } else {
                format!("{} 已缓存，跳过下载", spec.repo)
            };
            progress("completed", 100, message, None);
            println!(
                "{}",
                json!({"success":true,"message":"所有模型下载完成","results":{"asr":result}})
            );
            Ok(())
        }
        Err(e) => {
            let error = e.to_string();
            progress(
                "error",
                0,
                format!("{} 下载失败: {error}", spec.repo),
                Some(&error),
            );
            println!(
                "{}",
                json!({"success":false,"error":"以下模型下载失败: asr","failed_models":["asr"],
                "results":{"asr":{"success":false,"model":"asr","error":error}}})
            );
            Err(error)
        }
    }
}
