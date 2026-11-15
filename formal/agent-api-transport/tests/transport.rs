#![cfg(windows)]

#[allow(unused_imports)]
use agent_api_transport_verification::{agent_api, secure_files};
#[path = "../../../src-tauri/src/agent_api/transport.rs"]
mod transport;
use ring::hmac;
use serde_json::{json, Value};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Cursor, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[path = "../../agent-api-security/tests/support/mod.rs"]
mod acl;

const TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_MAX: usize = 64 * 1024;
const RESPONSE_MAX: usize = 4 * 1024 * 1024;
const PROBE: &str = env!("CARGO_BIN_EXE_agent-transport-probe");
const SYNTHETIC_SECRET: &str = "synthetic-private-controller-value";
const CHILD_CASE: &str = "LIGHT_WHISPER_TRANSPORT_TEST_CASE";

// Rust threads cannot be killed. A hung deadline/Drop mutant therefore runs in
// one owned test subprocess; its parent enforces a real process deadline, drains
// pipes and removes the exact temporary subtree only after process termination.
macro_rules! bounded_test {
    ($name:ident, $body:block) => {
        #[test]
        fn $name() {
            if std::env::var(CHILD_CASE).as_deref() == Ok(stringify!($name)) {
                $body
            } else {
                let fixture = acl::Fixture::new();
                let mut command = Command::new(std::env::current_exe().unwrap());
                command
                    .args([
                        "--exact",
                        stringify!($name),
                        "--nocapture",
                        "--test-threads=1",
                    ])
                    .env(CHILD_CASE, stringify!($name))
                    .env("TEMP", &fixture.0)
                    .env("TMP", &fixture.0);
                let output = bounded_output(&mut command, Duration::from_secs(20));
                let diagnostic = String::from_utf8_lossy(&output.stdout).to_string()
                    + &String::from_utf8_lossy(&output.stderr);
                assert!(
                    !output.timed_out,
                    "owned test watchdog terminated stalled case after cleanup: {}\n{}",
                    stringify!($name),
                    diagnostic
                );
                assert!(
                    output.status.success(),
                    "native case failed after owned process/pipe cleanup: {}\n{}",
                    stringify!($name),
                    diagnostic
                );
            }
        }
    };
}

struct BoundedOutput {
    status: ExitStatus,
    timed_out: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn drain_tail(mut pipe: impl Read) -> Vec<u8> {
    const DIAGNOSTIC_MAX: usize = 128 * 1024;
    let mut tail = Vec::new();
    let mut chunk = [0u8; 8192];
    while let Ok(length) = pipe.read(&mut chunk) {
        if length == 0 {
            break;
        }
        tail.extend_from_slice(&chunk[..length]);
        if tail.len() > DIAGNOSTIC_MAX {
            tail.drain(..tail.len() - DIAGNOSTIC_MAX);
        }
    }
    tail
}

fn bounded_output(command: &mut Command, timeout: Duration) -> BoundedOutput {
    let mut child = OwnedChild(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let stderr = child.0.stderr.take().unwrap();
    let out = thread::spawn(move || drain_tail(stdout));
    let err = thread::spawn(move || drain_tail(stderr));
    let (status, timed_out) = child.wait_bounded(timeout);
    drop(child);
    BoundedOutput {
        status,
        timed_out,
        stdout: out.join().unwrap(),
        stderr: err.join().unwrap(),
    }
}

fn endpoint_path(data: &Path) -> PathBuf {
    data.join("agent/endpoint.json")
}

fn descriptor(data: &Path) -> Value {
    let mut file = secure_files::open_private_file(&endpoint_path(data), false)
        .expect("server must publish a native private endpoint");
    acl::assert_private(&file);
    acl::assert_plain_file(&file);
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() <= 4096);
    serde_json::from_slice(&bytes).unwrap()
}

fn keys(value: &Value) -> Vec<&str> {
    let mut result: Vec<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    result.sort();
    result
}

fn mac(token: &str, domain: &str, instance: &Value, nonce: &Value, inner: &Value) -> String {
    let key: Vec<u8> = (0..token.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&token[index..index + 2], 16).unwrap())
        .collect();
    let canonical = serde_json::to_vec(&json!([domain, 1, instance, nonce, inner])).unwrap();
    let tag = hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, &key), &canonical);
    tag.as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn signed(endpoint: &Value, domain: &str, nonce: Value, inner: Value) -> Value {
    let auth = mac(
        endpoint["token"].as_str().unwrap(),
        domain,
        &endpoint["instance_id"],
        &nonce,
        &inner,
    );
    let mut envelope = json!({"protocol": 1, "instance_id": endpoint["instance_id"], "nonce": nonce, "auth": auth});
    envelope[domain] = inner;
    envelope
}

fn frame(endpoint: &Value, request: Value) -> Value {
    signed(endpoint, "request", json!("c".repeat(32)), request)
}

fn assert_signed_response(endpoint: &Value, envelope: &Value, nonce: &Value) {
    assert_eq!(
        keys(envelope),
        ["auth", "instance_id", "nonce", "protocol", "response"]
    );
    assert_eq!(envelope["protocol"].as_u64(), Some(1));
    assert_eq!(envelope["instance_id"], endpoint["instance_id"]);
    assert_eq!(&envelope["nonce"], nonce);
    assert_eq!(
        envelope["auth"],
        mac(
            endpoint["token"].as_str().unwrap(),
            "response",
            &envelope["instance_id"],
            &envelope["nonce"],
            &envelope["response"]
        )
    );
}

fn line(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(value).unwrap();
    bytes.push(b'\n');
    bytes
}

fn multibyte_text(bytes: usize) -> String {
    "界".repeat(bytes / 3) + &"a".repeat(bytes % 3)
}

fn connect(endpoint: &Value) -> TcpStream {
    let address: SocketAddr = endpoint["address"].as_str().unwrap().parse().unwrap();
    let stream = TcpStream::connect_timeout(&address, TIMEOUT).unwrap();
    stream.set_read_timeout(Some(TIMEOUT)).unwrap();
    stream.set_write_timeout(Some(TIMEOUT)).unwrap();
    stream
}

fn exchange_bytes(endpoint: &Value, bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut stream = connect(endpoint);
    if let Err(error) = stream.write_all(bytes) {
        return if matches!(
            error.kind(),
            io::ErrorKind::ConnectionReset
                | io::ErrorKind::ConnectionAborted
                | io::ErrorKind::BrokenPipe
        ) {
            Ok(Vec::new())
        } else {
            Err(error)
        };
    }
    let _ = stream.shutdown(Shutdown::Write);
    let mut reply = Vec::new();
    if let Err(error) = BufReader::new(stream).read_until(b'\n', &mut reply) {
        if !matches!(
            error.kind(),
            io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted
        ) {
            return Err(error);
        }
    }
    Ok(reply)
}

fn exchange(endpoint: &Value, value: &Value) -> Value {
    let bytes = exchange_bytes(endpoint, &line(value)).expect("local server response");
    assert!(
        !bytes.is_empty(),
        "valid authenticated record must receive a reply"
    );
    assert!(bytes.len() <= RESPONSE_MAX);
    assert_eq!(bytes.last(), Some(&b'\n'));
    let response = serde_json::from_slice(&bytes).unwrap();
    assert_signed_response(endpoint, &response, &value["nonce"]);
    response
}

fn assert_error(value: &Value, request_id: Value, code: &str) {
    assert_eq!(value["success"], false);
    assert_eq!(value["request_id"], request_id);
    assert_eq!(value["code"], code);
    assert!(value["error"].as_str().is_some_and(|text| !text.is_empty()));
}

fn assert_redacted(bytes: &[u8], endpoint: &Value) {
    let text = String::from_utf8_lossy(bytes);
    assert!(
        !text.contains(endpoint["token"].as_str().unwrap()),
        "authentication token leaked"
    );
    assert!(
        !text.contains(SYNTHETIC_SECRET),
        "controller/private input leaked"
    );
}

fn start_counted(data: &Path) -> (transport::AgentServer, Arc<Mutex<Vec<Value>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let seen = calls.clone();
    let server = transport::AgentServer::start(
        data,
        Arc::new(move |message| {
            seen.lock().unwrap().push(message);
            json!({"success": true, "observation": "synthetic-controller"})
        }),
    )
    .expect("native server start must implement the frozen transport contract");
    (server, calls)
}

bounded_test!(
    start_publishes_private_strict_descriptor_and_owns_stable_lock,
    {
        let fixture = acl::Fixture::new();
        let (server, calls) = start_counted(&fixture.0);
        let endpoint = descriptor(&fixture.0);
        assert_eq!(
            keys(&endpoint),
            ["address", "instance_id", "pid", "protocol", "token"]
        );
        assert_eq!(endpoint["protocol"].as_u64(), Some(1));
        let address: SocketAddr = endpoint["address"].as_str().unwrap().parse().unwrap();
        assert_eq!(address.ip().to_string(), "127.0.0.1");
        assert_ne!(address.port(), 0);
        assert!(endpoint["pid"]
            .as_u64()
            .is_some_and(|pid| pid > 0 && pid <= u32::MAX as u64));
        for (field, length) in [("token", 64), ("instance_id", 32)] {
            let value = endpoint[field].as_str().unwrap();
            assert_eq!(value.len(), length);
            assert!(value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        }
        acl::assert_private(&acl::open_directory(&fixture.0.join("agent")));
        let lock =
            secure_files::open_private_file(&fixture.0.join("agent/host.lock"), true).unwrap();
        acl::assert_private(&lock);
        assert!(
            lock.try_lock().is_err(),
            "server must retain the real exclusive lease"
        );
        assert!(
            calls.lock().unwrap().is_empty(),
            "start must not probe the controller"
        );
        drop(lock);
        drop(server);
        assert!(!endpoint_path(&fixture.0).exists());
        assert!(fixture.0.join("agent/host.lock").is_file());
    }
);

bounded_test!(
    request_crosses_real_authenticated_socket_and_server_controls_response_identity,
    {
        let fixture = acl::Fixture::new();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = calls.clone();
        let server = transport::AgentServer::start(
            &fixture.0,
            Arc::new(move |message| {
                observed.lock().unwrap().push(message);
                json!({"success": true, "request_id": 999, "data": {"source": "synthetic"}})
            }),
        )
        .expect("start authenticated socket");
        let message = json!({"request_id": u64::MAX, "action": "status"});
        let reply = transport::request(&fixture.0, message.clone(), TIMEOUT).expect("round trip");
        assert_eq!(
            reply,
            json!({"success": true, "request_id": u64::MAX,
                            "data": {"source": "synthetic"}})
        );
        assert_eq!(*calls.lock().unwrap(), [message]);
        drop(server);
    }
);

bounded_test!(
    every_allowlisted_shape_dispatches_and_backend_failure_remains_recoverable,
    {
        let fixture = acl::Fixture::new();
        let (server, calls) = start_counted(&fixture.0);
        let mut requests: Vec<Value> = [
            "status",
            "stats",
            "models",
            "devices",
            "engine_get",
            "engine_start",
            "engine_restart",
        ]
        .into_iter()
        .map(|action| json!({"request_id": 0, "action": action}))
        .collect();
        for engine in ["qwen3-asr-0.6b", "confucius4-r2t2"] {
            requests.push(json!({"request_id": 0, "action": "engine_set", "engine": engine}));
        }
        requests.push(json!({"request_id": 0, "action": "transcribe", "audio_path": "synthetic.wav", "options": {}}));
        requests.push(json!({"request_id": 0, "action": "transcribe", "audio_path": "synthetic.wav",
                        "options": {"language": "", "context": "synthetic context"}, "hot_words": ["one", "二"]}));
        requests.push(
            json!({"request_id": 0, "action": "transcribe", "audio_path": "synthetic.wav",
                        "options": {}, "hot_words": []}),
        );
        for (index, message) in requests.iter_mut().enumerate() {
            message["request_id"] = json!(index as u64);
            assert_eq!(
                transport::request(&fixture.0, message.clone(), TIMEOUT).unwrap()["success"],
                true
            );
        }
        assert_eq!(*calls.lock().unwrap(), requests);
        drop(server);

        let server = transport::AgentServer::start(
        &fixture.0,
        Arc::new(
            |_| json!({"success": false, "code": "operation_failed", "error": "synthetic failure"}),
        ),
    )
    .unwrap();
        for id in [10, 11] {
            let result = transport::request(
                &fixture.0,
                json!({"request_id": id, "action": "stats"}),
                TIMEOUT,
            )
            .expect("valid server failure is an ordinary reply");
            assert_error(&result, json!(id), "operation_failed");
        }
        drop(server);
    }
);

bounded_test!(
    server_rejects_inner_schema_partitions_before_any_dispatch,
    {
        let fixture = acl::Fixture::new();
        let (server, calls) = start_counted(&fixture.0);
        let endpoint = descriptor(&fixture.0);
        let mut invalid = vec![
            Value::Null,
            json!([]),
            json!({}),
            json!({"request_id": 7}),
            json!({"request_id": -1, "action": "status"}),
            json!({"request_id": 1.0, "action": "status"}),
            json!({"request_id": true, "action": "status"}),
            json!({"request_id": "1", "action": "status"}),
        ];
        for action in [
            json!("exit"),
            json!("invoke"),
            json!("download"),
            json!("record"),
            Value::Null,
            json!(true),
            json!([]),
        ] {
            invalid.push(json!({"request_id": 7, "action": action}));
        }
        invalid.push(json!({"request_id": 7, "action": "status", "command": SYNTHETIC_SECRET}));
        invalid.push(json!({"request_id": 7, "action": "status", "hot_words": []}));
        for engine in [
            Value::Null,
            json!("cloud"),
            json!(true),
            json!("localQwen"),
            json!("R2T2"),
        ] {
            invalid.push(json!({"request_id": 7, "action": "engine_set", "engine": engine}));
        }
        invalid
            .push(json!({"request_id": 7, "action": "engine_set", "engine": "confucius4-r2t2", "extra": 1}));
        invalid
            .push(json!({"request_id": 7, "action": "transcribe", "audio_path": "synthetic.wav"}));
        for path in [Value::Null, json!(""), json!(3)] {
            invalid.push(
                json!({"request_id": 7, "action": "transcribe", "audio_path": path, "options": {}}),
            );
        }
        for options in [
            Value::Null,
            json!([]),
            json!("x"),
            json!({"unknown": true}),
            json!({"language": 2}),
            json!({"context": false}),
            json!({"hot_words": "x"}),
            json!({"hot_words": ["x", 2]}),
            json!({"hot_words": ["valid-at-top-level-only"]}),
        ] {
            invalid.push(json!({"request_id": 7, "action": "transcribe", "audio_path": "synthetic.wav", "options": options}));
        }
        for hot_words in [
            Value::Null,
            json!("x"),
            json!(true),
            json!(2),
            json!({}),
            json!(["x", 2]),
            json!([Value::Null]),
        ] {
            invalid.push(json!({"request_id": 7, "action": "transcribe", "audio_path": "synthetic.wav", "options": {}, "hot_words": hot_words}));
        }
        for message in invalid {
            let id = message
                .get("request_id")
                .filter(|id| id.as_u64().is_some())
                .cloned()
                .unwrap_or(Value::Null);
            let reply = exchange(&endpoint, &frame(&endpoint, message));
            assert_error(&reply["response"], id, "invalid_request");
            assert_redacted(&line(&reply), &endpoint);
        }
        assert!(calls.lock().unwrap().is_empty());
        assert_eq!(
            transport::request(
                &fixture.0,
                json!({"request_id": 8, "action": "status"}),
                TIMEOUT
            )
            .unwrap()["success"],
            true
        );
        assert_eq!(calls.lock().unwrap().len(), 1);
        drop(server);
    }
);

bounded_test!(
    header_and_authentication_partitions_never_reach_dispatch_or_echo_secrets,
    {
        let fixture = acl::Fixture::new();
        let (server, calls) = start_counted(&fixture.0);
        let endpoint = descriptor(&fixture.0);
        let good = frame(&endpoint, json!({"request_id": 2, "action": "status"}));
        let mut invalid = vec![Value::Null, json!([]), json!({})];
        for (field, bad) in [
            ("protocol", json!(2)),
            ("protocol", json!("1")),
            ("protocol", json!(true)),
            ("instance_id", json!("d".repeat(32))),
            ("instance_id", Value::Null),
            ("auth", json!("0".repeat(64))),
            ("auth", json!(1)),
            ("nonce", json!("0".repeat(31))),
            ("nonce", json!("G".repeat(32))),
        ] {
            let mut value = good.clone();
            value[field] = bad;
            if matches!(field, "nonce" | "instance_id") {
                // Isolate these guards: a perfectly valid MAC must not make an
                // invalid nonce or foreign instance eligible for dispatch.
                value["auth"] = json!(mac(
                    endpoint["token"].as_str().unwrap(),
                    "request",
                    &value["instance_id"],
                    &value["nonce"],
                    &value["request"]
                ));
            }
            invalid.push(value);
        }
        for field in ["protocol", "auth", "nonce", "instance_id", "request"] {
            let mut value = good.clone();
            value.as_object_mut().unwrap().remove(field);
            invalid.push(value);
        }
        let mut extra = good.clone();
        extra["unexpected"] = json!(SYNTHETIC_SECRET);
        invalid.push(extra);
        let mut tampered = good.clone();
        tampered["request"]["action"] = json!("engine_restart");
        invalid.push(tampered);
        let mut request_domain_replay = good.clone();
        request_domain_replay["auth"] = json!(mac(
            endpoint["token"].as_str().unwrap(),
            "response",
            &good["instance_id"],
            &good["nonce"],
            &good["request"]
        ));
        invalid.push(request_domain_replay);
        for value in invalid {
            let bytes = exchange_bytes(&endpoint, &line(&value)).expect("bounded auth rejection");
            assert_redacted(&bytes, &endpoint);
            if !bytes.is_empty() {
                let reply: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(reply["response"]["success"], false);
                assert!(matches!(
                    reply["response"]["code"].as_str(),
                    Some("authentication_failed" | "invalid_request")
                ));
            }
        }
        assert!(calls.lock().unwrap().is_empty());
        drop(server);
    }
);

bounded_test!(
    server_requires_complete_bounded_jsonl_and_accepts_crlf_and_last_duplicate_key,
    {
        let fixture = acl::Fixture::new();
        let (server, calls) = start_counted(&fixture.0);
        let endpoint = descriptor(&fixture.0);
        let good = frame(&endpoint, json!({"request_id": 4, "action": "status"}));
        for bytes in [
            b"{broken\n".to_vec(),
            serde_json::to_vec(&good).unwrap(),
            vec![b'x'; REQUEST_MAX + 1],
            vec![0xff, b'\n'],
        ] {
            let reply = exchange_bytes(&endpoint, &bytes).unwrap();
            assert_redacted(&reply, &endpoint);
        }
        assert!(
            calls.lock().unwrap().is_empty(),
            "malformed, truncated and oversized frames must not dispatch"
        );
        let mut bytes = serde_json::to_vec(&good).unwrap();
        bytes.extend_from_slice(b"\r\n");
        let reply: Value =
            serde_json::from_slice(&exchange_bytes(&endpoint, &bytes).unwrap()).unwrap();
        assert_eq!(reply["response"]["success"], true);
        let duplicate = format!("{{\"protocol\":2,\"protocol\":1,\"instance_id\":{},\"nonce\":{},\"auth\":{},\"request\":{{\"request_id\":4,\"action\":\"exit\",\"action\":\"status\"}}}}\n",
        endpoint["instance_id"], good["nonce"], good["auth"]);
        assert_eq!(
            serde_json::from_slice::<Value>(
                &exchange_bytes(&endpoint, duplicate.as_bytes()).unwrap()
            )
            .unwrap()["response"]["success"],
            true
        );
        assert_eq!(calls.lock().unwrap().len(), 2);
        drop(server);
    }
);

bounded_test!(
    request_size_cap_counts_newline_and_utf8_bytes_at_the_boundary,
    {
        let fixture = acl::Fixture::new();
        let (server, calls) = start_counted(&fixture.0);
        let endpoint = descriptor(&fixture.0);
        let mut message = json!({"request_id": 10, "action": "transcribe", "audio_path": "synthetic.wav", "options": {"context": ""}});
        let overhead = line(&frame(&endpoint, message.clone())).len();
        message["options"]["context"] = json!("a".repeat(REQUEST_MAX - overhead));
        let maximum = line(&frame(&endpoint, message.clone()));
        assert_eq!(maximum.len(), REQUEST_MAX);
        assert_eq!(
            exchange(&endpoint, &frame(&endpoint, message.clone()))["response"]["success"],
            true
        );
        message["options"]["context"].as_str().unwrap();
        let mut too_large = maximum.clone();
        too_large.insert(too_large.len() - 1, b' ');
        assert_eq!(too_large.len(), REQUEST_MAX + 1);
        let _ = exchange_bytes(&endpoint, &too_large);
        let mut crlf_large = maximum;
        crlf_large.insert(crlf_large.len() - 1, b'\r');
        let _ = exchange_bytes(&endpoint, &crlf_large);
        assert_eq!(
            calls.lock().unwrap().len(),
            1,
            "LF/CRLF bytes count toward admission bound"
        );
        message["options"]["context"] = json!(multibyte_text(REQUEST_MAX - overhead));
        let unicode_maximum = line(&frame(&endpoint, message.clone()));
        assert_eq!(unicode_maximum.len(), REQUEST_MAX);
        assert!(String::from_utf8_lossy(&unicode_maximum).chars().count() < REQUEST_MAX);
        assert_eq!(
            exchange(&endpoint, &frame(&endpoint, message.clone()))["response"]["success"],
            true
        );
        message["options"]["context"] =
            json!(message["options"]["context"].as_str().unwrap().to_owned() + "界");
        let unicode_excess = line(&frame(&endpoint, message));
        assert_eq!(unicode_excess.len(), REQUEST_MAX + 3);
        assert!(
            String::from_utf8_lossy(&unicode_excess).chars().count() < REQUEST_MAX,
            "a character-count limit would incorrectly accept this signed record"
        );
        let _ = exchange_bytes(&endpoint, &unicode_excess);
        assert_eq!(
            calls.lock().unwrap().len(),
            2,
            "UTF8 byte overflow must not dispatch"
        );
        drop(server);
    }
);

bounded_test!(
    invalid_controller_replies_and_panics_are_sanitized_and_keep_server_usable,
    {
        let fixture = acl::Fixture::new();
        let outcomes = Arc::new(Mutex::new(
            vec![
                Value::Null,
                json!([]),
                json!({}),
                json!({"success": "true"}),
                json!({"success": true, "token": SYNTHETIC_SECRET}),
                json!({"success": true, "protocol": 1}),
                json!({"success": true, "instance_id": SYNTHETIC_SECRET}),
                json!({"success": true, "data": {"token": SYNTHETIC_SECRET}}),
                json!({"success": true, "data": [{"protocol": 1}]}),
                json!({"success": true, "data": {"instance_id": SYNTHETIC_SECRET}}),
            ]
            .into_iter(),
        ));
        let next = outcomes.clone();
        let server = transport::AgentServer::start(
            &fixture.0,
            Arc::new(move |_| next.lock().unwrap().next().unwrap()),
        )
        .unwrap();
        let endpoint = descriptor(&fixture.0);
        for id in 0..10 {
            let reply = transport::request(
                &fixture.0,
                json!({"request_id": id, "action": "stats"}),
                TIMEOUT,
            )
            .unwrap();
            assert_error(&reply, json!(id), "invalid_response");
            assert_redacted(&line(&reply), &endpoint);
        }
        drop(server);
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        let server = transport::AgentServer::start(
            &fixture.0,
            Arc::new(move |_| {
                if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                    panic!("{SYNTHETIC_SECRET}");
                }
                json!({"success": true})
            }),
        )
        .unwrap();
        let endpoint = descriptor(&fixture.0);
        let reply = transport::request(
            &fixture.0,
            json!({"request_id": 81, "action": "status"}),
            TIMEOUT,
        )
        .unwrap();
        assert_error(&reply, json!(81), "operation_failed");
        assert_redacted(&line(&reply), &endpoint);
        assert_eq!(
            transport::request(
                &fixture.0,
                json!({"request_id": 82, "action": "status"}),
                TIMEOUT
            )
            .unwrap()["success"],
            true
        );
        drop(server);
    }
);

bounded_test!(response_size_cap_includes_wrapper_and_newline, {
    let fixture = acl::Fixture::new();
    let length = Arc::new(AtomicUsize::new(0));
    let size = length.clone();
    let multibyte = Arc::new(AtomicBool::new(false));
    let unicode = multibyte.clone();
    let server = transport::AgentServer::start(
        &fixture.0,
        Arc::new(move |_| {
            let count = size.load(Ordering::SeqCst);
            json!({"success": true, "data": if unicode.load(Ordering::SeqCst) { multibyte_text(count) } else { "x".repeat(count) }})
        }),
    )
    .unwrap();
    let endpoint = descriptor(&fixture.0);
    let base = signed(
        &endpoint,
        "response",
        json!("c".repeat(32)),
        json!({"success": true, "request_id": 1, "data": ""}),
    );
    length.store(RESPONSE_MAX - line(&base).len(), Ordering::SeqCst);
    let request = frame(&endpoint, json!({"request_id": 1, "action": "stats"}));
    let bytes = exchange_bytes(&endpoint, &line(&request)).unwrap();
    assert_eq!(bytes.len(), RESPONSE_MAX);
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["response"]["success"],
        true
    );
    length.fetch_add(1, Ordering::SeqCst);
    let reply = exchange(&endpoint, &request);
    assert_error(&reply["response"], json!(1), "invalid_response");
    multibyte.store(true, Ordering::SeqCst);
    length.store(RESPONSE_MAX - line(&base).len(), Ordering::SeqCst);
    let bytes = exchange_bytes(&endpoint, &line(&request)).unwrap();
    assert_eq!(bytes.len(), RESPONSE_MAX);
    assert!(String::from_utf8_lossy(&bytes).chars().count() < RESPONSE_MAX);
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["response"]["success"],
        true
    );
    length.fetch_add(3, Ordering::SeqCst);
    let reply = exchange(&endpoint, &request);
    assert_error(&reply["response"], json!(1), "invalid_response");
    drop(server);
});

fn fake_descriptor(address: SocketAddr) -> Value {
    json!({"protocol": 1, "address": address.to_string(), "token": "a".repeat(64),
        "instance_id": "b".repeat(32), "pid": std::process::id()})
}

fn publish(data: &Path, endpoint: &Value) {
    secure_files::ensure_private_dir(&data.join("agent")).unwrap();
    secure_files::publish_private_file(
        &endpoint_path(data),
        &serde_json::to_vec(endpoint).unwrap(),
    )
    .unwrap();
}

struct Peer {
    endpoint: Value,
    address: SocketAddr,
    worker: Option<thread::JoinHandle<Option<Value>>>,
}
impl Peer {
    fn new(data: &Path, respond: impl FnOnce(&Value) -> Vec<u8> + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let endpoint = fake_descriptor(address);
        publish(data, &endpoint);
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(TIMEOUT)).unwrap();
            stream.set_write_timeout(Some(TIMEOUT)).unwrap();
            let mut bytes = Vec::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_until(b'\n', &mut bytes)
                .ok()?;
            let request: Value = serde_json::from_slice(&bytes).ok()?;
            let _ = stream.write_all(&respond(&request));
            Some(request)
        });
        Self {
            endpoint,
            address,
            worker: Some(worker),
        }
    }

    fn finish(mut self) -> Option<Value> {
        // Wake only our own unused listener when scaffold/request validation never connected.
        if let Ok(stream) = TcpStream::connect_timeout(&self.address, Duration::from_millis(100)) {
            let _ = stream.shutdown(Shutdown::Both);
        }
        self.worker.take().unwrap().join().unwrap()
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        if self.worker.is_some() {
            if let Ok(stream) =
                TcpStream::connect_timeout(&self.address, Duration::from_millis(100))
            {
                let _ = stream.shutdown(Shutdown::Both);
            }
            let _ = self.worker.take().unwrap().join();
        }
    }
}

fn good_response(envelope: &Value) -> Value {
    let endpoint = json!({"token": "a".repeat(64), "instance_id": envelope["instance_id"]});
    signed(
        &endpoint,
        "response",
        envelope["nonce"].clone(),
        json!({"success": true, "request_id": envelope["request"]["request_id"]}),
    )
}

fn resign_response(envelope: &mut Value) {
    envelope["auth"] = json!(mac(
        &"a".repeat(64),
        "response",
        &envelope["instance_id"],
        &envelope["nonce"],
        &envelope["response"]
    ));
}

bounded_test!(
    client_uses_exact_network_envelope_and_accepts_correlated_failure,
    {
        let fixture = acl::Fixture::new();
        let peer = Peer::new(&fixture.0, |request| {
            let mut reply = good_response(request);
            reply["response"] = json!({"success": false, "request_id": 55,
            "code": "operation_failed", "error": "synthetic failure"});
            resign_response(&mut reply);
            line(&reply)
        });
        let endpoint = peer.endpoint.clone();
        let message = json!({"request_id": 55, "action": "engine_get"});
        let result = transport::request(&fixture.0, message.clone(), TIMEOUT);
        let sent = peer
            .finish()
            .expect("client must connect to the descriptor socket");
        assert_eq!(
            keys(&sent),
            ["auth", "instance_id", "nonce", "protocol", "request"]
        );
        assert_eq!(sent["request"], message);
        assert_eq!(sent["instance_id"], endpoint["instance_id"]);
        assert_eq!(sent["protocol"], 1);
        let nonce = sent["nonce"].as_str().unwrap();
        assert_eq!(nonce.len(), 32);
        assert!(nonce
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        assert_eq!(
            sent["auth"],
            mac(
                endpoint["token"].as_str().unwrap(),
                "request",
                &sent["instance_id"],
                &sent["nonce"],
                &sent["request"]
            )
        );
        assert_redacted(&line(&sent), &endpoint);
        assert_error(
            &result.expect("server failure is recoverable Value"),
            json!(55),
            "operation_failed",
        );
        assert_eq!(
            fs::read_dir(&fixture.0.join("agent")).unwrap().count(),
            1,
            "client writes no lease or other state"
        );
    }
);

bounded_test!(
    client_nonces_are_fresh_and_old_response_nonce_cannot_be_replayed,
    {
        let fixture = acl::Fixture::new();
        let first = Peer::new(&fixture.0, |request| line(&good_response(request)));
        let result = transport::request(
            &fixture.0,
            json!({"request_id": 9, "action": "status"}),
            TIMEOUT,
        );
        let first_sent = first
            .finish()
            .expect("first signed request must reach socket");
        assert!(result.is_ok());
        let old_response = good_response(&first_sent);
        let second = Peer::new(&fixture.0, move |_| line(&old_response));
        let result = transport::request(
            &fixture.0,
            json!({"request_id": 9, "action": "status"}),
            TIMEOUT,
        );
        let second_sent = second
            .finish()
            .expect("second signed request must reach socket");
        assert_ne!(first_sent["nonce"], second_sent["nonce"]);
        assert!(
            result.is_err(),
            "valid old MAC and matching ID/instance cannot authenticate a fresh nonce"
        );
    }
);

bounded_test!(
    client_rejects_malformed_stale_wrong_identity_or_untyped_replies,
    {
        let fixture = acl::Fixture::new();
        for variant in 0..18 {
            let peer = Peer::new(&fixture.0, move |request| {
                let mut response = good_response(request);
                match variant {
                    0 => return b"{broken\n".to_vec(),
                    1 => return serde_json::to_vec(&response).unwrap(),
                    2 => response["protocol"] = json!(2),
                    3 => response["protocol"] = json!("1"),
                    4 => response["instance_id"] = json!("c".repeat(32)),
                    5 => response["response"]["request_id"] = json!(99),
                    6 => response["response"]["request_id"] = json!(true),
                    7 => response["response"]["success"] = json!("true"),
                    8 => {
                        response["response"]
                            .as_object_mut()
                            .unwrap()
                            .remove("success");
                    }
                    9 => response["response"] = json!([]),
                    10 => response = json!([]),
                    11 => response["response"]["token"] = json!(SYNTHETIC_SECRET),
                    12 => {
                        let mut bytes = line(&response);
                        bytes.extend_from_slice(&line(&good_response(request)));
                        return bytes;
                    }
                    13 => response["auth"] = json!("0".repeat(64)),
                    14 => response["nonce"] = json!("e".repeat(32)),
                    15 => {
                        response["auth"] = json!(mac(
                            &"d".repeat(64),
                            "response",
                            &response["instance_id"],
                            &response["nonce"],
                            &response["response"]
                        ))
                    }
                    16 => response["auth"] = request["auth"].clone(),
                    17 => {
                        response["auth"] = json!(mac(
                            &"a".repeat(64),
                            "request",
                            &response["instance_id"],
                            &response["nonce"],
                            &response["response"]
                        ))
                    }
                    _ => unreachable!(),
                }
                if response.is_object() && (variant < 12 || variant == 14) {
                    resign_response(&mut response);
                }
                line(&response)
            });
            let result = transport::request(
                &fixture.0,
                json!({"request_id": 9, "action": "stats"}),
                TIMEOUT,
            );
            let observed = peer.finish();
            assert!(
                observed.is_some(),
                "rejection case {variant} must exercise actual socket read"
            );
            if variant == 12 {
                // One request/connection consumes one reply. Extra queued bytes are never replayed.
                assert!(result.is_ok());
            } else {
                assert!(
                    result.is_err(),
                    "response partition {variant} must fail closed"
                );
            }
        }
    }
);

bounded_test!(
    client_does_not_open_a_second_connection_after_malformed_reply,
    {
        let fixture = acl::Fixture::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        publish(&fixture.0, &fake_descriptor(address));
        let (accepted, acceptance) = mpsc::channel();
        let (finished, finish) = mpsc::channel();
        let peer = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(TIMEOUT)).unwrap();
            let mut bytes = Vec::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_until(b'\n', &mut bytes)
                .unwrap();
            if bytes.is_empty() {
                return false;
            }
            accepted.send(()).unwrap();
            stream.write_all(b"{malformed\n").unwrap();
            drop(stream);
            finish.recv_timeout(Duration::from_secs(10)).unwrap();
            listener.set_nonblocking(true).unwrap();
            matches!(listener.accept(), Ok(_))
        });
        let result = transport::request(
            &fixture.0,
            json!({"request_id": 1, "action": "status"}),
            Duration::from_millis(500),
        );
        let reached = acceptance.try_recv().is_ok();
        if !reached {
            // Declaration-only scaffold never connects. Release our exact fixture
            // listener, then fail the public-path assertion below instead of hanging.
            if let Ok(stream) = TcpStream::connect_timeout(&address, Duration::from_millis(100)) {
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
        let _ = finished.send(());
        let retried = peer.join().unwrap();
        assert!(
            reached,
            "no-retry test must first exercise the real client connection"
        );
        assert!(result.is_err());
        assert!(
            !retried,
            "outcome-unknown transport failure must not open a second connection"
        );
    }
);

bounded_test!(
    exact_4096_byte_private_endpoint_is_accepted_without_rewriting,
    {
        let fixture = acl::Fixture::new();
        let peer = Peer::new(&fixture.0, |request| line(&good_response(request)));
        let mut bytes = serde_json::to_vec(&peer.endpoint).unwrap();
        bytes.resize(4096, b' ');
        secure_files::publish_private_file(&endpoint_path(&fixture.0), &bytes).unwrap();
        let result = transport::request(
            &fixture.0,
            json!({"request_id": 1, "action": "status"}),
            TIMEOUT,
        );
        assert!(
            peer.finish().is_some(),
            "exact endpoint boundary must reach socket"
        );
        assert!(result.is_ok());
        assert_eq!(fs::read(endpoint_path(&fixture.0)).unwrap(), bytes);
    }
);

bounded_test!(
    client_rejects_oversized_response_and_accepts_exact_maximum,
    {
        let fixture = acl::Fixture::new();
        for (unicode, excess) in [(false, 0), (false, 1), (true, 0), (true, 1)] {
            let peer = Peer::new(&fixture.0, move |request| {
                let mut reply = good_response(request);
                reply["response"]["data"] = json!("");
                let overhead = line(&reply).len();
                let bytes = RESPONSE_MAX + excess - overhead;
                reply["response"]["data"] = json!(if unicode {
                    multibyte_text(bytes)
                } else {
                    "x".repeat(bytes)
                });
                resign_response(&mut reply);
                let result = line(&reply);
                assert_eq!(result.len(), RESPONSE_MAX + excess);
                if unicode {
                    assert!(String::from_utf8_lossy(&result).chars().count() < RESPONSE_MAX);
                }
                result
            });
            let result = transport::request(
                &fixture.0,
                json!({"request_id": 9, "action": "stats"}),
                TIMEOUT,
            );
            assert!(
                peer.finish().is_some(),
                "response limit test must reach the peer"
            );
            assert_eq!(result.is_ok(), excess == 0);
        }
    }
);

bounded_test!(
    client_rejects_endpoint_schema_partitions_without_connecting_or_rewriting,
    {
        let fixture = acl::Fixture::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let alias = TcpListener::bind("127.0.0.2:0").unwrap();
        alias.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let alias_address = alias.local_addr().unwrap().to_string();
        let good = fake_descriptor(listener.local_addr().unwrap());
        let mut owned_listeners = vec![listener, alias];
        let mut invalid = vec![Value::Null, json!([]), json!({})];
        for (field, value) in [
            ("protocol", json!(2)),
            ("protocol", json!(true)),
            ("protocol", json!("1")),
            ("address", json!(format!("localhost:{port}"))),
            ("address", json!(alias_address)),
            ("address", json!(format!("0.0.0.0:{port}"))),
            ("address", json!("127.0.0.1:0")),
            ("token", json!("a".repeat(63))),
            ("token", json!("g".repeat(64))),
            ("token", json!("A".repeat(64))),
            ("token", json!(1)),
            ("instance_id", json!("b".repeat(31))),
            ("instance_id", json!("g".repeat(32))),
            ("instance_id", json!("B".repeat(32))),
            ("pid", json!(0)),
            ("pid", json!(-1)),
            ("pid", json!(u64::from(u32::MAX) + 1)),
            ("pid", json!(1.0)),
            ("pid", json!(true)),
        ] {
            let mut value_to_test = good.clone();
            value_to_test[field] = value;
            invalid.push(value_to_test);
        }
        // Invalid addresses still point only at owned loopback canaries. A broken
        // validator must not make a test/mutant contact an external or user service.
        if let Ok(ipv6) = TcpListener::bind("[::1]:0") {
            ipv6.set_nonblocking(true).unwrap();
            let mut endpoint = good.clone();
            endpoint["address"] = json!(ipv6.local_addr().unwrap().to_string());
            invalid.push(endpoint);
            owned_listeners.push(ipv6);
        }
        for field in ["protocol", "address", "token", "instance_id", "pid"] {
            let mut value = good.clone();
            value.as_object_mut().unwrap().remove(field);
            invalid.push(value);
        }
        let mut extra = good.clone();
        extra["extra"] = json!(true);
        invalid.push(extra);
        for endpoint in invalid {
            publish(&fixture.0, &endpoint);
            let before = fs::read(endpoint_path(&fixture.0)).unwrap();
            assert!(transport::request(
                &fixture.0,
                json!({"request_id": 1, "action": "status"}),
                Duration::from_millis(200)
            )
            .is_err());
            assert_eq!(fs::read(endpoint_path(&fixture.0)).unwrap(), before);
            for canary in &owned_listeners {
                assert!(
                    matches!(canary.accept(), Err(error) if error.kind() == io::ErrorKind::WouldBlock),
                    "invalid endpoint must not connect to any owned canary"
                );
            }
        }
    }
);

bounded_test!(
    endpoint_raw_byte_limit_and_native_acl_are_checked_before_socket_use,
    {
        let fixture = acl::Fixture::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let good = fake_descriptor(listener.local_addr().unwrap());
        publish(&fixture.0, &good);
        let mut bytes = serde_json::to_vec(&good).unwrap();
        bytes.resize(4097, b' ');
        secure_files::publish_private_file(&endpoint_path(&fixture.0), &bytes).unwrap();
        assert!(transport::request(
            &fixture.0,
            json!({"request_id": 1, "action": "status"}),
            TIMEOUT
        )
        .is_err());
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == io::ErrorKind::WouldBlock)
        );
        fs::remove_file(endpoint_path(&fixture.0)).unwrap();
        let user = acl::user_sid();
        let mut broad = acl::create_file(
            &endpoint_path(&fixture.0),
            &format!("O:{user}D:P(A;;FA;;;{user})(A;;FA;;;WD)"),
        )
        .unwrap();
        broad
            .write_all(&serde_json::to_vec(&good).unwrap())
            .unwrap();
        drop(broad);
        assert!(transport::request(
            &fixture.0,
            json!({"request_id": 1, "action": "status"}),
            TIMEOUT
        )
        .is_err());
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == io::ErrorKind::WouldBlock)
        );
        assert!(
            acl::security(&File::open(endpoint_path(&fixture.0)).unwrap())
                .aces
                .iter()
                .any(|(_, _, _, sid)| sid == "S-1-1-0"),
            "client must not repair broad ACLs"
        );
    }
);

bounded_test!(
    absent_broker_client_failure_never_autostarts_or_creates_client_state,
    {
        let fixture = acl::Fixture::new();
        let before: Vec<_> = fs::read_dir(&fixture.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert!(transport::request(
            &fixture.0,
            json!({"request_id": 3, "action": "status"}),
            Duration::from_millis(100)
        )
        .is_err());
        let after: Vec<_> = fs::read_dir(&fixture.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(after, before);
    }
);

#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    wake: Condvar,
}
impl Gate {
    fn wait(&self) {
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.wake.wait(open).unwrap();
        }
    }
    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.wake.notify_all();
    }
}
struct Release(Arc<Gate>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}

bounded_test!(
    four_admitted_workers_bound_fifth_connection_without_invoking_handler,
    {
        let fixture = acl::Fixture::new();
        let gate = Arc::new(Gate::default());
        let hold = gate.clone();
        let (admitted, seen) = mpsc::channel();
        let server = transport::AgentServer::start(
            &fixture.0,
            Arc::new(move |message| {
                admitted
                    .send(message["request_id"].as_u64().unwrap())
                    .unwrap();
                hold.wait();
                json!({"success": true})
            }),
        )
        .unwrap();
        let _release = Release(gate.clone());
        let endpoint = descriptor(&fixture.0);
        let mut clients = Vec::new();
        for id in 1..=4 {
            let mut client = connect(&endpoint);
            client
                .write_all(&line(&frame(
                    &endpoint,
                    json!({"request_id": id, "action": "stats"}),
                )))
                .unwrap();
            assert_eq!(seen.recv_timeout(TIMEOUT).unwrap(), id);
            clients.push(client);
        }
        let fifth = exchange_bytes(
            &endpoint,
            &line(&frame(
                &endpoint,
                json!({"request_id": 5, "action": "stats"}),
            )),
        );
        assert!(
            fifth.is_ok(),
            "full admission must reject or close promptly, rather than dispatch and wait"
        );
        let fifth = fifth.unwrap();
        if !fifth.is_empty() {
            assert_error(
                &serde_json::from_slice::<Value>(&fifth).unwrap()["response"],
                json!(5),
                "busy",
            );
        }
        assert!(matches!(seen.try_recv(), Err(mpsc::TryRecvError::Empty)));
        gate.release();
        for client in clients {
            let mut bytes = Vec::new();
            BufReader::new(client)
                .read_until(b'\n', &mut bytes)
                .unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&bytes).unwrap()["response"]["success"],
                true
            );
        }
        drop(server);
    }
);

bounded_test!(
    client_deadline_does_not_cancel_admitted_work_and_drop_drains_before_releasing_lease,
    {
        let fixture = acl::Fixture::new();
        let gate = Arc::new(Gate::default());
        let hold = gate.clone();
        let (admitted, arrived) = mpsc::channel();
        let completed = Arc::new(AtomicUsize::new(0));
        let done = completed.clone();
        let server = transport::AgentServer::start(
            &fixture.0,
            Arc::new(move |_| {
                admitted.send(()).unwrap();
                hold.wait();
                done.fetch_add(1, Ordering::SeqCst);
                json!({"success": true})
            }),
        )
        .unwrap();
        let _release = Release(gate.clone());
        let endpoint = descriptor(&fixture.0);
        let data = fixture.0.clone();
        let (returned, client_result) = mpsc::channel();
        let client = thread::spawn(move || {
            let result = transport::request(
                &data,
                json!({"request_id": 1, "action": "stats"}),
                Duration::from_millis(150),
            );
            let _ = returned.send(result);
        });
        arrived
            .recv_timeout(TIMEOUT)
            .expect("dispatch admission barrier");
        let early_result = client_result.recv_timeout(Duration::from_millis(800));
        if early_result.is_err() {
            // Missing client deadline must fail an assertion after releasing the
            // actual admitted callback and reaping the exact request worker.
            gate.release();
            drop(server);
            let settled = client_result.recv_timeout(TIMEOUT);
            if settled.is_ok() {
                client.join().unwrap();
            }
            assert!(false, "client did not finish its 150ms deadline before watchdog; callback released and server cleaned");
            return;
        }
        client.join().unwrap();
        assert!(early_result.unwrap().is_err());
        assert_eq!(completed.load(Ordering::SeqCst), 0);
        let (dropped, dropped_result) = mpsc::channel();
        let dropper = thread::spawn(move || {
            drop(server);
            dropped.send(()).unwrap();
        });
        let address: SocketAddr = endpoint["address"].as_str().unwrap().parse().unwrap();
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if TcpStream::connect_timeout(&address, Duration::from_millis(50)).is_err() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Drop must close listener before waiting for controller"
            );
            thread::yield_now();
        }
        assert!(
            matches!(
                dropped_result.recv_timeout(Duration::from_millis(100)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "Drop returned before admitted dispatch settled"
        );
        assert!(endpoint_path(&fixture.0).exists());
        let lock =
            secure_files::open_private_file(&fixture.0.join("agent/host.lock"), true).unwrap();
        assert!(lock.try_lock().is_err());
        drop(lock);
        gate.release();
        dropped_result
            .recv_timeout(TIMEOUT)
            .expect("drained Drop must finish");
        dropper.join().unwrap();
        assert_eq!(completed.load(Ordering::SeqCst), 1);
        assert!(!endpoint_path(&fixture.0).exists());
        let lock =
            secure_files::open_private_file(&fixture.0.join("agent/host.lock"), true).unwrap();
        lock.try_lock()
            .expect("Drop releases lease after its workers and descriptor");
    }
);

bounded_test!(
    absolute_ingress_deadline_closes_slow_partial_header_without_dispatch,
    {
        let fixture = acl::Fixture::new();
        let (server, calls) = start_counted(&fixture.0);
        let endpoint = descriptor(&fixture.0);
        let mut client = connect(&endpoint);
        let started = Instant::now();
        client.write_all(b"{").unwrap();
        let (stop, timer) = mpsc::channel();
        let mut writer = client.try_clone().unwrap();
        let drip = thread::spawn(move || {
            // Timer ticks are the adversarial input for the absolute deadline test;
            // admission/order in concurrency tests uses channels and controller gates.
            while timer.recv_timeout(Duration::from_millis(300)).is_err() {
                if writer.write_all(b" ").is_err() {
                    break;
                }
            }
        });
        let mut received = Vec::new();
        let result = client.read_to_end(&mut received);
        let elapsed = started.elapsed();
        let _ = stop.send(());
        drip.join().unwrap();
        let _ = client.shutdown(Shutdown::Both);
        drop(client);
        assert!(
            result.is_ok()
                || matches!(
                    result.as_ref().unwrap_err().kind(),
                    io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted
                )
        );
        assert!(
            elapsed < Duration::from_millis(3200),
            "per-read timeout reset would admit indefinite slowloris: {elapsed:?}"
        );
        assert!(calls.lock().unwrap().is_empty());
        assert_redacted(&received, &endpoint);
        drop(server);
    }
);

bounded_test!(
    client_total_deadline_is_not_reset_by_partial_response_reads,
    {
        let fixture = acl::Fixture::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = fake_descriptor(listener.local_addr().unwrap());
        publish(&fixture.0, &endpoint);
        let (stop, timer) = mpsc::channel();
        let address = listener.local_addr().unwrap();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(TIMEOUT)).unwrap();
            stream.set_write_timeout(Some(TIMEOUT)).unwrap();
            let mut request = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            if request.is_empty() {
                return false;
            }
            let _ = stream.write_all(b"{");
            let watchdog = Instant::now() + Duration::from_millis(1000);
            while Instant::now() < watchdog
                && timer.recv_timeout(Duration::from_millis(40)).is_err()
            {
                if stream.write_all(b" ").is_err() {
                    break;
                }
            }
            true
        });
        let started = Instant::now();
        let data = fixture.0.clone();
        let (returned, response) = mpsc::channel();
        let client = thread::spawn(move || {
            let result = transport::request(
                &data,
                json!({"request_id": 1, "action": "stats"}),
                Duration::from_millis(200),
            );
            let _ = returned.send((result, started.elapsed()));
        });
        let early = response.recv_timeout(Duration::from_millis(900));
        let _ = stop.send(());
        if let Ok(stream) = TcpStream::connect_timeout(&address, Duration::from_millis(100)) {
            let _ = stream.shutdown(Shutdown::Both);
        }
        let exercised = worker.join().unwrap();
        let on_time = early.is_ok();
        let result = early.or_else(|_| response.recv_timeout(TIMEOUT));
        if result.is_ok() {
            client.join().unwrap();
        }
        assert!(on_time, "partial-response watchdog stopped the peer before joining client; absolute deadline was missing/reset");
        let (result, elapsed) = result.expect("client must settle after owned peer closes");
        assert!(
            exercised,
            "client deadline must be measured on actual TCP reads"
        );
        assert!(result.is_err());
        assert!(
            elapsed < Duration::from_millis(800),
            "partial data must not reset total deadline"
        );
    }
);

bounded_test!(
    egress_partial_writes_keep_deadline_and_slots_until_expiry,
    {
        #[derive(Clone, Copy, Debug)]
        struct ObservedWrite {
            peer: SocketAddr,
            sequence: usize,
            deadline: Instant,
            at: Instant,
            written: usize,
            timeout: Result<Option<Duration>, io::ErrorKind>,
        }
        const CHUNK: usize = 1024;
        let fixture = acl::Fixture::new();
        let dispatch_gate = Arc::new(Gate::default());
        let first_write_gate = Arc::new(Gate::default());
        let second_write_gate = Arc::new(Gate::default());
        let held_dispatch = dispatch_gate.clone();
        let held_first = first_write_gate.clone();
        let held_second = second_write_gate.clone();
        let payload_length = Arc::new(AtomicUsize::new(0));
        let size = payload_length.clone();
        let counts = Arc::new(Mutex::new(
            std::collections::BTreeMap::<SocketAddr, usize>::new(),
        ));
        let observed_counts = counts.clone();
        let (admitted, admission) = mpsc::channel();
        let (completed, completion) = mpsc::channel();
        let (observed, observation) = mpsc::channel();
        let (sixth_admitted, sixth_admission) = mpsc::channel();
        let server = transport::AgentServer::start_observed_writes(
            &fixture.0,
            Arc::new(move |message| {
                let id = message["request_id"].as_u64().unwrap();
                if id <= 4 {
                    admitted.send(id).unwrap();
                    held_dispatch.wait();
                    let response =
                        json!({"success": true, "data": "x".repeat(size.load(Ordering::SeqCst))});
                    completed.send((id, Instant::now())).unwrap();
                    response
                } else {
                    if id == 6 {
                        let _ = sixth_admitted.send(Instant::now());
                    }
                    json!({"success": true})
                }
            }),
            CHUNK,
            Arc::new(move |stream, deadline, written| {
                let peer = stream.peer_addr().unwrap();
                let sequence = {
                    let mut counts = observed_counts.lock().unwrap();
                    let Some(count) = counts.get_mut(&peer) else {
                        return;
                    };
                    *count += 1;
                    *count
                };
                let event = ObservedWrite {
                    peer,
                    sequence,
                    deadline,
                    at: Instant::now(),
                    written,
                    timeout: stream.write_timeout().map_err(|error| error.kind()),
                };
                // All observations follow actual successful socket writes. Bound the
                // trace even if a mutant keeps writing the complete 4MiB response.
                if sequence <= 3 {
                    let _ = observed.send(event);
                }
                if sequence == 1 {
                    held_first.wait();
                }
                if sequence == 2 {
                    held_second.wait();
                }
            }),
        )
        .expect("actual TCP write scheduling seam must share the native server core");
        let _release_dispatch = Release(dispatch_gate.clone());
        let _release_first = Release(first_write_gate.clone());
        let _release_second = Release(second_write_gate.clone());
        let endpoint = descriptor(&fixture.0);
        let empty = signed(
            &endpoint,
            "response",
            json!("c".repeat(32)),
            json!({"success": true, "request_id": 1, "data": ""}),
        );
        let expected_frame_bytes = RESPONSE_MAX - 64;
        payload_length.store(expected_frame_bytes - line(&empty).len(), Ordering::SeqCst);
        let mut peers = Vec::new();
        for id in 1..=4 {
            let mut peer = connect(&endpoint);
            let address = peer.local_addr().unwrap();
            counts.lock().unwrap().insert(address, 0);
            peer.write_all(&line(&frame(
                &endpoint,
                json!({"request_id": id, "action": "stats"}),
            )))
            .unwrap();
            assert_eq!(admission.recv_timeout(TIMEOUT).unwrap(), id);
            peers.push((id, address, peer, Vec::<u8>::new()));
        }
        // This timer is deliberate long controller work; admission ordering above
        // and every write transition below use actual native callbacks/barriers.
        let (_clock, ticks) = mpsc::channel::<()>();
        assert!(matches!(
            ticks.recv_timeout(Duration::from_millis(2200)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        dispatch_gate.release();
        let mut finished = std::collections::BTreeMap::new();
        for _ in 0..4 {
            let (id, at) = completion.recv_timeout(TIMEOUT).unwrap();
            finished.insert(id, at);
        }
        let mut first = std::collections::BTreeMap::new();
        for _ in 0..4 {
            let event: ObservedWrite = observation.recv_timeout(TIMEOUT).unwrap();
            assert_eq!(event.sequence, 1);
            assert_eq!(
                event.written, CHUNK,
                "observe the actual first native write length"
            );
            assert!(first.insert(event.peer, event).is_none());
        }
        for (id, address, peer, received) in &mut peers {
            let event = first[address];
            assert!(
                event.deadline.saturating_duration_since(finished[id])
                    >= Duration::from_millis(1900),
                "egress deadline must start after completed dispatch, not accept"
            );
            let timeout = event
                .timeout
                .expect("query actual first socket timeout")
                .expect("first write must have a socket deadline");
            assert!(timeout > Duration::ZERO && timeout <= Duration::from_secs(2));
            let available = event.deadline.saturating_duration_since(event.at);
            assert!(
                timeout.abs_diff(available) <= Duration::from_millis(250),
                "first native timeout must reflect its absolute remaining budget"
            );
            let mut prefix = vec![0u8; event.written];
            peer.read_exact(&mut prefix).unwrap();
            let text = String::from_utf8_lossy(&prefix);
            assert!(
                text.contains("data") && text.contains(&"x".repeat(100)),
                "actual payload prefix must reach the owned TCP peer"
            );
            received.extend_from_slice(&prefix);
        }
        // Four genuine first-write callbacks retain the real WorkerSlots. Kernel
        // buffering cannot make these scheduled native writers finish early.
        let busy = exchange_bytes(
            &endpoint,
            &line(&frame(
                &endpoint,
                json!({"request_id": 5, "action": "stats"}),
            )),
        )
        .unwrap();
        if !busy.is_empty() {
            assert_error(
                &serde_json::from_slice::<Value>(&busy).unwrap()["response"],
                json!(5),
                "busy",
            );
        }
        let original_latest = first.values().map(|event| event.deadline).max().unwrap();
        let phase_one_release = original_latest - Duration::from_millis(800);
        if let Some(delay) = phase_one_release.checked_duration_since(Instant::now()) {
            assert!(matches!(
                ticks.recv_timeout(delay),
                Err(mpsc::RecvTimeoutError::Timeout)
            ));
        }
        first_write_gate.release();
        let mut second = std::collections::BTreeMap::new();
        for _ in 0..4 {
            let event: ObservedWrite = observation.recv_timeout(TIMEOUT).unwrap();
            assert_eq!(event.sequence, 2);
            assert_eq!(
                event.written, CHUNK,
                "observe the actual second native write length"
            );
            assert!(second.insert(event.peer, event).is_none());
        }
        for (_, address, peer, received) in &mut peers {
            let event = second[address];
            assert_eq!(
                event.deadline, first[address].deadline,
                "partial write must keep the original absolute egress deadline"
            );
            let timeout = event
                .timeout
                .expect("query actual second socket timeout")
                .expect("second write must have a socket deadline");
            assert!(
                timeout > Duration::ZERO && timeout < Duration::from_millis(1100),
                "native socket timeout must shrink after the scheduled partial write: {timeout:?}"
            );
            let available = event.deadline.saturating_duration_since(event.at);
            assert!(
                timeout.abs_diff(available) <= Duration::from_millis(250),
                "second native timeout must reflect its reduced remaining budget"
            );
            let mut bytes = vec![0u8; event.written];
            peer.read_exact(&mut bytes).unwrap();
            received.extend_from_slice(&bytes);
        }
        let release_after_expiry = original_latest + Duration::from_millis(100);
        if let Some(delay) = release_after_expiry.checked_duration_since(Instant::now()) {
            assert!(matches!(
                ticks.recv_timeout(delay),
                Err(mpsc::RecvTimeoutError::Timeout)
            ));
        }
        second_write_gate.release();
        let admission_limit = original_latest + Duration::from_millis(700);
        let mut eof = Vec::new();
        for (_, _, peer, _) in &mut peers {
            let remaining = admission_limit
                .saturating_duration_since(Instant::now())
                .max(Duration::from_millis(1));
            peer.set_read_timeout(Some(remaining.min(Duration::from_millis(500))))
                .unwrap();
            let mut extra = [0u8; 1];
            eof.push(peer.read(&mut extra));
        }
        // This is explicit bounded readiness driving with a read-only status action,
        // not an automatic retry inside the production request() implementation.
        let mut sixth_succeeded = false;
        for _ in 0..64 {
            let Some(remaining) = admission_limit.checked_duration_since(Instant::now()) else {
                break;
            };
            if transport::request(
                &fixture.0,
                json!({"request_id": 6, "action": "status"}),
                remaining,
            )
            .is_ok_and(|reply| reply["success"] == true)
            {
                sixth_succeeded = true;
                break;
            }
            thread::yield_now();
        }
        let sixth_at = sixth_admission.try_recv();
        // Shutdown all owned peers before any post-schedule failure assertion, then
        // drain the real server while every callback gate is already released.
        for (_, _, peer, _) in &peers {
            let _ = peer.shutdown(Shutdown::Both);
        }
        let actual_counts = counts.lock().unwrap().clone();
        let captured: Vec<_> = peers.iter().map(|(_, _, _, bytes)| bytes.clone()).collect();
        drop(peers);
        drop(server);
        assert!(eof.iter().all(|result| matches!(result, Ok(0))), "expiry must close each partial response; no later bytes or queued-open timeout: {eof:?}");
        assert!(
            actual_counts.values().all(|count| *count == 2),
            "no third native write may occur after the original deadline: {actual_counts:?}"
        );
        assert!(
            sixth_succeeded,
            "expired writes must release slots by the original 2.7s admission bound"
        );
        assert!(
            sixth_at.is_ok_and(|at| at <= admission_limit),
            "sixth actual handler admission must meet the original deadline bound"
        );
        for bytes in captured {
            assert_eq!(bytes.len(), 2 * CHUNK);
            assert!(bytes.len() < expected_frame_bytes);
            assert!(
                !bytes.ends_with(b"\n") && serde_json::from_slice::<Value>(&bytes).is_err(),
                "owned peer observes a bounded incomplete JSONL reply"
            );
        }
    }
);

bounded_test!(
    cleanup_preserves_replaced_descriptor_unrelated_files_and_stable_lock_identity,
    {
        let fixture = acl::Fixture::new();
        let (server, _) = start_counted(&fixture.0);
        let initial = descriptor(&fixture.0);
        let lock = File::open(fixture.0.join("agent/host.lock")).unwrap();
        let lock_identity = acl::file_identity(&lock);
        drop(lock);
        let neighbor = fixture.0.join("agent/unrelated.json");
        fs::write(&neighbor, b"preserve-neighbor").unwrap();
        let mut changed = initial.clone();
        changed["instance_id"] = json!("e".repeat(32));
        publish(&fixture.0, &changed);
        let before = fs::read(endpoint_path(&fixture.0)).unwrap();
        drop(server);
        assert_eq!(fs::read(endpoint_path(&fixture.0)).unwrap(), before);
        assert_eq!(fs::read(&neighbor).unwrap(), b"preserve-neighbor");
        let (successor, _) = start_counted(&fixture.0);
        let next = descriptor(&fixture.0);
        assert_ne!(next["instance_id"], initial["instance_id"]);
        assert_ne!(next["token"], initial["token"]);
        assert_eq!(
            acl::file_identity(&File::open(fixture.0.join("agent/host.lock")).unwrap()),
            lock_identity
        );
        drop(successor);
        assert!(!endpoint_path(&fixture.0).exists());
        assert_eq!(fs::read(neighbor).unwrap(), b"preserve-neighbor");
    }
);

struct OwnedChild(Child);
impl OwnedChild {
    fn wait_bounded(&mut self, timeout: Duration) -> (ExitStatus, bool) {
        use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows_sys::Win32::System::Threading::WaitForSingleObject;
        let wait = unsafe {
            WaitForSingleObject(
                self.0.as_raw_handle(),
                timeout.as_millis().try_into().unwrap(),
            )
        };
        assert!(
            matches!(wait, WAIT_OBJECT_0 | WAIT_TIMEOUT),
            "owned process wait failed: {}",
            io::Error::last_os_error()
        );
        let timed_out = wait == WAIT_TIMEOUT;
        if timed_out {
            self.0.kill().expect("terminate exact owned watchdog child");
        }
        self.0.stdin.take();
        (self.0.wait().unwrap(), timed_out)
    }
    fn terminate(&mut self) {
        self.0.stdin.take();
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        self.terminate();
    }
}

struct HostProcess {
    child: OwnedChild,
    reader: Option<thread::JoinHandle<()>>,
}
impl Drop for HostProcess {
    fn drop(&mut self) {
        self.child.terminate();
        if let Some(reader) = self.reader.take() {
            reader.join().unwrap();
        }
    }
}

#[test]
fn watchdog_kills_only_owned_stalled_probe_and_drains_its_pipes() {
    let fixture = acl::Fixture::new();
    let mut command = Command::new(PROBE);
    command.arg("watchdog-stall").arg(&fixture.0);
    let output = bounded_output(&mut command, Duration::from_millis(150));
    assert!(output.timed_out);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("watchdog-ready"));
    assert!(output.stderr.is_empty());
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}

#[test]
fn real_process_second_host_is_refused_and_stable_lease_path_cannot_be_replaced() {
    let fixture = acl::Fixture::new();
    let mut child = OwnedChild(
        Command::new(PROBE)
            .arg("host")
            .arg(&fixture.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let (send, receive) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if send.send(line).is_err() {
                break;
            }
        }
    });
    let mut host = HostProcess {
        child,
        reader: Some(reader),
    };
    let ready = receive
        .recv_timeout(TIMEOUT)
        .expect("real native host readiness")
        .unwrap();
    assert_eq!(ready, "ready");
    let endpoint = fs::read(endpoint_path(&fixture.0)).unwrap();
    let second = bounded_output(Command::new(PROBE).arg("try-host").arg(&fixture.0), TIMEOUT);
    assert!(
        !second.timed_out,
        "second host did not fail fast; owned process was terminated/reaped"
    );
    assert!(second.status.success());
    assert_eq!(String::from_utf8(second.stdout).unwrap().trim(), "blocked");
    assert_eq!(fs::read(endpoint_path(&fixture.0)).unwrap(), endpoint);
    assert_eq!(
        fs::rename(
            fixture.0.join("agent/host.lock"),
            fixture.0.join("replaced.lock")
        )
        .unwrap_err()
        .raw_os_error(),
        Some(32)
    );
    host.child
        .0
        .stdin
        .take()
        .unwrap()
        .write_all(b"stop\n")
        .unwrap();
    assert_eq!(receive.recv_timeout(TIMEOUT).unwrap().unwrap(), "stopped");
    let (status, timed_out) = host.child.wait_bounded(TIMEOUT);
    assert!(
        !timed_out,
        "host did not exit after stop; owned process was terminated/reaped"
    );
    assert!(status.success());
    host.reader.take().unwrap().join().unwrap();
    let successor = bounded_output(Command::new(PROBE).arg("try-host").arg(&fixture.0), TIMEOUT);
    assert!(
        !successor.timed_out,
        "successor host did not settle; owned process was terminated/reaped"
    );
    assert!(successor.status.success());
    assert_eq!(
        String::from_utf8(successor.stdout).unwrap().trim(),
        "acquired"
    );
}

#[derive(Default)]
struct Recording {
    bytes: Vec<u8>,
    flushed: Vec<usize>,
}
struct Output(Arc<Mutex<Recording>>);
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        let mut record = self.0.lock().unwrap();
        let len = record.bytes.len();
        record.flushed.push(len);
        Ok(())
    }
}
fn replies(record: &Arc<Mutex<Recording>>) -> Vec<Value> {
    let record = record.lock().unwrap();
    let values: Vec<_> = record
        .bytes
        .split(|byte| *byte == b'\n')
        .filter(|bytes| !bytes.is_empty())
        .map(|bytes| serde_json::from_slice(bytes).unwrap())
        .collect();
    for end in record
        .bytes
        .iter()
        .enumerate()
        .filter_map(|(index, byte)| (*byte == b'\n').then_some(index + 1))
    {
        assert!(
            record.flushed.iter().any(|flushed| *flushed >= end),
            "every reply must be flushed"
        );
    }
    values
}

struct GuardedInput {
    bytes: Vec<u8>,
    position: usize,
    allowed: usize,
    output: Arc<Mutex<Recording>>,
}
fn startup() -> Value {
    json!({"success": true, "protocol": 1, "surface": "desktop-client",
        "agent_protocol": "LIGHT_WHISPER_AGENT_PROTOCOL_1"})
}
impl Read for GuardedInput {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let count = available.len().min(output.len());
        output[..count].copy_from_slice(&available[..count]);
        self.consume(count);
        Ok(count)
    }
}
impl BufRead for GuardedInput {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        {
            let recorded = self.output.lock().unwrap();
            assert_eq!(
                recorded.flushed.last().copied(),
                Some(recorded.bytes.len()),
                "each reply must flush before reading the next input record"
            );
        }
        assert_eq!(
            replies(&self.output).first(),
            Some(&startup()),
            "startup readiness must flush before first stdin read"
        );
        assert!(
            self.position < self.allowed,
            "queued record read after terminal exit/fatal reply"
        );
        let end = self.bytes[self.position..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(self.bytes.len(), |end| self.position + end + 1);
        Ok(&self.bytes[self.position..end])
    }
    fn consume(&mut self, count: usize) {
        self.position += count;
    }
}

bounded_test!(
    broker_startup_is_flushed_without_any_endpoint_or_controller_probe_and_eof_has_no_reply,
    {
        let fixture = acl::Fixture::new();
        let recorded = Arc::new(Mutex::new(Recording::default()));
        let mut output = Output(recorded.clone());
        let mut input = GuardedInput {
            bytes: vec![],
            position: 0,
            allowed: 1,
            output: recorded.clone(),
        };
        transport::run_client(&fixture.0, &mut input, &mut output, TIMEOUT)
            .expect("EOF success needs no app connection");
        assert_eq!(replies(&recorded), [startup()]);
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
    }
);

bounded_test!(
    broker_exit_ack_is_local_terminal_and_does_not_read_queued_input,
    {
        let fixture = acl::Fixture::new();
        let (server, calls) = start_counted(&fixture.0);
        let recorded = Arc::new(Mutex::new(Recording::default()));
        let first = line(&json!({"request_id": 31, "action": "exit"}));
        let mut bytes = first.clone();
        bytes.extend_from_slice(&line(&json!({"request_id": 32, "action": "status"})));
        let mut input = GuardedInput {
            bytes,
            position: 0,
            allowed: first.len(),
            output: recorded.clone(),
        };
        transport::run_client(
            &fixture.0,
            &mut input,
            &mut Output(recorded.clone()),
            TIMEOUT,
        )
        .unwrap();
        assert_eq!(
            replies(&recorded),
            [startup(), json!({"success": true, "request_id": 31})]
        );
        assert!(
            calls.lock().unwrap().is_empty(),
            "exit must never dispatch application termination"
        );
        drop(server);
    }
);

bounded_test!(
    broker_bad_input_and_backend_failure_are_recoverable_then_forward_serial_requests,
    {
        let fixture = acl::Fixture::new();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = calls.clone();
        let server = transport::AgentServer::start(
        &fixture.0,
        Arc::new(move |message| {
            observed.lock().unwrap().push(message.clone());
            if message["request_id"] == 6 {
                json!({"success": false, "code": "operation_failed", "error": "synthetic failure"})
            } else {
                json!({"success": true})
            }
        }),
    )
    .unwrap();
        let mut bytes = b"{broken\n\n".to_vec();
        for message in [
            json!({"request_id": 4, "action": "invoke"}),
            json!({"request_id": 5, "action": "transcribe", "audio_path": "", "options": {}}),
            json!({"request_id": 6, "action": "stats"}),
            json!({"request_id": 7, "action": "status"}),
        ] {
            bytes.extend_from_slice(&line(&message));
        }
        let recorded = Arc::new(Mutex::new(Recording::default()));
        let allowed = bytes.len() + 1;
        let mut input = GuardedInput {
            bytes,
            position: 0,
            allowed,
            output: recorded.clone(),
        };
        transport::run_client(
            &fixture.0,
            &mut input,
            &mut Output(recorded.clone()),
            TIMEOUT,
        )
        .unwrap();
        let output = replies(&recorded);
        assert_eq!(
            output.len(),
            6,
            "blank records are ignored and EOF has no padding"
        );
        assert_error(&output[1], Value::Null, "invalid_request");
        assert_error(&output[2], json!(4), "invalid_request");
        assert_error(&output[3], json!(5), "invalid_request");
        assert_error(&output[4], json!(6), "operation_failed");
        assert_eq!(output[5], json!({"success": true, "request_id": 7}));
        assert_eq!(
            *calls.lock().unwrap(),
            [
                json!({"request_id": 6, "action": "stats"}),
                json!({"request_id": 7, "action": "status"})
            ]
        );
        drop(server);
    }
);

bounded_test!(
    broker_transport_failure_flushes_correlated_fatal_then_stops_before_queued_read,
    {
        let fixture = acl::Fixture::new();
        let recorded = Arc::new(Mutex::new(Recording::default()));
        let first = line(&json!({"request_id": 41, "action": "status"}));
        let mut bytes = first.clone();
        bytes.extend_from_slice(&line(&json!({"request_id": 42, "action": "stats"})));
        let mut input = GuardedInput {
            bytes,
            position: 0,
            allowed: first.len(),
            output: recorded.clone(),
        };
        assert!(transport::run_client(
            &fixture.0,
            &mut input,
            &mut Output(recorded.clone()),
            TIMEOUT
        )
        .is_err());
        let output = replies(&recorded);
        assert_eq!(output.len(), 2);
        assert_error(&output[1], json!(41), "transport_error");
        assert_eq!(output[1]["fatal"], true);
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
    }
);

bounded_test!(
    broker_oversized_record_is_recoverable_without_server_effect,
    {
        let fixture = acl::Fixture::new();
        let (server, calls) = start_counted(&fixture.0);
        let oversized = json!({"request_id": 61, "action": "transcribe", "audio_path": "synthetic.wav",
        "options": {"context": "x".repeat(REQUEST_MAX)}});
        let mut bytes = line(&oversized);
        bytes.extend_from_slice(&line(&json!({"request_id": 62, "action": "exit"})));
        let recorded = Arc::new(Mutex::new(Recording::default()));
        transport::run_client(
            &fixture.0,
            &mut Cursor::new(bytes),
            &mut Output(recorded.clone()),
            TIMEOUT,
        )
        .unwrap();
        let output = replies(&recorded);
        assert_eq!(output.len(), 3);
        assert_eq!(output[1]["success"], false);
        assert_eq!(output[1]["code"], "invalid_request");
        assert_eq!(output[2], json!({"success": true, "request_id": 62}));
        assert!(calls.lock().unwrap().is_empty());
        drop(server);
    }
);

bounded_test!(
    broker_startup_output_error_propagates_before_any_input_or_external_effect,
    {
        struct RefuseWrite;
        impl Write for RefuseWrite {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
        }
        struct NoInput;
        impl Read for NoInput {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                panic!("input read after failed startup output")
            }
        }
        impl BufRead for NoInput {
            fn fill_buf(&mut self) -> io::Result<&[u8]> {
                panic!("input read after failed startup output")
            }
            fn consume(&mut self, _: usize) {
                panic!("input consumed after failed startup output")
            }
        }
        let fixture = acl::Fixture::new();
        let error =
            transport::run_client(&fixture.0, &mut NoInput, &mut RefuseWrite, TIMEOUT).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
    }
);

bounded_test!(
    four_unauthenticated_ingress_slots_close_fifth_without_dispatch,
    {
        let fixture = acl::Fixture::new();
        let gate = Arc::new(Gate::default());
        let blocked = gate.clone();
        let reserved = Arc::new(AtomicUsize::new(0));
        let reservations = reserved.clone();
        let (observed, observation) = mpsc::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let dispatches = calls.clone();
        let server = transport::AgentServer::start_observed(
            &fixture.0,
            Arc::new(move |_| {
                dispatches.fetch_add(1, Ordering::SeqCst);
                json!({"success": true})
            }),
            Arc::new(move || {
                let count = reservations.fetch_add(1, Ordering::SeqCst) + 1;
                let _ = observed.send(count);
                blocked.wait();
            }),
        )
        .expect("observed start shares actual production ingress reservation");
        let _release = Release(gate.clone());
        let endpoint = descriptor(&fixture.0);
        let mut partial = Vec::new();
        for count in 1..=4 {
            let mut peer = connect(&endpoint);
            peer.write_all(b"{").unwrap();
            assert_eq!(
                observation.recv_timeout(TIMEOUT).unwrap(),
                count,
                "actual retained worker slot must be observed before opening the next peer"
            );
            partial.push(peer);
        }
        let mut fifth = connect(&endpoint);
        fifth
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        fifth
            .write_all(&line(&frame(
                &endpoint,
                json!({"request_id": 5, "action": "status"}),
            )))
            .unwrap();
        let mut rejection = Vec::new();
        let closed = fifth.read_to_end(&mut rejection);
        // Close every owned socket and release barriers before asserting a missing
        // guard's fifth-observer or queued-open failure. No accept queue is counted.
        let _ = fifth.shutdown(Shutdown::Both);
        for peer in &partial {
            let _ = peer.shutdown(Shutdown::Both);
        }
        drop(fifth);
        drop(partial);
        gate.release();
        drop(server);
        assert_eq!(
            reserved.load(Ordering::SeqCst),
            4,
            "fifth unauthenticated worker must never reserve a slot"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "pre-auth saturation must never invoke the controller"
        );
        assert!(closed.is_ok() || matches!(closed.as_ref().unwrap_err().kind(),
        io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted),
        "fifth peer must observe EOF/close while all four observed ingress slots are retained; timeout/queued-open is not rejection");
        assert_redacted(&rejection, &endpoint);
    }
);

bounded_test!(
    broker_signed_envelope_overflow_is_recoverable_without_dispatch,
    {
        let fixture = acl::Fixture::new();
        let (server, calls) = start_counted(&fixture.0);
        let endpoint = descriptor(&fixture.0);
        let mut oversized_on_wire = json!({"request_id": 71, "action": "transcribe",
        "audio_path": "synthetic.wav", "options": {"context": ""}});
        let body_overhead = line(&oversized_on_wire).len();
        oversized_on_wire["options"]["context"] = json!("x".repeat(REQUEST_MAX - body_overhead));
        let first = line(&oversized_on_wire);
        assert_eq!(
            first.len(),
            REQUEST_MAX,
            "stdin body is schema-valid and exactly within its byte cap"
        );
        assert!(
            line(&frame(&endpoint, oversized_on_wire)).len() > REQUEST_MAX,
            "only the authenticated network envelope exceeds its byte cap"
        );
        let mut bytes = first;
        bytes.extend_from_slice(&line(&json!({"request_id": 72, "action": "status"})));
        bytes.extend_from_slice(&line(&json!({"request_id": 73, "action": "exit"})));
        let allowed = bytes.len();
        bytes.extend_from_slice(&line(&json!({"request_id": 74, "action": "stats"})));
        let recorded = Arc::new(Mutex::new(Recording::default()));
        let mut input = GuardedInput {
            bytes,
            position: 0,
            allowed,
            output: recorded.clone(),
        };
        let result = transport::run_client(
            &fixture.0,
            &mut input,
            &mut Output(recorded.clone()),
            TIMEOUT,
        );
        let output = replies(&recorded);
        let actual_calls = calls.lock().unwrap().clone();
        drop(server);
        assert_error(&output[1], json!(71), "invalid_request");
        assert_ne!(
            output[1].get("fatal"),
            Some(&Value::Bool(true)),
            "local frame rejection must remain recoverable"
        );
        assert!(
            result.is_ok(),
            "broker must continue after local encoded-envelope rejection"
        );
        assert_eq!(
            output,
            [
                startup(),
                output[1].clone(),
                json!({"success": true, "request_id": 72, "observation": "synthetic-controller"}),
                json!({"success": true, "request_id": 73})
            ]
        );
        assert_eq!(
            actual_calls,
            [json!({"request_id": 72, "action": "status"})],
            "oversized wire payload and local exit must never dispatch"
        );
    }
);

bounded_test!(
    malformed_controller_failure_fields_are_sanitized_and_backend_declines_remain_usable,
    {
        let fixture = acl::Fixture::new();
        let mut malformed = Vec::new();
        for field in ["error", "code"] {
            for value in [
                None,
                Some(Value::Null),
                Some(json!(false)),
                Some(json!(7)),
                Some(json!([])),
                Some(json!({})),
                Some(json!("")),
            ] {
                let mut response = json!({"success": false, "request_id": u64::MAX,
                    "code": "backend_declined", "error": SYNTHETIC_SECRET,
                    "data": SYNTHETIC_SECRET});
                match value {
                    Some(value) => response[field] = value,
                    None => {
                        response.as_object_mut().unwrap().remove(field);
                    }
                }
                malformed.push(response);
            }
        }
        for code in [
            "Backend_declined",
            "backend2",
            "backend-declined",
            "backend declined",
            "backend\nfailed",
            "backend_é",
        ] {
            malformed.push(json!({"success": false, "request_id": u64::MAX,
                "code": code, "error": SYNTHETIC_SECRET, "data": SYNTHETIC_SECRET}));
        }
        let malformed_count = malformed.len();
        let decline = json!({"success": false, "request_id": u64::MAX,
            "code": "backend_declined", "error": "synthetic backend declined"});
        malformed.push(decline.clone());
        let outcomes = Arc::new(Mutex::new(malformed.into_iter()));
        let next = outcomes.clone();
        let server = transport::AgentServer::start(
            &fixture.0,
            Arc::new(move |_| next.lock().unwrap().next().unwrap()),
        )
        .unwrap();
        let endpoint = descriptor(&fixture.0);
        for index in 0..malformed_count {
            let id = index as u64;
            let reply = transport::request(
                &fixture.0,
                json!({"request_id": id, "action": "stats"}),
                TIMEOUT,
            )
            .expect("malformed controller failure must yield a sanitized ordinary reply");
            assert_error(&reply, json!(id), "invalid_response");
            assert_eq!(keys(&reply), ["code", "error", "request_id", "success"]);
            assert_redacted(&line(&reply), &endpoint);
        }
        let id = malformed_count as u64;
        let reply = transport::request(
            &fixture.0,
            json!({"request_id": id, "action": "stats"}),
            TIMEOUT,
        )
        .expect("server must remain usable for a valid backend decline");
        let mut expected = decline;
        expected["request_id"] = json!(id);
        assert_eq!(reply, expected);
        assert_redacted(&line(&reply), &endpoint);
        assert!(outcomes.lock().unwrap().next().is_none());
        drop(server);
        assert!(!endpoint_path(&fixture.0).exists());
    }
);
