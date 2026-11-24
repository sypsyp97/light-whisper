use crate::{Error, Result};
use serde_json::{Value, json};
use std::{env, path::PathBuf, time::Duration};

pub const LOCAL: [&str; 2] = ["qwen3-asr-0.6b", "confucius4-r2t2"];
pub const HELP: &str = "\
Usage: light-whisper [OPTIONS] COMMAND

Commands:
  doctor | status | stats | models | devices
  engine get | set ENGINE | start | restart
  transcribe AUDIO [--language LANG] [--context TEXT] [--hot-word WORD]...
  session               Read UTF-8 JSON requests, one per line

Options (before COMMAND):
  --target app|engine    Connect to the desktop (default) or start a local engine
  --app-exe PATH         Desktop executable supporting --agent-client
  --engine ENGINE        qwen3-asr-0.6b or confucius4-r2t2
  --engine-exe PATH      Local engine executable
  --threads N            Independent engine thread budget (default 2)
  --data-dir PATH        Keep independent engine data in this directory
  --timeout SECONDS      Positive response deadline (default 120)
  -h, --help             Display help";

#[derive(Default)]
pub struct Args {
    pub desktop: bool,
    pub app: Option<PathBuf>,
    pub engine_exe: Option<PathBuf>,
    pub engine: Option<String>,
    pub data: Option<PathBuf>,
    pub threads: i32,
    pub timeout: Duration,
    pub request: Value,
}

fn option_value<'a>(
    arg: &'a str,
    input: &mut impl Iterator<Item = String>,
) -> Result<(&'a str, String)> {
    if let Some((name, value)) = arg.split_once('=') {
        return Ok((name, value.into()));
    }
    let value = input
        .next()
        .filter(|s| !s.starts_with("--"))
        .ok_or_else(|| Error::input(format!("Missing value for {arg}")))?;
    Ok((arg, value))
}

pub fn parse() -> Result<Option<Args>> {
    let mut input = env::args_os()
        .skip(1)
        .map(|s| {
            s.into_string()
                .map_err(|_| Error::input("Invalid Unicode in arguments"))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter();
    let mut args = Args {
        threads: 2,
        timeout: Duration::from_secs(120),
        ..Default::default()
    };
    let mut target = None;
    let mut command = loop {
        let arg = input
            .next()
            .ok_or_else(|| Error::input("Expected a command; see --help"))?;
        if matches!(arg.as_str(), "-h" | "--help") {
            return Ok(None);
        }
        if !arg.starts_with('-') {
            break arg;
        }
        if arg == "--python" || arg.starts_with("--python=") {
            return Err(Error::input(
                "The native CLI requires --engine-exe for an explicit engine runtime",
            ));
        }
        let (name, value) = option_value(&arg, &mut input)?;
        match name {
            "--app-exe" => args.app = Some(value.into()),
            "--engine-exe" => args.engine_exe = Some(value.into()),
            "--data-dir" => args.data = Some(value.into()),
            "--engine" if LOCAL.contains(&value.as_str()) => args.engine = Some(value),
            "--target" if matches!(value.as_str(), "app" | "engine") => target = Some(value),
            "--timeout" => {
                args.timeout = value
                    .parse::<f64>()
                    .ok()
                    .filter(|n| n.is_finite() && *n > 0.0)
                    .and_then(|n| Duration::try_from_secs_f64(n).ok())
                    .ok_or_else(|| {
                        Error::input("timeout must be a positive finite number within timer range")
                    })?;
            }
            "--threads" => {
                args.threads = value.parse().ok().filter(|n| *n > 0).ok_or_else(|| {
                    Error::input("threads must be an integer from 1 to 2147483647")
                })?
            }
            _ => return Err(Error::input(format!("Invalid option or value: {arg}"))),
        }
    };
    let independent = args.engine.is_some() || args.engine_exe.is_some() || args.data.is_some();
    if (independent && (args.app.is_some() || target.as_deref() == Some("app")))
        || (args.app.is_some() && target.as_deref() == Some("engine"))
    {
        return Err(Error::input(
            "Desktop and independent-engine options cannot be combined",
        ));
    }
    args.desktop = target.as_deref() != Some("engine") && !independent;
    let mut fields = json!({});
    if command == "transcribe" {
        let mut positional = false;
        while let Some(arg) = input.next() {
            if !positional && matches!(arg.as_str(), "-h" | "--help") {
                return Ok(None);
            }
            if !positional && arg == "--" {
                positional = true;
                continue;
            }
            if !positional && arg.starts_with('-') {
                let (name, value) = option_value(&arg, &mut input)?;
                match name {
                    "--context" | "--language" => fields[&name[2..]] = value.into(),
                    "--hot-word" => {
                        fields
                            .as_object_mut()
                            .unwrap()
                            .entry("hot_words")
                            .or_insert_with(|| json!([]))
                            .as_array_mut()
                            .unwrap()
                            .push(value.into());
                    }
                    _ => return Err(Error::input(format!("Unknown transcribe option {name}"))),
                }
            } else if fields.get("audio").is_none() {
                fields["audio"] = arg.into();
            } else {
                return Err(Error::input("Expected a single audio path"));
            }
        }
    } else {
        let rest = input.collect::<Vec<_>>();
        if rest.iter().any(|s| matches!(s.as_str(), "-h" | "--help")) {
            return Ok(None);
        }
        if command == "engine" {
            match rest.as_slice() {
                [sub] if matches!(sub.as_str(), "get" | "start" | "restart") => {
                    command = format!("engine_{sub}")
                }
                [sub, value] if sub == "set" => {
                    command = "engine_set".into();
                    fields["engine"] = value.clone().into();
                }
                _ => {
                    return Err(Error::input(
                        "Expected engine get, set ENGINE, start, or restart",
                    ));
                }
            }
        } else if !rest.is_empty()
            || !matches!(
                command.as_str(),
                "doctor" | "status" | "stats" | "models" | "devices" | "session"
            )
        {
            return Err(Error::input(format!(
                "Invalid command or arguments: {command}"
            )));
        }
    }
    fields["action"] = command.into();
    args.request = fields;
    Ok(Some(args))
}
