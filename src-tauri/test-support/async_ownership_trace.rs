
use sha2::{Digest, Sha256};

pub fn load(name: &str) -> Vec<String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let trace: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("formal/traces").join(format!("{name}.json"))).unwrap(),
    )
    .unwrap();
    assert_eq!(trace["generator"], "TLC 1.7.4 counterexample");
    assert_eq!(
        trace["tlc_sha256"],
        "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"
    );
    for kind in ["model", "config"] {
        let bytes = std::fs::read(root.join(trace[kind].as_str().unwrap())).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(bytes)),
            trace[format!("{kind}_sha256")].as_str().unwrap(),
            "stale {kind} provenance in {name}"
        );
    }
    let events: Vec<String> = trace["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event.as_str().unwrap().to_string())
        .collect();
    assert!(!events.is_empty(), "missing generated events");
    let counterexample =
        std::fs::read(root.join(trace["counterexample"].as_str().unwrap())).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&counterexample)),
        trace["counterexample_sha256"].as_str().unwrap(),
        "corrupt TLC counterexample in {name}"
    );
    let text = std::str::from_utf8(&counterexample).unwrap();
    let recorded: Vec<String> = text
        .lines()
        .filter_map(|line| {
            line.strip_prefix("/\\ lastEvent = \"")
                .and_then(|event| event.strip_suffix('"'))
                .filter(|event| *event != "init")
                .map(str::to_string)
        })
        .collect();
    assert_eq!(
        recorded, events,
        "events differ from TLC counterexample in {name}"
    );
    events
}
