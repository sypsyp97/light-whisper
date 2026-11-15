"""Test-only entrypoint observation in an owned, separately built mirror."""

from pathlib import Path
import hashlib
import json
import os
import re
import stat

LIB = "src-tauri/src/lib.rs"
MARKER = "LIGHT_WHISPER_ENTRYPOINT_BOUNDARY_ONLY_1"
EXIT = 86
REVIEWED = json.loads(Path(__file__).with_name("reviewed.json").read_text())
TRUSTED_FIXTURE_SHA256 = "2c42e5ed583c398ae03a5d5d0b6039e7634d5737c496e792344f764d3d5277c0"
OBSERVER = r'''    { // verification-only entrypoint observation
        println!(
            "{{\"entrypoint_test_only\":\"LIGHT_WHISPER_ENTRYPOINT_BOUNDARY_ONLY_1\",\"surface\":\"gui-boundary-intercept\",\"pid\":{}}}",
            std::process::id()
        );
        std::io::Write::flush(&mut std::io::stdout()).expect("test observer stdout flush");
        std::process::exit(86);
    }
'''


def _plain(path, *, exists=False):
    path = Path(path)
    if ".." in path.parts:
        raise RuntimeError("measurement path must not traverse links or parent components")
    path = Path(os.path.abspath(path))
    for component in (path, *path.parents):
        try:
            information = component.lstat()
        except FileNotFoundError:
            continue
        if (stat.S_ISLNK(information.st_mode)
                or getattr(information, "st_file_attributes", 0)
                & stat.FILE_ATTRIBUTE_REPARSE_POINT):
            raise RuntimeError("measurement path traverses a link or reparse point: " + str(component))
    try:
        return path.resolve(strict=exists)
    except OSError as error:
        raise RuntimeError("measurement ownership path unavailable: " + str(path)) from error


def _file(path):
    path = _plain(path, exists=True)
    if not path.is_file():
        raise RuntimeError("measurement identity requires a regular file: " + str(path))
    if path.stat().st_nlink != 1:
        raise RuntimeError("measurement file has shared identity or hardlinks: " + str(path))
    return path


def _sha(data):
    return hashlib.sha256(data).hexdigest()


def _owner(path, kind, mirror):
    try:
        owner = json.loads(_file(path).read_text(encoding="utf-8"))
        if owner["kind"] == kind and _plain(owner["mirror"], exists=True) == mirror:
            return owner
    except (OSError, ValueError, KeyError, TypeError):
        pass
    raise RuntimeError("measurement ownership mismatch: " + str(path))


def _separate(*paths):
    for index, left in enumerate(paths):
        for right in paths[index + 1:]:
            if left.is_relative_to(right) or right.is_relative_to(left):
                raise RuntimeError("measurement paths overlap; require separate outside directories")


def _variant(candidate, original):
    if candidate == original:
        return "original"
    for variant in REVIEWED["variants"]:
        lines = original.splitlines(keepends=True)
        index = variant["line"] - 1
        line = lines[index]
        if line.rstrip(b"\r\n") != variant["before"].encode():
            raise RuntimeError("reviewed variant source identity mismatch")
        newline = b"\r\n" if line.endswith(b"\r\n") else b"\n"
        lines[index] = variant["after"].encode() + newline
        generated = b"".join(lines)
        if candidate == generated and _sha(candidate) == variant["sha256"]:
            return variant["key"]
    raise RuntimeError("unreviewed entrypoint variant identity")


def _mirror(mirror, *, original=None):
    mirror = _plain(mirror, exists=True)
    owner = _owner(mirror.parent / "ownership.json", "native-agent-api-measurement", mirror)
    try:
        source = _plain(owner["source_repo"], exists=True)
    except (KeyError, TypeError) as error:
        raise RuntimeError("measurement source ownership mismatch") from error
    _separate(source, mirror)
    if not source.is_dir() or not mirror.is_dir():
        raise RuntimeError("measurement ownership requires source and mirror directories")
    observed_head = owner.get("observed_head")
    if not isinstance(observed_head, str) or re.fullmatch("[0-9a-f]{40}", observed_head) is None:
        raise RuntimeError("measurement ownership snapshot mismatch")
    copied = owner.get("copied_source_sha256", {})
    for relative, expected in REVIEWED["frozen"].items():
        shipping = _file(source / relative).read_bytes()
        measured = _file(mirror / relative).read_bytes()
        if copied.get(relative) != expected or _sha(shipping) != expected:
            raise RuntimeError("frozen source identity changed: " + relative)
        if relative == LIB:
            variant = _variant(measured if original is None else original, shipping)
        elif _sha(measured) != expected:
            raise RuntimeError("frozen mirror identity changed: " + relative)
    return mirror, source, variant, observed_head


def _overlay(original):
    anchor = b"    tauri::Builder::default()"
    if original.count(anchor) != 1:
        raise RuntimeError("normal builder observer identity mismatch")
    newline = "\r\n" if b"\r\n" in original else "\n"
    return original.replace(anchor, OBSERVER.replace("\n", newline).encode() + anchor, 1)


def apply_overlay(mirror, evidence, target):
    mirror, source, variant, observed_head = _mirror(mirror)
    evidence, target = _plain(evidence), _plain(target)
    _separate(source, mirror, evidence, target)
    if evidence.exists() or target.exists():
        raise RuntimeError("measurement evidence and target must be fresh; existing paths refused")
    library = _file(mirror / LIB)
    original = library.read_bytes()
    transformed = _overlay(original)
    receipt = {
        "kind": "native-entrypoint-observation-overlay-v1",
        "mirror": str(mirror), "source_repo": str(source),
        "evidence": str(evidence), "target": str(target),
        "observed_head": observed_head, "variant": variant,
        "frozen_sha256": REVIEWED["frozen"],
        "original_sha256": _sha(original), "transformed_sha256": _sha(transformed),
    }
    evidence.mkdir(parents=True)
    target.mkdir(parents=True)
    with (evidence / "lib.rs.original").open("xb") as backup:
        backup.write(original)
    with (evidence / "ownership.json").open("x", encoding="utf-8") as ownership:
        ownership.write(json.dumps(receipt, indent=2) + "\n")
    library.write_bytes(transformed)
    return receipt


def _applied(mirror, evidence):
    mirror, evidence = _plain(mirror, exists=True), _plain(evidence, exists=True)
    receipt = _owner(evidence / "ownership.json", "native-entrypoint-observation-overlay-v1", mirror)
    original = _file(evidence / "lib.rs.original").read_bytes()
    mirror, source, variant, observed_head = _mirror(mirror, original=original)
    try:
        target = _plain(receipt["target"], exists=True)
        if (_plain(receipt["source_repo"], exists=True) != source
                or _plain(receipt["evidence"], exists=True) != evidence):
            raise RuntimeError("overlay ownership mismatch")
    except (KeyError, TypeError) as error:
        raise RuntimeError("overlay ownership mismatch") from error
    _separate(source, mirror, evidence, target)
    transformed = _overlay(original)
    if (receipt.get("variant") != variant
            or receipt.get("observed_head") != observed_head
            or receipt.get("frozen_sha256") != REVIEWED["frozen"]
            or receipt.get("original_sha256") != _sha(original)
            or receipt.get("transformed_sha256") != _sha(transformed)
            or _file(mirror / LIB).read_bytes() != transformed):
        raise RuntimeError("overlay, observer or backup identity changed; refusing later edits")
    return receipt, original, target


def restore(mirror, evidence):
    _, original, _ = _applied(mirror, evidence)
    _file(Path(mirror) / LIB).write_bytes(original)


def verify_launch(mirror, evidence, app_exe, app_sha256, fixture_exe):
    receipt, _, target = _applied(mirror, evidence)
    app_exe = _plain(app_exe, exists=True)
    if app_exe != target / "debug/light-whisper.exe":
        raise RuntimeError("app identity must be the owned debug target artifact")
    app = _file(app_exe).read_bytes()
    fixture_sha256 = _sha(_file(fixture_exe).read_bytes())
    if (_sha(app) != app_sha256 or MARKER.encode() not in app
            or b"LIGHT_WHISPER_AGENT_PROTOCOL_1" not in app):
        raise RuntimeError("compiled app identity or observer marker mismatch")
    if fixture_sha256 != TRUSTED_FIXTURE_SHA256:
        raise RuntimeError("trusted fixture identity mismatch")
    return {**receipt, "app_sha256": app_sha256, "fixture_sha256": fixture_sha256}


def probe_environment(environment, scratch, *, empty_data):
    scratch = _plain(scratch, exists=True)
    if not scratch.is_dir():
        raise RuntimeError("probe ownership requires a scratch directory")
    paths = {key: _plain(scratch / name) for key, name in (
        ("APPDATA", "appdata"), ("LOCALAPPDATA", "localappdata"),
        ("TEMP", "temp"), ("TMP", "tmp"))}
    if not empty_data:
        paths["LIGHT_WHISPER_DATA_DIR"] = _plain(scratch / "data")
    for path in paths.values():
        if path.exists() and not path.is_dir():
            raise RuntimeError("probe environment ownership requires directories")
    for path in paths.values():
        path.mkdir(exist_ok=True)
    result = {**environment, **{key: str(path) for key, path in paths.items()}}
    if empty_data:
        result["LIGHT_WHISPER_DATA_DIR"] = ""
    return result


def _probe_case(case, *, empty_data):
    scratch, cwd = _plain(case.scratch, exists=True), _plain(case.data, exists=True)
    if not cwd.is_dir() or not cwd.is_relative_to(scratch):
        raise RuntimeError("probe cwd must be inside owned scratch")
    if "LIGHT_WHISPER_DATA_DIR" not in case.env:
        raise RuntimeError("explicit data environment required; absent variable refused")
    return probe_environment(case.env, scratch, empty_data=empty_data)


def client_probe(case, driver, executable):
    environment = _probe_case(case, empty_data=True)
    case.assertFalse(case.endpoint.exists())
    case.assertEqual(case.audit(), [])
    client = driver.Process(case, [str(executable), "--agent-client"], "entrypoint-client", environment)
    ready = client.receive()
    case.assertIs(ready.get("success"), True)
    case.assertEqual(ready.get("surface"), "desktop-client")
    case.assertEqual(ready.get("agent_protocol"), driver.MARKER)
    client.send({"request_id": 181, "action": "exit"})
    reply = client.receive()
    case.assertIs(reply.get("success"), True)
    case.assertEqual(reply.get("request_id"), 181)
    client.wait(0)
    case.assertFalse(case.endpoint.exists())
    case.assertEqual(case.audit(), [])


def desktop_probe(case, driver, executable, arguments):
    if arguments not in ([], ["--minimized"]):
        raise ValueError("only original argv and --minimized desktop probes are supported")
    environment = _probe_case(case, empty_data=False)
    case.assertFalse(case.endpoint.exists())
    case.assertEqual(case.audit(), [])
    desktop = driver.Process(case, [str(executable), *arguments], "entrypoint-desktop", environment)
    case.assertEqual(desktop.receive(), {
        "entrypoint_test_only": MARKER, "surface": "gui-boundary-intercept",
        "pid": desktop.child.pid,
    })
    desktop.wait(EXIT)
    case.assertFalse(case.endpoint.exists())
    case.assertEqual(case.audit(), [])
