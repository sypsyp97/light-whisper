use crate::{Error, Result};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{self, BufReader, Read, Seek, SeekFrom},
    path::{Component, Path, PathBuf},
};
use xz2::read::XzDecoder;

const ENGINE_STAMP: &[u8] = b"LIGHT_WHISPER_RUST_BUILD_V1\0engine\0";
fn sha(reader: impl Read) -> io::Result<[u8; 32]> {
    let mut hash = Sha256::new();
    io::copy(
        &mut BufReader::with_capacity(1024 * 1024, reader),
        &mut hash,
    )?;
    Ok(hash.finalize().into())
}
fn stamped(path: &Path) -> io::Result<bool> {
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    let footer_len = ENGINE_STAMP.len() + 64;
    let Some(len) = f.metadata()?.len().checked_sub(footer_len as u64) else {
        return Ok(false);
    };
    let mut mz = [0; 2];
    if len < 64 {
        return Ok(false);
    }
    f.read_exact(&mut mz)?;
    if &mz != b"MZ" {
        return Ok(false);
    }
    f.seek(SeekFrom::Start(len))?;
    let mut footer = vec![0; footer_len];
    f.read_exact(&mut footer)?;
    if !footer.starts_with(ENGINE_STAMP) {
        return Ok(false);
    }
    f.rewind()?;
    Ok(sha(f.take(len))?[..] == footer[footer_len - 32..])
}
fn version(dir: &Path) -> Option<String> {
    let s = fs::read_to_string(dir.join(".version")).ok()?;
    let s = s.trim().rsplit('+').next()?.strip_prefix("sha256:")?;
    (s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())).then(|| s.to_ascii_lowercase())
}
fn regular_path(p: &Path) -> bool {
    !p.as_os_str().is_empty()
        && p.components().all(|c| {
            let Component::Normal(s) = c else {
                return false;
            };
            let Some(s) = s.to_str() else { return false };
            let base = s.split('.').next().unwrap_or("").to_ascii_uppercase();
            !s.ends_with(['.', ' '])
                && !s.contains([':', '\\', '<', '>', '"', '|', '?', '*'])
                && !s.chars().any(char::is_control)
                && !matches!(
                    base.as_str(),
                    "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
                )
                && !(base.len() == 4
                    && (base.starts_with("COM") || base.starts_with("LPT"))
                    && base.as_bytes()[3].is_ascii_digit())
        })
}
fn extract(source: File, dir: &Path) -> Result<()> {
    let mut archive = tar::Archive::new(XzDecoder::new(BufReader::new(source)));
    let mut seen = HashSet::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let rel = entry.path()?.into_owned();
        let kind = entry.header().entry_type();
        if !regular_path(&rel)
            || !seen.insert(rel.to_string_lossy().to_lowercase())
            || !(kind.is_file() || kind.is_dir())
        {
            return Err(Error::runtime("Invalid native engine archive entry"));
        }
        let dst = dir.join(rel);
        if kind.is_dir() {
            fs::create_dir_all(dst)?;
            continue;
        }
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?
        }
        let mut dest = File::options().write(true).create_new(true).open(dst)?;
        io::copy(&mut entry, &mut dest)?;
    }
    // tar ends before XZ's footer. Consume the stream to verify its checksum and length.
    io::copy(&mut archive.into_inner(), &mut io::sink())?;
    if !stamped(&dir.join("engine.exe"))? {
        return Err(Error::runtime(
            "Invalid native engine executable in archive",
        ));
    }
    Ok(())
}

// The standalone client never replaces the desktop's potentially live engine.
pub fn discover(exe: &Path, data: &Path, install: bool) -> Result<Option<PathBuf>> {
    let host = data.join("engine");
    let host_exe = host.join("engine.exe");
    let host_version = version(&host);
    let path = exe
        .parent()
        .ok_or_else(|| Error::runtime("Missing executable directory"))?
        .join("resources/engine.tar.xz");
    let mut archive = match File::open(&path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(if host_version.is_some() && stamped(&host_exe)? {
                Some(host_exe)
            } else {
                None
            });
        }
        Err(e) => return Err(e.into()),
    };
    let digest = sha(&mut archive)?
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    if host_version.as_deref() == Some(&digest) && stamped(&host_exe)? {
        return Ok(Some(host_exe));
    }
    let root = data.join("cli-engines");
    let cache = root.join(&digest);
    let engine = cache.join("engine.exe");
    if stamped(&engine)? {
        return Ok(Some(engine));
    }
    if !install {
        return Ok(None);
    }
    fs::create_dir_all(&root)?;
    let lock = File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(format!("{digest}.lock")))?;
    lock.lock()?;
    if stamped(&engine)? {
        return Ok(Some(engine));
    }
    if cache.exists() {
        return Err(Error::runtime(format!(
            "Damaged native engine cache: {}",
            cache.display()
        )));
    }
    let stage = tempfile::Builder::new()
        .prefix(".extract-")
        .tempdir_in(&root)?;
    archive.rewind()?;
    extract(archive, stage.path())?;
    fs::rename(stage.path(), cache)?;
    Ok(Some(engine))
}
