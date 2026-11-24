use crate::{
    Error, Result,
    args::{Args, LOCAL},
};
use serde_json::{Value, json};
use std::{
    env, fs,
    io::{self, Read},
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use tokio::process::Command;

fn home() -> Result<PathBuf> {
    env::home_dir().ok_or_else(|| Error::input("Unable to locate the user home directory"))
}

pub fn absolute(path: &Path) -> Result<PathBuf> {
    let path = match path.strip_prefix("~") {
        Ok(rest) => home()?.join(rest),
        Err(_) => path.to_owned(),
    };
    Ok(match dunce::canonicalize(&path) {
        Ok(path) => path,
        Err(e) if e.kind() == io::ErrorKind::NotFound => std::path::absolute(path)?,
        Err(e) => return Err(e.into()),
    })
}

pub struct Config {
    dir: PathBuf,
    engine: String,
    source: &'static str,
    models: PathBuf,
    model_source: &'static str,
    app: Option<PathBuf>,
    engine_exe: PathBuf,
    exe: PathBuf,
}

fn supports_protocol(path: Option<&Path>) -> Result<bool> {
    const MARKER: &[u8] = b"LIGHT_WHISPER_AGENT_PROTOCOL_1";
    let Some(path) = path.filter(|p| p.is_file()) else {
        return Ok(false);
    };
    let mut file = fs::File::open(path)?;
    let mut buffer = vec![0; 65536 + MARKER.len() - 1];
    let mut carry = 0;
    loop {
        let n = file.read(&mut buffer[carry..])?;
        if n == 0 {
            return Ok(false);
        }
        let end = carry + n;
        if memchr::memmem::find(&buffer[..end], MARKER).is_some() {
            return Ok(true);
        }
        carry = (MARKER.len() - 1).min(end);
        buffer.copy_within(end - carry..end, 0);
    }
}

fn app_path(exe: &Path, explicit: Option<&Path>) -> Result<Option<PathBuf>> {
    if let Some(path) = explicit {
        return Ok(Some(absolute(path)?));
    }
    let mut candidates: Vec<_> = exe
        .ancestors()
        .skip(1)
        .take(2)
        .map(|p| p.join("light-whisper.exe"))
        .collect();
    if let Some(repo) = exe
        .ancestors()
        .find(|p| p.join("src-tauri/Cargo.toml").is_file() && p.join("package.json").is_file())
    {
        for build in ["release", "debug"] {
            candidates.push(
                repo.join("src-tauri/target")
                    .join(build)
                    .join("light-whisper.exe"),
            );
        }
    }
    if let Some(dir) = env::var_os("LOCALAPPDATA").filter(|p| !p.is_empty()) {
        candidates.push(PathBuf::from(dir).join("轻语 Whisper/light-whisper.exe"));
    }
    Ok(candidates
        .into_iter()
        .filter(|p| p.is_file())
        .filter_map(|p| dunce::canonicalize(p).ok())
        .find(|p| p != exe))
}

impl Config {
    pub fn load(args: &Args) -> Result<Self> {
        let dir = if let Some(path) = env::var_os("LIGHT_WHISPER_DATA_DIR") {
            PathBuf::from(path)
        } else {
            match env::var_os("APPDATA") {
                Some(p) => PathBuf::from(p),
                None => home()?.join(".local/share"),
            }
            .join("com.light-whisper.app")
        };
        let dir = absolute(&dir)?;
        let path = dir.join("engine.json");
        let config: Value = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
                Error::input(format!(
                    "Invalid engine configuration {}: {e}",
                    path.display()
                ))
            })?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => json!({}),
            Err(e) => return Err(e.into()),
        };
        if !config.is_object() {
            return Err(Error::input(format!(
                "Engine configuration must be a JSON object: {}",
                path.display()
            )));
        }
        let (engine, source) = match (&args.engine, config.get("engine")) {
            (Some(engine), _) => (engine.as_str(), "argument"),
            (None, None) => (LOCAL[0], "default"),
            (None, Some(value)) => {
                let engine = match value.as_str() {
                    Some("qwen3-asr-1.7b") => LOCAL[1],
                    Some(s) if LOCAL.contains(&s) || matches!(s, "glm-asr" | "alibaba-asr") => s,
                    _ => {
                        return Err(Error::input(format!(
                            "Invalid saved engine in {}",
                            path.display()
                        )));
                    }
                };
                (engine, "desktop_config")
            }
        };
        let (models, model_source) =
            if let Some(s) = config["models_dir"].as_str().filter(|s| !s.is_empty()) {
                (PathBuf::from(s), "desktop_config")
            } else if let Some(path) = env::var_os("HF_HOME") {
                (PathBuf::from(path).join("hub"), "HF_HOME")
            } else {
                (home()?.join(".cache/huggingface/hub"), "default")
            };
        let exe = dunce::canonicalize(env::current_exe()?)?;
        let app = app_path(&exe, args.app.as_deref())?;
        let engine_exe = absolute(
            &args
                .engine_exe
                .clone()
                .unwrap_or_else(|| dir.join("engine/engine.exe")),
        )?;
        Ok(Self {
            dir,
            engine: engine.into(),
            source,
            models: absolute(&models)?,
            model_source,
            app,
            engine_exe,
            exe,
        })
    }

    pub fn doctor(&self, args: &Args) -> Result<Value> {
        let package: Value = serde_json::from_str(include_str!("../../../../package.json"))
            .map_err(io::Error::other)?;
        let version = package["version"]
            .as_str()
            .ok_or_else(|| Error::runtime("Missing application version in package.json"))?;
        let native = if !args.desktop && args.engine_exe.is_none() {
            crate::standalone::discover(&self.exe, &self.dir, false)?
        } else {
            None
        };
        let runtime = if args.desktop {
            self.app.as_deref()
        } else if args.engine_exe.is_some() {
            Some(self.engine_exe.as_path())
        } else {
            native.as_deref()
        };
        Ok(
            json!({"success":true,"version":version,"engine":self.engine,"engine_source":self.source,
            "config_dir":self.dir,"local_engine_supported":LOCAL.contains(&self.engine.as_str()),"target":if args.desktop {"app"} else {"engine"},
            "surface":if args.desktop {"running desktop instance"} else {"separate local engine process; not the running desktop instance"},
            "cli_runtime":"rust","repository":null,"resource_root":self.exe.parent(),"frozen":false,"engine_script":null,"default_python":null,
            "engine_script_present":false,"app_protocol_supported":self.app.as_ref()!=Some(&self.exe) && supports_protocol(self.app.as_deref())?,
            "models_dir":self.models,"models_dir_source":self.model_source,"runtime":runtime,
            "runtime_present":runtime.is_some_and(Path::is_file),"model_load_checked":false}),
        )
    }

    pub fn command(&self, args: &Args) -> Result<(Command, Option<TempDir>)> {
        if args.desktop {
            let app =
                self.app.as_ref().filter(|p| p.is_file()).ok_or_else(|| {
                    Error::input("Desktop executable not found; specify --app-exe")
                })?;
            if app == &self.exe || !supports_protocol(Some(app))? {
                return Err(Error::input(
                    "Desktop CLI protocol is unavailable; upgrade Light Whisper or specify a compatible --app-exe",
                ));
            }
            let mut command = Command::new(app);
            command
                .arg("--agent-client")
                .env("LIGHT_WHISPER_DATA_DIR", &self.dir);
            return Ok((command, None));
        }
        if !LOCAL.contains(&self.engine.as_str()) {
            return Err(Error::input(
                "The saved engine is a cloud service; select a local --engine",
            ));
        }
        let engine = if args.engine_exe.is_some() {
            self.engine_exe.clone()
        } else {
            crate::standalone::discover(&self.exe, &self.dir, true)?.ok_or_else(|| {
                Error::input(
                    "Native engine not found. Use the installed CLI or specify --engine-exe.",
                )
            })?
        };
        if !engine.is_file() {
            return Err(Error::input(format!(
                "Runtime file does not exist: {}",
                engine.display()
            )));
        }
        let mut command = Command::new(engine);
        command
            .args(["serve", "--engine", &self.engine])
            .env("HF_HUB_OFFLINE", "1")
            .env("TRANSFORMERS_OFFLINE", "1")
            .env("HF_HUB_CACHE", &self.models);
        for name in [
            "LIGHT_WHISPER_NATIVE_THREADS",
            "OMP_NUM_THREADS",
            "MKL_NUM_THREADS",
            "OPENBLAS_NUM_THREADS",
        ] {
            command.env(name, args.threads.to_string());
        }
        let temp = if let Some(path) = &args.data {
            let path = absolute(path)?;
            fs::create_dir_all(&path)?;
            command.env("LIGHT_WHISPER_DATA_DIR", path);
            None
        } else {
            let temp = tempfile::Builder::new()
                .prefix("light-whisper-cli-")
                .tempdir()?;
            command.env("LIGHT_WHISPER_DATA_DIR", temp.path());
            Some(temp)
        };
        Ok((command, temp))
    }
}
