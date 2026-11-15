use crate::agent_api::secure_files;
use rand::RngCore;
use ring::hmac;
use serde_json::{json, Value};
use std::fs::File;
use std::io::{self, BufRead, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const REQUEST_MAX: usize = 64 * 1024;
const RESPONSE_MAX: usize = 4 * 1024 * 1024;
const SOCKET_BUDGET: Duration = Duration::from_secs(2);
const WORKERS: usize = 4;

pub type Dispatch = Arc<dyn Fn(Value) -> Value + Send + Sync>;
type IngressObserver = Arc<dyn Fn() + Send + Sync>;
type WriteObserver = Arc<dyn Fn(&TcpStream, Instant, usize) + Send + Sync>;

#[derive(Clone)]
struct WriteObservation {
    chunk_bytes: usize,
    after_write: WriteObserver,
}

#[derive(Default)]
struct Admission {
    closing: bool,
    active: usize,
}

struct WorkerSlot(Arc<Mutex<Admission>>);

impl Drop for WorkerSlot {
    fn drop(&mut self) {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .active -= 1;
    }
}

pub struct AgentServer {
    admission: Arc<Mutex<Admission>>,
    accept: Option<JoinHandle<()>>,
    endpoint_path: PathBuf,
    descriptor: Value,
    _lease: File,
}

impl AgentServer {
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn start_observed_writes(
        data_dir: &Path,
        dispatch: Dispatch,
        chunk_bytes: usize,
        after_write: WriteObserver,
    ) -> io::Result<Self> {
        if chunk_bytes == 0 {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        Self::start_core(
            data_dir,
            dispatch,
            None,
            Some(WriteObservation {
                chunk_bytes,
                after_write,
            }),
        )
    }

    pub fn start(data_dir: &Path, dispatch: Dispatch) -> io::Result<Self> {
        Self::start_core(data_dir, dispatch, None, None)
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub fn start_observed(
        data_dir: &Path,
        dispatch: Dispatch,
        on_ingress: IngressObserver,
    ) -> io::Result<Self> {
        Self::start_core(data_dir, dispatch, Some(on_ingress), None)
    }

    fn start_core(
        data_dir: &Path,
        dispatch: Dispatch,
        on_ingress: Option<IngressObserver>,
        write_observation: Option<WriteObservation>,
    ) -> io::Result<Self> {
        let directory = data_dir.join("agent");
        secure_files::ensure_private_dir(&directory)?;
        let lock_path = directory.join("host.lock");
        match secure_files::create_private_file(&lock_path) {
            // Reopen without DELETE desired access so other hosts can open the
            // stable, non-delete-shared file and fail on the actual lease.
            Ok(file) => drop(file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        let lease = secure_files::open_private_file(&lock_path, true)?;
        lease.try_lock().map_err(io::Error::from)?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let token = random_hex::<32>()?;
        let instance = random_hex::<16>()?;
        let descriptor = json!({
            "protocol": 1, "address": listener.local_addr()?.to_string(),
            "token": token, "instance_id": instance, "pid": std::process::id()
        });
        let endpoint_path = directory.join("endpoint.json");
        secure_files::publish_private_file(&endpoint_path, &serde_json::to_vec(&descriptor)?)?;
        let endpoint = Endpoint::parse(&descriptor)?;
        let admission = Arc::new(Mutex::new(Admission::default()));
        let mut server = Self {
            admission: admission.clone(),
            accept: None,
            endpoint_path,
            descriptor,
            _lease: lease,
        };
        server.accept = Some(thread::Builder::new().name("agent-accept".into()).spawn(
            move || {
                accept_connections(
                    listener,
                    endpoint,
                    dispatch,
                    admission,
                    on_ingress,
                    write_observation,
                );
            },
        )?);
        Ok(server)
    }
}

impl Drop for AgentServer {
    fn drop(&mut self) {
        self.admission
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .closing = true;
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
        // Keep the stable lease across observation and unlink. Other cooperating
        // hosts cannot publish until all of this host's workers have drained.
        if read_descriptor(&self.endpoint_path).is_ok_and(|value| value == self.descriptor) {
            let _ = std::fs::remove_file(&self.endpoint_path);
        }
    }
}

fn accept_connections(
    listener: TcpListener,
    endpoint: Endpoint,
    dispatch: Dispatch,
    admission: Arc<Mutex<Admission>>,
    on_ingress: Option<IngressObserver>,
    write_observation: Option<WriteObservation>,
) {
    let endpoint = Arc::new(endpoint);
    let mut workers: Vec<JoinHandle<()>> = Vec::new();
    loop {
        if admission
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .closing
        {
            break;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let accepted = Instant::now();
                let slot = {
                    let mut state = admission.lock().unwrap_or_else(|error| error.into_inner());
                    if state.closing || state.active == WORKERS {
                        None
                    } else {
                        state.active += 1;
                        Some(WorkerSlot(admission.clone()))
                    }
                };
                if let Some(slot) = slot {
                    let endpoint = endpoint.clone();
                    let dispatch = dispatch.clone();
                    let observer = on_ingress.clone();
                    let writes = write_observation.clone();
                    if let Ok(worker) =
                        thread::Builder::new()
                            .name("agent-request".into())
                            .spawn(move || {
                                let _slot = slot;
                                if let Some(observer) = observer {
                                    observer();
                                }
                                let _ = serve_connection(
                                    stream,
                                    &endpoint,
                                    &dispatch,
                                    accepted,
                                    writes.as_ref(),
                                );
                            })
                    {
                        workers.push(worker);
                    }
                } else {
                    let _ = stream.shutdown(Shutdown::Both);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
        let mut index = 0;
        while index < workers.len() {
            if workers[index].is_finished() {
                let _ = workers.swap_remove(index).join();
            } else {
                index += 1;
            }
        }
    }
    drop(listener);
    for worker in workers {
        let _ = worker.join();
    }
}

struct Endpoint {
    address: SocketAddr,
    instance: String,
    key: hmac::Key,
}

impl Endpoint {
    fn parse(value: &Value) -> io::Result<Self> {
        if !exact_keys(
            value,
            &["protocol", "address", "token", "instance_id", "pid"],
        ) || value["protocol"].as_u64() != Some(1)
            || !value["pid"]
                .as_u64()
                .is_some_and(|pid| pid > 0 && pid <= u64::from(u32::MAX))
        {
            return Err(invalid_data());
        }
        let address: SocketAddr = value["address"]
            .as_str()
            .ok_or_else(invalid_data)?
            .parse()
            .map_err(|_| invalid_data())?;
        if address.ip() != Ipv4Addr::LOCALHOST || address.port() == 0 {
            return Err(invalid_data());
        }
        let key = decode_hex(value["token"].as_str().ok_or_else(invalid_data)?, 32)?;
        let instance = value["instance_id"].as_str().ok_or_else(invalid_data)?;
        decode_hex(instance, 16)?;
        Ok(Self {
            address,
            instance: instance.to_owned(),
            key: hmac::Key::new(hmac::HMAC_SHA256, &key),
        })
    }

    fn envelope(&self, domain: &str, nonce: &str, inner: Value) -> io::Result<Value> {
        let canonical = serde_json::to_vec(&json!([domain, 1, self.instance, nonce, inner]))?;
        let tag = hmac::sign(&self.key, &canonical);
        let mut envelope = json!({"protocol": 1, "instance_id": self.instance,
            "nonce": nonce, "auth": hex(tag.as_ref())});
        envelope[domain] = inner;
        Ok(envelope)
    }

    fn authenticate<'a>(
        &self,
        envelope: &'a Value,
        domain: &str,
    ) -> io::Result<(&'a str, &'a Value)> {
        if !exact_keys(
            envelope,
            &["protocol", "instance_id", "nonce", "auth", domain],
        ) || envelope["protocol"].as_u64() != Some(1)
            || envelope["instance_id"].as_str() != Some(self.instance.as_str())
        {
            return Err(invalid_data());
        }
        let nonce = envelope["nonce"].as_str().ok_or_else(invalid_data)?;
        decode_hex(nonce, 16)?;
        let tag = decode_hex(envelope["auth"].as_str().ok_or_else(invalid_data)?, 32)?;
        let inner = &envelope[domain];
        let canonical = serde_json::to_vec(&json!([domain, 1, self.instance, nonce, inner]))?;
        hmac::verify(&self.key, &canonical, &tag).map_err(|_| invalid_data())?;
        Ok((nonce, inner))
    }
}

fn serve_connection(
    mut stream: TcpStream,
    endpoint: &Endpoint,
    dispatch: &Dispatch,
    accepted: Instant,
    write_observation: Option<&WriteObservation>,
) -> io::Result<()> {
    // Winsock accepts inherit the listener's nonblocking mode; worker socket
    // deadlines require blocking reads and writes.
    stream.set_nonblocking(false)?;
    let ingress_deadline = accepted + SOCKET_BUDGET;
    let bytes = read_frame(&mut stream, REQUEST_MAX, ingress_deadline)?;
    let envelope: Value = serde_json::from_slice(&bytes).map_err(|_| invalid_data())?;
    let (nonce, message) = endpoint.authenticate(&envelope, "request")?;
    remaining(ingress_deadline)?;
    let id = request_id(message);
    let (mut response, completed) = if valid_request(message, false) {
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dispatch(message.clone())));
        let completed = Instant::now();
        (
            result.unwrap_or_else(|_| failure(id.clone(), "operation_failed", "Operation failed")),
            completed,
        )
    } else {
        (
            failure(id.clone(), "invalid_request", "Invalid request"),
            Instant::now(),
        )
    };
    if !valid_response(&response) {
        response = failure(id.clone(), "invalid_response", "Invalid response");
    }
    response["request_id"] = id.clone();
    let mut bytes = json_line(&endpoint.envelope("response", nonce, response)?)?;
    if bytes.len() > RESPONSE_MAX {
        bytes = json_line(&endpoint.envelope(
            "response",
            nonce,
            failure(id, "invalid_response", "Invalid response"),
        )?)?;
    }
    write_frame(
        &mut stream,
        &bytes,
        completed + SOCKET_BUDGET,
        write_observation,
    )
}

pub fn request(data_dir: &Path, message: Value, timeout: Duration) -> io::Result<Value> {
    // All transport failures use fixed text: endpoint/authentication input never
    // becomes an error message, including failures returned to the stdin broker.
    request_inner(data_dir, message, timeout).map_err(|error| {
        if local_request_error(&error) {
            error
        } else {
            io::Error::new(error.kind(), "Agent transport failed")
        }
    })
}

fn request_inner(data_dir: &Path, message: Value, timeout: Duration) -> io::Result<Value> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(invalid_data)?;
    remaining(deadline)?;
    if !valid_request(&message, false) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, InvalidRequest));
    }
    let endpoint = Endpoint::parse(&read_descriptor(&data_dir.join("agent/endpoint.json"))?)?;
    remaining(deadline)?;
    let nonce = random_hex::<16>()?;
    let bytes = json_line(&endpoint.envelope("request", &nonce, message.clone())?)?;
    if bytes.len() > REQUEST_MAX {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, InvalidRequest));
    }
    let mut stream = TcpStream::connect_timeout(&endpoint.address, remaining(deadline)?)?;
    write_frame(&mut stream, &bytes, deadline, None)?;
    let bytes = read_frame(&mut stream, RESPONSE_MAX, deadline)?;
    let envelope: Value = serde_json::from_slice(&bytes).map_err(|_| invalid_data())?;
    let (received_nonce, response) = endpoint.authenticate(&envelope, "response")?;
    if received_nonce != nonce
        || !valid_response(response)
        || response["request_id"].as_u64() != message["request_id"].as_u64()
    {
        return Err(invalid_data());
    }
    remaining(deadline)?;
    Ok(response.clone())
}

fn read_descriptor(path: &Path) -> io::Result<Value> {
    let file = secure_files::open_private_file(path, false)?;
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(invalid_data());
    }
    serde_json::from_slice(&bytes).map_err(|_| invalid_data())
}

fn read_frame(stream: &mut TcpStream, maximum: usize, deadline: Instant) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    while bytes.len() < maximum {
        stream.set_read_timeout(Some(remaining(deadline)?))?;
        let capacity = buffer.len().min(maximum - bytes.len());
        let length = match stream.read(&mut buffer[..capacity]) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        remaining(deadline)?;
        if length == 0 {
            return Err(invalid_data());
        }
        if let Some(end) = buffer[..length].iter().position(|byte| *byte == b'\n') {
            bytes.extend_from_slice(&buffer[..=end]);
            return Ok(bytes);
        }
        bytes.extend_from_slice(&buffer[..length]);
    }
    Err(invalid_data())
}

fn write_frame(
    stream: &mut TcpStream,
    mut bytes: &[u8],
    deadline: Instant,
    observation: Option<&WriteObservation>,
) -> io::Result<()> {
    while !bytes.is_empty() {
        stream.set_write_timeout(Some(remaining(deadline)?))?;
        let capacity = observation.map_or(bytes.len(), |observer| {
            bytes.len().min(observer.chunk_bytes)
        });
        let length = match stream.write(&bytes[..capacity]) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if length == 0 {
            return Err(io::ErrorKind::WriteZero.into());
        }
        if let Some(observer) = observation {
            (observer.after_write)(stream, deadline, length);
        }
        remaining(deadline)?;
        bytes = &bytes[length..];
    }
    Ok(())
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|value| !value.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "Agent transport timed out"))
}

fn exact_keys(value: &Value, keys: &[&str]) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == keys.len() && object.keys().all(|key| keys.contains(&key.as_str()))
    })
}

fn request_id(message: &Value) -> Value {
    message
        .get("request_id")
        .filter(|id| id.as_u64().is_some())
        .cloned()
        .unwrap_or(Value::Null)
}

fn valid_request(message: &Value, allow_exit: bool) -> bool {
    if message.get("request_id").and_then(Value::as_u64).is_none() {
        return false;
    }
    match message["action"].as_str() {
        Some(
            "status" | "stats" | "models" | "devices" | "engine_get" | "engine_start"
            | "engine_restart",
        ) => exact_keys(message, &["request_id", "action"]),
        Some("exit") if allow_exit => exact_keys(message, &["request_id", "action"]),
        Some("engine_set") => {
            exact_keys(message, &["request_id", "action", "engine"])
                && matches!(
                    message["engine"].as_str(),
                    Some("qwen3-asr-0.6b" | "confucius4-r2t2")
                )
        }
        Some("transcribe") => {
            let keys: &[&str] = if message.get("hot_words").is_some() {
                &["request_id", "action", "audio_path", "options", "hot_words"]
            } else {
                &["request_id", "action", "audio_path", "options"]
            };
            exact_keys(message, keys)
                && message["audio_path"]
                    .as_str()
                    .is_some_and(|path| !path.is_empty())
                && message["options"].as_object().is_some_and(|options| {
                    options.iter().all(|(key, value)| {
                        matches!(key.as_str(), "language" | "context") && value.is_string()
                    })
                })
                && message.get("hot_words").is_none_or(|words| {
                    words
                        .as_array()
                        .is_some_and(|words| words.iter().all(Value::is_string))
                })
        }
        _ => false,
    }
}

fn forbidden_payload(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            matches!(key.as_str(), "token" | "protocol" | "instance_id") || forbidden_payload(value)
        }),
        Value::Array(array) => array.iter().any(forbidden_payload),
        _ => false,
    }
}

fn valid_response(response: &Value) -> bool {
    let Some(success) = response.get("success").and_then(Value::as_bool) else {
        return false;
    };
    !forbidden_payload(response)
        && (success
            || (response["error"]
                .as_str()
                .is_some_and(|error| !error.is_empty())
                && response["code"].as_str().is_some_and(|code| {
                    !code.is_empty()
                        && code
                            .bytes()
                            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
                })))
}

fn failure(id: Value, code: &str, message: &str) -> Value {
    json!({"success": false, "request_id": id, "code": code, "error": message})
}

fn invalid_data() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid agent transport data")
}

#[derive(Debug)]
struct InvalidRequest;

impl std::fmt::Display for InvalidRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Invalid request")
    }
}

impl std::error::Error for InvalidRequest {}

fn local_request_error(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|error| error.is::<InvalidRequest>())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(DIGITS[usize::from(byte >> 4)] as char);
        text.push(DIGITS[usize::from(byte & 15)] as char);
    }
    text
}

fn decode_hex(text: &str, size: usize) -> io::Result<Vec<u8>> {
    if text.len() != size * 2
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid_data());
    }
    Ok(text
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digit = |byte| {
                if byte <= b'9' {
                    byte - b'0'
                } else {
                    byte - b'a' + 10
                }
            };
            (digit(pair[0]) << 4) | digit(pair[1])
        })
        .collect())
}

fn random_hex<const N: usize>() -> io::Result<String> {
    let mut bytes = [0u8; N];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(io::Error::other)?;
    Ok(hex(&bytes))
}

fn json_line(value: &Value) -> io::Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn run_client(
    data_dir: &Path,
    input: &mut impl BufRead,
    output: &mut impl Write,
    timeout: Duration,
) -> io::Result<()> {
    write_reply(
        output,
        &json!({"success": true, "protocol": 1, "surface": "desktop-client",
        "agent_protocol": "LIGHT_WHISPER_AGENT_PROTOCOL_1"}),
    )?;
    loop {
        // Retain a bounded record, draining an oversized one before continuing.
        // Stdin itself has no network deadline; a complete record is the unit.
        let mut bytes = Vec::new();
        let mut oversized = false;
        loop {
            let available = input.fill_buf()?;
            if available.is_empty() {
                break;
            }
            let count = available
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(available.len(), |end| end + 1);
            let complete = available[count - 1] == b'\n';
            if !oversized && bytes.len() + count <= REQUEST_MAX {
                bytes.extend_from_slice(&available[..count]);
            } else {
                oversized = true;
            }
            input.consume(count);
            if complete {
                break;
            }
        }
        if bytes.is_empty() && !oversized {
            return Ok(());
        }
        if !oversized && bytes.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let message = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
        let id = request_id(&message);
        if oversized || bytes.last() != Some(&b'\n') || !valid_request(&message, true) {
            write_reply(output, &failure(id, "invalid_request", "Invalid request"))?;
            continue;
        }
        if message["action"] == "exit" {
            return write_reply(output, &json!({"success": true, "request_id": id}));
        }
        match request(data_dir, message, timeout) {
            Ok(response) => write_reply(output, &response)?,
            Err(error) if local_request_error(&error) => {
                write_reply(output, &failure(id, "invalid_request", "Invalid request"))?;
            }
            Err(error) => {
                let mut response = failure(id, "transport_error", "Agent transport failed");
                response["fatal"] = json!(true);
                write_reply(output, &response)?;
                return Err(error);
            }
        }
    }
}

fn write_reply(output: &mut impl Write, response: &Value) -> io::Result<()> {
    output.write_all(&json_line(response)?)?;
    output.flush()
}
