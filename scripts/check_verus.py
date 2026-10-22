"""Verify production capture, hotkey ownership and interim interval contracts."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "src-tauri/src/services/audio_service/capture_bounds.rs"
TEMPLATE = ROOT / "formal/verus/capture_bounds.template.rs"
BODY_TOKEN = "/*__CAPTURE_BOUNDS_BODY__*/"
FLAGS = ["--crate-type", "lib", "--target", "x86_64-pc-windows-msvc",
         "--num-threads", "2", "--rlimit", "10", "--no-cheating"]
TIMEOUT_SECONDS = 120
TRUST_ESCAPE = re.compile(r"\b(?:assume|assume_specification|admit|external_body|external|axiom)\b")
SUMMARY = re.compile(r"^verification results::\s*(\d+) verified,\s*(\d+) errors\s*$", re.M)
SEMANTIC_ERROR = re.compile(
    r"^error:.*(?:postcondition|precondition|assertion|overflow|underflow|division by zero|divide by zero)",
    re.M | re.I,
)
INVALID_PROOF = re.compile(
    r"error\[E\d+\]|\bpanic(?:ked)?\b|internal (?:compiler|verifier) error|"
    r"expected (?:expression|item|token)|mismatched types|unresolved (?:import|name)|"
    r"cannot find|unsupported|timed out|resource limit|rlimit|segmentation fault|"
    r"stack overflow|access violation|fatal runtime error",
    re.I,
)
MUTATIONS = (
    ("zero-channel-guard", "channels.max(1)", "channels"),
    ("capacity-limit", "frame_count.min(allowed_mono_samples)", "frame_count"),
    ("unchecked-subtraction", "cap.saturating_sub(used)", "(cap - used)"),
)
HOTKEY_SOURCE = ROOT / "src-tauri/src/commands/hotkey/start_owner.rs"
INTERVAL_SOURCE = ROOT / "src-tauri/src/services/audio_service/interim.rs"
CONSTANT_SOURCE = ROOT / "src-tauri/src/services/audio_service/mod.rs"
INTERVAL_CONSTANTS = {
    "INTERIM_INTERVAL_MIN_MS": 140,
    "INTERIM_INTERVAL_BASE_MS": 220,
    "INTERIM_INTERVAL_MAX_MS": 460,
    "INTERIM_INTERVAL_DOWN_STEP_MS": 24,
    "INTERIM_INTERVAL_UP_STEP_MS": 42,
    "INTERIM_HEAVY_COST_MS": 420,
    "INTERIM_LIGHT_COST_MS": 180,
}
DOMAINS = {
    "capture_bounds": (SOURCE, ("capped_frame_count",)),
    "hotkey_ownership": (HOTKEY_SOURCE, ("begin", "bind", "release", "fail")),
    "interim_interval": (INTERVAL_SOURCE, ("adjust_interval",)),
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def extract_body(source):
    # Deliberately accept one context-free, straight-line Rust function only.
    # New syntax needs an explicit review of the extraction boundary.
    signature = (
        r"\s*pub\(crate\)\s+fn\s+capped_frame_count\s*\(\s*"
        r"input_len\s*:\s*usize\s*,\s*channels\s*:\s*usize\s*,\s*"
        r"used\s*:\s*usize\s*,\s*cap\s*:\s*usize\s*,?\s*\)\s*->\s*usize\s*"
        r"\{(?P<body>[^{}]+)\}\s*"
    )
    match = re.fullmatch(signature, source)
    require(match is not None, "production function signature or surrounding context changed")
    body = match["body"]
    require(re.fullmatch(r"[A-Za-z0-9_\s().;=/]+", body) is not None,
            "production body uses syntax outside the reviewed extraction boundary")
    require(not re.search(r"//|/\*|\b(?:fn|use|mod|const|static|type|struct|enum|trait|impl|"
                          r"extern|unsafe|spec|proof|exec|requires|ensures)\b", body),
            "production body contains comments, items, or verification context")
    return body


def programs(source, template):
    for name, text in (("source", source), ("template", template)):
        require(TRUST_ESCAPE.search(text) is None, f"trust bypass in {name}")
    require(template.count(BODY_TOKEN) == 1, "template must contain exactly one body anchor")
    body = extract_body(source)
    result = {"positive": template.replace(BODY_TOKEN, body)}
    for name, anchor, replacement in MUTATIONS:
        require(body.count(anchor) == 1, f"mutation anchor changed: {name}")
        result[f"negative-{name}"] = template.replace(BODY_TOKEN, body.replace(anchor, replacement, 1))
    require(len(set(result.values())) == 4, "negative controls must be distinct")
    return result


def production_scope(source):
    return source.split("#[cfg(test)]", 1)[0]


def without_comments(source):
    return re.sub(r"//[^\r\n]*|/\*[\s\S]*?\*/", "", source)


def block(source, header, label):
    matches = list(re.finditer(header + r"\s*\{", source, re.M))
    require(len(matches) == 1, f"expected one {label}")
    match = matches[0]
    opening = match.end() - 1
    depth, end = 1, opening + 1
    while end < len(source) and depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    require(depth == 0, f"unbalanced {label}")
    return source[match.start():end], source[opening + 1:end - 1]


def reviewed_body(body):
    require(re.fullmatch(r"[A-Za-z0-9_\s().;,:&|!=<>+\-{}]+", body) is not None,
            "body uses syntax outside the reviewed extraction boundary")
    require(not re.search(r"\b(?:fn|use|mod|const|static|type|struct|enum|trait|impl|extern|"
                          r"unsafe|spec|proof|exec|requires|ensures|loop|while|for)\b|\w+\s*!\s*[({\[]", body),
            "body contains items, macros or verification context")
    return body


def instantiate(template, pieces):
    require(set(re.findall(r"/\*__\w+__\*/", template)) == set(pieces),
            "template anchors differ from the required production inventory")
    for token, value in pieces.items():
        require(template.count(token) == 1, f"template must contain exactly one {token}")
        template = template.replace(token, value)
    return template


def hotkey_programs(source, template):
    # This complete context is deliberately small: two structs, the locking
    # wrapper, and four pure methods. Reject new context rather than guessing.
    structure, _ = block(source, r"^struct\s+StartOwnership", "StartOwnership structure")
    require(re.fullmatch(r"struct\s+StartOwnership\s*\{\s*generation:\s*u64,\s*"
                         r"active:\s*bool,\s*session_id:\s*Option<u64>,\s*\}", structure),
            "hotkey state structure changed")
    implementation, methods = block(source, r"^impl\s+StartOwnership", "StartOwnership implementation")
    wrapper, wrapper_methods = block(source, r"^impl\s+HotkeyStartOwner", "HotkeyStartOwner wrapper")
    context = source.replace(implementation, "", 1).replace(wrapper, "", 1)
    require(re.fullmatch(r"\s*use parking_lot::Mutex;\s*#\[derive\(Default\)\]\s*"
                         r"pub\(crate\) struct HotkeyStartOwner\s*\{\s*inner:\s*Mutex<StartOwnership>,\s*\}\s*"
                         r"#\[derive\(Default\)\]\s*" + re.escape(structure) + r"\s*", context),
            "hotkey source context changed")
    signatures = {
        "begin": r"fn\s+begin\s*\(\s*&mut self\s*\)\s*->\s*Option<u64>",
        "bind": r"fn\s+bind\s*\(\s*&mut self,\s*generation:\s*u64,\s*session_id:\s*u64,?\s*\)\s*->\s*bool",
        "release": r"fn\s+release\s*\(\s*&mut self\s*\)\s*->\s*Option<u64>",
        "fail": r"fn\s+fail\s*\(\s*&mut self,\s*generation:\s*u64,?\s*\)\s*->\s*bool",
    }
    pieces = {"/*__START_OWNERSHIP_STRUCT__*/": structure}
    remaining = methods
    for name, signature in signatures.items():
        method, body = block(methods, r"^\s*" + signature, name)
        pieces[f"/*__{name.upper()}_BODY__*/"] = reviewed_body(body)
        remaining = remaining.replace(method, "", 1)
    require(not remaining.strip(), "unexpected hotkey methods or attributes")
    # Keep the caller link explicit, including holding the guard across expect.
    expected_wrapper = '''
    pub(crate) fn begin(&self) -> u64 {
        let mut inner = self.inner.lock();
        inner.begin().expect("hotkey generation exhausted")
    }
    pub(crate) fn bind(&self, generation: u64, session_id: u64) -> bool {
        let mut inner = self.inner.lock();
        inner.bind(generation, session_id)
    }
    pub(crate) fn release(&self) -> Option<u64> {
        let mut inner = self.inner.lock();
        inner.release()
    }
    pub(crate) fn fail(&self, generation: u64) -> bool {
        let mut inner = self.inner.lock();
        inner.fail(generation)
    }
    '''
    compact = lambda text: re.sub(r"\s+", "", text)
    require(compact(wrapper_methods) == compact(expected_wrapper), "hotkey wrapper wiring changed")
    result = {"positive": instantiate(template, pieces)}
    controls = (
        ("stale-bind", "BIND", "self.generation != generation", "false"),
        ("duplicate-bind", "BIND", "self.session_id.is_some()", "false"),
        ("generation-wrap", "BEGIN", "self.generation.checked_add(1)", "Some(self.generation.wrapping_add(1))"),
        ("stale-failure", "FAIL", "self.generation != generation", "false"),
        ("release-retains-binding", "RELEASE", "self.session_id.take()", "self.session_id"),
    )
    for name, method, anchor, replacement in controls:
        token = f"/*__{method}_BODY__*/"
        require(pieces[token].count(anchor) == 1, f"mutation anchor changed: {name}")
        changed = dict(pieces)
        changed[token] = changed[token].replace(anchor, replacement, 1)
        result[f"negative-{name}"] = instantiate(template, changed)
    return result


def interval_programs(source, constants, template):
    code = without_comments(source)
    require("#[" not in code and not re.search(
        r"\b(?:type|struct|enum|trait|impl|mod|const|extern)\b", code),
        "interval context contains attributes, aliases or shadow items")
    imports = re.findall(r"\buse\s+([^;]+);", code)
    require(not any(re.search(r"\bas\b|\*|\b(?:u64|std|Option)\s*[,};]", item + ";")
                    for item in imports), "interval imports shadow proof context")
    constant_import = [item for item in imports if item.strip().startswith("super::{")]
    require(len(constant_import) == 1 and all(
        len(re.findall(r"\b" + name + r"\b", constant_import[0])) == 1
        for name in INTERVAL_CONSTANTS), "interval constants must come from the parent module")
    const_code = without_comments(constants)
    require(not re.search(r"\b(?:type\s+u64|as\s+u64)\b", const_code), "constant type alias changed")
    declarations = []
    for name, value in INTERVAL_CONSTANTS.items():
        matches = list(re.finditer(r"(?m)^pub\(crate\) const " + name + r": u64 = (\d+);", constants))
        require(len(matches) == 1 and int(matches[0][1]) == value,
                f"reviewed interval constant changed: {name}")
        require(len(re.findall(r"\bconst\s+" + name + r"\b", const_code)) == 1,
                f"duplicate interval constant: {name}")
        prefix = without_comments(constants[:matches[0].start()]).rstrip()
        require(not prefix.endswith("]"), f"conditional interval constant: {name}")
        declarations.append(matches[0][0])
    _, body = block(source,
                    r"^fn\s+adjust_interval\s*\(\s*current:\s*u64,\s*executed:\s*bool,\s*elapsed_ms:\s*u64,?\s*\)\s*->\s*u64",
                    "adjust_interval")
    require(len(re.findall(r"\bfn\s+adjust_interval\b", code)) == 1, "duplicate interval function")
    pieces = {"/*__INTERIM_CONSTANTS__*/": "\n".join(declarations),
              "/*__INTERVAL_BODY__*/": reviewed_body(body)}
    result = {"positive": instantiate(template, pieces)}
    for name, anchor, replacement in (
        ("upper-cap", ".min(INTERIM_INTERVAL_MAX_MS)", ""),
        ("lower-floor", ".max(INTERIM_INTERVAL_MIN_MS)", ""),
        ("heavy-threshold", "elapsed_ms >= INTERIM_HEAVY_COST_MS", "elapsed_ms > INTERIM_HEAVY_COST_MS"),
    ):
        require(body.count(anchor) == 1, f"mutation anchor changed: {name}")
        changed = dict(pieces)
        changed["/*__INTERVAL_BODY__*/"] = body.replace(anchor, replacement, 1)
        result[f"negative-{name}"] = instantiate(template, changed)
    return result


def prepare_domain(name, source, template, constants):
    for label, text in (("source", source), ("template", template), ("constants", constants)):
        require(TRUST_ESCAPE.search(text) is None, f"trust bypass in {name}/{label}")
    require("#[" not in without_comments(template) and not re.search(r"\bproof\s+fn\b", template),
            "template attributes or extra proof functions are outside the reviewed boundary")
    executable_names = re.findall(r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?fn\s+(\w+)\s*\(", template)
    require(tuple(executable_names) == DOMAINS[name][1], "template executable inventory changed")
    if name == "capture_bounds":
        result = programs(source, template)
    elif name == "hotkey_ownership":
        result = hotkey_programs(source, template)
    else:
        result = interval_programs(source, constants, template)
    require(len(set(result.values())) == len(result), f"{name}: controls must be distinct")
    return result


def run(command, label, logs, evidence):
    record = {"command": command, "cwd": str(ROOT), "timeout_seconds": TIMEOUT_SECONDS}
    evidence["runs"][label] = record
    started = time.monotonic()
    try:
        completed = subprocess.run(command, cwd=ROOT, capture_output=True, timeout=TIMEOUT_SECONDS)
        stdout, stderr = completed.stdout, completed.stderr
        record["exit_code"] = completed.returncode
    except subprocess.TimeoutExpired as error:
        stdout, stderr = error.stdout or b"", error.stderr or b""
        record.update(exit_code=None, error="timeout")
    except OSError as error:
        stdout, stderr = b"", str(error).encode("utf-8")
        record.update(exit_code=None, error=str(error))
    record["elapsed_seconds"] = round(time.monotonic() - started, 3)
    for suffix, content in (("stdout", stdout), ("stderr", stderr)):
        output = logs / f"{label}.{suffix}.log"
        output.write_bytes(content)
        record[suffix] = str(output)
    require(record["exit_code"] is not None, f"{label}: {record.get('error')}")
    return record, (stdout + b"\n" + stderr).decode("utf-8", errors="replace")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verus", required=True, help="path to the Verus executable")
    parser.add_argument("--logs", required=True, type=Path)
    args = parser.parse_args()
    logs = args.logs.resolve()
    logs.mkdir(parents=True, exist_ok=True)
    evidence = {"status": "failed", "runs": {}, "contracts": {}, "flags": FLAGS}
    try:
        inputs = {}

        def read_input(path):
            data = path.read_bytes()
            inputs[path] = {"path": str(path), "sha256": hashlib.sha256(data).hexdigest()}
            return data.decode("utf-8")

        evidence["runner_sha256"] = sha256(Path(__file__))
        constants = production_scope(read_input(CONSTANT_SOURCE))
        evidence["constants"] = inputs[CONSTANT_SOURCE]
        # Finish all source/template checks before executing any proof. A broken
        # later domain must not disappear behind successful earlier domains.
        for domain, (source_path, functions) in DOMAINS.items():
            template_path = ROOT / f"formal/verus/{domain}.template.rs"
            source = production_scope(read_input(source_path))
            template = read_input(template_path)
            constant_initializers = len(INTERVAL_CONSTANTS) if domain == "interim_interval" else 0
            record = {"source": inputs[source_path], "template": inputs[template_path],
                      "functions": functions, "constant_initializers": constant_initializers,
                      "expected_verified": len(functions) + constant_initializers,
                      "runs": {}, "programs": {}}
            evidence["contracts"][domain] = record
            generated = prepare_domain(domain, source, template, constants)
            directory = logs / domain
            directory.mkdir(exist_ok=True)
            for name, program in generated.items():
                path = directory / f"{name}.rs"
                path.write_bytes(program.encode("utf-8"))
                record["programs"][name] = {"path": str(path), "sha256": sha256(path)}

        verus = Path(shutil.which(args.verus) or args.verus).resolve()
        require(verus.is_file(), f"Verus executable not found: {verus}")
        evidence["tool"] = {"path": str(verus), "sha256": sha256(verus)}
        version, output = run([str(verus), "--version"], "version", logs, evidence)
        require(version["exit_code"] == 0 and output.strip(), "Verus version query failed")
        evidence["tool"]["version"] = output.strip()

        for domain, record in evidence["contracts"].items():
            for name, program in record["programs"].items():
                label = f"{domain}/{name}"
                command = [str(verus), *FLAGS, program["path"]]
                checked, output = run(command + ["--no-verify"], name + "-typecheck", logs / domain, record)
                require(checked["exit_code"] == 0 and not re.search(r"^error(?:\[|:)", output, re.M),
                        f"{label}: typecheck failed")
                require(sha256(Path(program["path"])) == program["sha256"],
                        f"{label}: generated source changed after typecheck")
                proved, output = run(command, name + "-proof", logs / domain, record)
                require(sha256(Path(program["path"])) == program["sha256"],
                        f"{label}: generated source changed during proof")
                summaries = SUMMARY.findall(output)
                require(len(summaries) == 1 and INVALID_PROOF.search(output) is None,
                        f"{label}: missing or invalid verification result")
                verified, errors = map(int, summaries[0])
                proved.update(verified=verified, verification_errors=errors)
                if name == "positive":
                    require(proved["exit_code"] == 0 and verified == record["expected_verified"] and errors == 0,
                            f"{domain}: proof must verify the full executable inventory")
                else:
                    require(proved["exit_code"] == 1 and errors > 0 and SEMANTIC_ERROR.search(output),
                            f"{label}: expected a semantic verification failure")
        for path, record in inputs.items():
            require(sha256(path) == record["sha256"], f"{path} changed during verification")
        require(sha256(verus) == evidence["tool"]["sha256"], "tool changed during verification")
        require(sha256(Path(__file__)) == evidence["runner_sha256"], "runner changed during verification")
        evidence["status"] = "passed"
        verified = sum(len(record["functions"]) for record in evidence["contracts"].values())
        rejected = sum(len(record["programs"]) - 1 for record in evidence["contracts"].values())
        print(f"Verus: {len(DOMAINS)} domains, {verified} functions proved; {rejected} negative controls rejected.")
        return 0
    except (OSError, ValueError) as error:
        evidence["error"] = str(error)
        print(f"Verus contracts failed: {error}", file=sys.stderr)
        return 1
    finally:
        (logs / "summary.json").write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    raise SystemExit(main())
