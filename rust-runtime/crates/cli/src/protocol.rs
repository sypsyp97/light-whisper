use crate::{Error, Result, args::LOCAL, config::absolute};
use serde::Serialize;
use serde_json::{Value, json, ser::Formatter, value::RawValue};
use std::{
    collections::BTreeMap,
    io::{self, Write},
    path::Path,
};

// Read the envelope separately so a bad Unicode escape in a field doesn't
// erase the caller's ID. RawValue also distinguishes integer tokens from 1.0.
pub fn caller_id(line: &[u8]) -> Option<Value> {
    let fields: BTreeMap<String, &RawValue> = serde_json::from_slice(line).ok()?;
    let raw = fields.get("request_id")?.get();
    if !integer(raw) {
        return None;
    }
    serde_json::from_str(raw).ok()
}
fn integer(raw: &str) -> bool {
    let digits = raw.strip_prefix('-').unwrap_or(raw);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

pub fn request(message: &Value, desktop: bool, sequence: u64) -> Result<Vec<u8>> {
    let action = message["action"]
        .as_str()
        .ok_or_else(|| Error::input("Unsupported session action"))?;
    let mut wire = json!({"action":action,"request_id":sequence});
    let extra: &[&str] = match action {
        "transcribe" => &["audio", "language", "context", "hot_words"],
        "stats" | "status" | "exit" => &[],
        "models" | "devices" | "engine_get" | "engine_set" | "engine_start" | "engine_restart" => {
            if !desktop {
                return Err(Error::input("This command requires the desktop target"));
            }
            if action == "engine_set" {
                &["engine"]
            } else {
                &[]
            }
        }
        _ => return Err(Error::input("Unsupported session action")),
    };
    let object = message
        .as_object()
        .ok_or_else(|| Error::input("Each request must be an object"))?;
    if object.keys().any(|key| {
        !matches!(key.as_str(), "action" | "request_id") && !extra.contains(&key.as_str())
    }) {
        return Err(Error::input("Request contains unsupported fields"));
    }
    if action == "engine_set" {
        let engine = message["engine"]
            .as_str()
            .filter(|s| LOCAL.contains(s))
            .ok_or_else(|| Error::input("engine_set requires a supported local engine"))?;
        wire["engine"] = engine.into();
    }
    if action == "transcribe" {
        let audio = message["audio"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| Error::input("transcribe requires a nonempty audio path"))?;
        let path = absolute(Path::new(audio))?;
        if !path.is_file() {
            return Err(Error::input(format!(
                "Audio file does not exist: {}",
                path.display()
            )));
        }
        let mut options = json!({});
        for key in ["language", "context"] {
            if let Some(value) = message.get(key) {
                options[key] = value
                    .as_str()
                    .ok_or_else(|| Error::input(format!("{key} must be a string")))?
                    .into();
            }
        }
        wire["audio_path"] = json!(path);
        wire["options"] = options;
        if let Some(words) = message.get("hot_words") {
            if !words
                .as_array()
                .is_some_and(|a| a.iter().all(Value::is_string))
            {
                return Err(Error::input("hot_words must be a list of strings"));
            }
            wire["hot_words"] = words.clone();
        }
    }
    let mut bytes = Vec::new();
    wire.serialize(&mut serde_json::Serializer::with_formatter(
        &mut bytes, WireFormat,
    ))
    .map_err(io::Error::other)?;
    bytes.extend_from_slice(if cfg!(windows) { b"\r\n" } else { b"\n" });
    if desktop && bytes.len() > 64 * 1024 {
        return Err(Error::input(
            "Desktop request exceeds the 64 KiB message limit",
        ));
    }
    Ok(bytes)
}

// The desktop admission boundary includes ASCII escapes, spaces, and the
// platform line ending. Validate the exact bytes sent to the child.
struct WireFormat;
impl Formatter for WireFormat {
    fn begin_array_value<W: ?Sized + Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        if first { Ok(()) } else { w.write_all(b", ") }
    }
    fn begin_object_key<W: ?Sized + Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        self.begin_array_value(w, first)
    }
    fn begin_object_value<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        w.write_all(b": ")
    }
    fn write_string_fragment<W: ?Sized + Write>(
        &mut self,
        w: &mut W,
        frag: &str,
    ) -> io::Result<()> {
        let mut rest = frag;
        while let Some(i) = rest.find(|c: char| c >= '\u{7f}') {
            w.write_all(&rest.as_bytes()[..i])?;
            let c = rest[i..].chars().next().unwrap();
            for unit in c.encode_utf16(&mut [0; 2]) {
                write!(w, "\\u{unit:04x}")?;
            }
            rest = &rest[i + c.len_utf8()..];
        }
        w.write_all(rest.as_bytes())
    }
}

fn finite(value: &Value) -> bool {
    match value {
        Value::Number(n) => integer(n.as_str()) || n.as_f64().is_some_and(f64::is_finite),
        Value::Object(m) => m.values().all(finite),
        Value::Array(a) => a.iter().all(finite),
        _ => true,
    }
}

// Some peers emit Python's NaN/Infinity extensions. Only a structurally
// valid matching response containing such a token terminates the operation.
fn parse_nonfinite_frame(bytes: &[u8]) -> Option<Value> {
    let mut parsed = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let mut quoted = false;
    while i < bytes.len() {
        if quoted && bytes[i] == b'\\' {
            parsed.extend_from_slice(bytes.get(i..i + 2)?);
            i += 2;
            continue;
        }
        if bytes[i] == b'"' {
            quoted = !quoted;
        } else if !quoted
            && let Some(token) = [b"-Infinity".as_slice(), b"Infinity", b"NaN"]
                .into_iter()
                .find(|t| bytes[i..].starts_with(t))
        {
            parsed.extend_from_slice(b" 1e999 ");
            i += token.len();
            continue;
        }
        parsed.push(bytes[i]);
        i += 1;
    }
    serde_json::from_slice(&parsed).ok()
}

pub fn response(line: &[u8], sequence: Option<u64>) -> Result<Option<Value>> {
    let Some(value) = serde_json::from_slice::<Value>(line)
        .ok()
        .or_else(|| parse_nonfinite_frame(line))
    else {
        return Ok(None);
    };
    if !value["success"].is_boolean()
        || sequence.is_some_and(|id| value["request_id"].as_u64() != Some(id))
    {
        return Ok(None);
    }
    if !finite(&value) {
        return Err(Error::runtime(
            "Engine response contains a nonfinite JSON number",
        ));
    }
    if value["fatal"] == true {
        return Err(Error::runtime(
            value["error"]
                .as_str()
                .unwrap_or("Desktop connection failed"),
        ));
    }
    Ok(Some(value))
}
