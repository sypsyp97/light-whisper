"""Pure filesystem/protocol tests. Never compile or launch the native application."""

import copy
import hashlib
import json
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import entrypoint_probe as probe

CHECKOUT = Path(__file__).resolve().parents[3]
PUBLIC_HEAD = "9f77717cc562367d6641ac073878bdc995ce97bf"
SYNTHETIC_HEAD = "0123456789abcdef0123456789abcdef01234567"
EXPECTED_OBSERVER = r'''    { // verification-only entrypoint observation
        println!(
            "{{\"entrypoint_test_only\":\"LIGHT_WHISPER_ENTRYPOINT_BOUNDARY_ONLY_1\",\"surface\":\"gui-boundary-intercept\",\"pid\":{}}}",
            std::process::id()
        );
        std::io::Write::flush(&mut std::io::stdout()).expect("test observer stdout flush");
        std::process::exit(86);
    }
'''


class Observation(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="lw-entrypoint-unit-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.source = self.root / "shipping"
        self.mirror = self.root / "measurement/repo"
        self.evidence = self.root / "observation"
        self.target = self.root / "target"
        for root in (self.source, self.mirror):
            for relative in probe.REVIEWED["frozen"]:
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes((CHECKOUT / relative).read_bytes())
        self.lib = self.mirror / probe.LIB
        self.original = self.lib.read_bytes()
        self.owner = self.mirror.parent / "ownership.json"
        self.owner.write_text(json.dumps({"kind": "native-agent-api-measurement",
            "source_repo": str(self.source), "mirror": str(self.mirror),
            "observed_head": SYNTHETIC_HEAD,
            "copied_source_sha256": probe.REVIEWED["frozen"]}))

    def apply(self):
        return probe.apply_overlay(self.mirror, self.evidence, self.target)

    def mutate(self, key):
        variant = next(v for v in probe.REVIEWED["variants"] if v["key"] == key)
        lines = self.lib.read_bytes().splitlines(keepends=True)
        index = variant["line"] - 1
        newline = b"\r\n" if lines[index].endswith(b"\r\n") else b"\n"
        lines[index] = variant["after"].encode() + newline
        self.lib.write_bytes(b"".join(lines))

    def set_owner_head(self, head):
        owner = json.loads(self.owner.read_text())
        owner["observed_head"] = head
        self.owner.write_text(json.dumps(owner))

    def files(self):
        return {path.relative_to(self.root).as_posix():
                path.read_bytes() if path.is_file() else None
                for path in self.root.rglob("*")}

    def assert_overlay_actions_refused_without_writes(self):
        app, expected, fixture = self.artifacts()
        before = self.files()
        with patch.object(probe, "TRUSTED_FIXTURE_SHA256", hashlib.sha256(fixture.read_bytes()).hexdigest()):
            for operation, action in (
                    ("launch", lambda: probe.verify_launch(self.mirror, self.evidence, app, expected, fixture)),
                    ("restore", lambda: probe.restore(self.mirror, self.evidence))):
                with self.subTest(operation=operation):
                    with self.assertRaises(RuntimeError):
                        action()
                    self.assertEqual(self.files(), before)

    def test_matching_content_admits_public_and_other_prepared_heads(self):
        for head in (PUBLIC_HEAD, SYNTHETIC_HEAD):
            for variant in (None, *[item["key"] for item in probe.REVIEWED["variants"]]):
                with self.subTest(head=head, variant=variant or "original"):
                    self.set_owner_head(head)
                    self.lib.write_bytes(self.original)
                    if variant is not None:
                        self.mutate(variant)
                    original = self.lib.read_bytes()
                    suffix = head + "-" + (variant or "original")
                    self.evidence = self.root / (suffix + "-observation")
                    self.target = self.root / (suffix + "-target")
                    self.assertEqual(json.loads(self.owner.read_text())["copied_source_sha256"], probe.REVIEWED["frozen"])
                    for relative, expected in probe.REVIEWED["frozen"].items():
                        self.assertEqual(hashlib.sha256((self.source / relative).read_bytes()).hexdigest(), expected)
                        if relative == probe.LIB and variant is not None:
                            expected = next(item["sha256"] for item in probe.REVIEWED["variants"] if item["key"] == variant)
                        self.assertEqual(hashlib.sha256((self.mirror / relative).read_bytes()).hexdigest(), expected)
                    try:
                        receipt = self.apply()
                    except RuntimeError as error:
                        self.fail("reviewed content at prepared HEAD " + head + " was refused: " + str(error))
                    self.assertEqual(receipt["observed_head"], head)
                    recorded = json.loads((self.evidence / "ownership.json").read_text())
                    self.assertEqual(recorded["observed_head"], head)
                    self.assertEqual(recorded["frozen_sha256"], probe.REVIEWED["frozen"])
                    self.assertEqual(recorded["variant"], variant or "original")
                    app, expected, fixture = self.artifacts()
                    with patch.object(probe, "TRUSTED_FIXTURE_SHA256", hashlib.sha256(fixture.read_bytes()).hexdigest()):
                        accepted = probe.verify_launch(self.mirror, self.evidence, app, expected, fixture)
                    self.assertEqual(accepted["observed_head"], head)
                    probe.restore(self.mirror, self.evidence)
                    self.assertEqual(self.lib.read_bytes(), original)

    def test_missing_or_malformed_prepared_head_is_refused_before_writes(self):
        owner = json.loads(self.owner.read_text())
        owner.pop("observed_head")
        cases = [{}, *[{"observed_head": value} for value in (
            None, "", "a" * 39, "a" * 41, PUBLIC_HEAD.upper(), "g" * 40,
            PUBLIC_HEAD + "\n", " " + PUBLIC_HEAD, 1, True, [], {})]]
        for invalid in cases:
            with self.subTest(head=invalid):
                self.owner.write_text(json.dumps({**owner, **invalid}))
                before = self.files()
                with self.assertRaises(RuntimeError):
                    self.apply()
                self.assertEqual(self.files(), before)
                self.assertFalse(self.evidence.exists())
                self.assertFalse(self.target.exists())

    def test_prepared_head_drift_refuses_launch_and_restore_before_writes(self):
        self.apply()
        self.set_owner_head(PUBLIC_HEAD)
        self.assert_overlay_actions_refused_without_writes()

    def test_receipt_head_mismatch_refuses_launch_and_restore_before_writes(self):
        receipt = self.apply()
        receipt["observed_head"] = PUBLIC_HEAD
        (self.evidence / "ownership.json").write_text(json.dumps(receipt))
        self.assert_overlay_actions_refused_without_writes()

    def test_all_prepared_copy_pins_are_required_before_writes(self):
        owner = json.loads(self.owner.read_text())
        for relative in probe.REVIEWED["frozen"]:
            for copied in ({**owner["copied_source_sha256"], relative: "0" * 64},
                           {key: value for key, value in owner["copied_source_sha256"].items() if key != relative}):
                with self.subTest(path=relative, present=relative in copied):
                    self.owner.write_text(json.dumps({**owner, "copied_source_sha256": copied}))
                    before = self.files()
                    with self.assertRaises(RuntimeError):
                        self.apply()
                    self.assertEqual(self.files(), before)
                    self.assertFalse(self.evidence.exists())
                    self.assertFalse(self.target.exists())

    def test_overlay_preserves_every_original_byte_and_stops_before_builder(self):
        receipt = self.apply()
        transformed = self.lib.read_bytes()
        self.assertTrue(probe.MARKER.encode() in transformed, "observer boundary marker missing")
        insertion = transformed.index(b"    { // verification-only entrypoint observation")
        builder = transformed.index(b"    tauri::Builder::default()")
        self.assertLess(insertion, builder)
        self.assertEqual(transformed[:insertion], self.original[:insertion])
        hook = transformed[insertion:builder]
        self.assertEqual(probe.OBSERVER, EXPECTED_OBSERVER)
        self.assertEqual(hook.replace(b"\r\n", b"\n").decode(), EXPECTED_OBSERVER)
        self.assertEqual(transformed.replace(hook, b"", 1), self.original)
        self.assertEqual(receipt["variant"], "original")
        self.assertEqual(receipt["transformed_sha256"], hashlib.sha256(transformed).hexdigest())
        self.assertEqual((self.evidence / "lib.rs.original").read_bytes(), self.original)
        self.assertEqual((self.source / probe.LIB).read_bytes(), self.original)
        probe.restore(self.mirror, self.evidence)
        self.assertEqual(self.lib.read_bytes(), self.original)

    def test_each_exact_single_generated_variant_is_admitted_and_restored(self):
        for variant in probe.REVIEWED["variants"]:
            with self.subTest(variant=variant["key"]):
                self.lib.write_bytes(self.original)
                self.mutate(variant["key"])
                candidate = self.lib.read_bytes()
                self.assertEqual(hashlib.sha256(candidate).hexdigest(), variant["sha256"])
                self.evidence = self.root / variant["key"]
                self.target = self.root / (variant["key"] + "-target")
                receipt = self.apply()
                self.assertEqual(receipt["variant"], variant["key"])
                probe.restore(self.mirror, self.evidence)
                self.assertEqual(self.lib.read_bytes(), candidate)

    def test_unknown_multiple_and_prefix_edits_refused_without_writes(self):
        candidates = [self.original + b"\n// unknown\n", b"// prefix\n" + self.original]
        self.mutate("initial-host-inverse")
        self.mutate("final-host-inverse")
        candidates.append(self.lib.read_bytes())
        for candidate in candidates:
            with self.subTest(candidate=hashlib.sha256(candidate).hexdigest()):
                self.lib.write_bytes(candidate)
                with self.assertRaisesRegex(RuntimeError, "identity|variant"):
                    self.apply()
                self.assertEqual(self.lib.read_bytes(), candidate)
                self.assertFalse(self.evidence.exists())
                self.assertFalse(self.target.exists())

    def test_all_twenty_one_frozen_identities_refuse_source_or_mirror_drift(self):
        self.assertEqual(len(probe.REVIEWED["frozen"]), 21)
        for root in (self.source, self.mirror):
            for relative in probe.REVIEWED["frozen"]:
                with self.subTest(root=root.name, path=relative):
                    path = root / relative
                    original = path.read_bytes()
                    try:
                        path.write_bytes(original + b"\n")
                        with self.assertRaisesRegex(RuntimeError, "identity|variant"):
                            self.apply()
                        self.assertEqual(path.read_bytes(), original + b"\n")
                        self.assertFalse(self.evidence.exists())
                        self.assertFalse(self.target.exists())
                    finally:
                        path.write_bytes(original)

    def test_owner_mismatch_and_shared_file_identity_refused(self):
        original_owner = self.owner.read_bytes()
        self.owner.write_text("{}")
        with self.assertRaisesRegex(RuntimeError, "ownership"):
            self.apply()
        self.owner.write_bytes(original_owner)
        self.lib.unlink()
        os.link(self.source / probe.LIB, self.lib)
        with self.assertRaisesRegex(RuntimeError, "shared|hardlink"):
            self.apply()
        self.assertEqual((self.source / probe.LIB).read_bytes(), self.original)

    def test_source_mirror_overlap_is_refused_before_writes(self):
        owner = json.loads(self.owner.read_text())
        nested = self.mirror / "nested-source"
        nested.mkdir()
        for source in (self.mirror, self.mirror.parent, nested):
            with self.subTest(source=source):
                owner["source_repo"] = str(source)
                self.owner.write_text(json.dumps(owner))
                with self.assertRaisesRegex(RuntimeError, "overlap|outside|separate"):
                    self.apply()
                self.assertEqual(self.lib.read_bytes(), self.original)
                self.assertFalse(self.evidence.exists())
                self.assertFalse(self.target.exists())

    def test_existing_evidence_or_target_is_refused_before_writes(self):
        for path in (self.evidence, self.target):
            with self.subTest(path=path):
                path.mkdir()
                try:
                    with self.assertRaisesRegex(RuntimeError, "fresh|exist"):
                        self.apply()
                    self.assertEqual(self.lib.read_bytes(), self.original)
                    self.assertEqual(list(path.iterdir()), [])
                finally:
                    path.rmdir()

    def test_evidence_target_source_and_mirror_overlaps_refused_before_writes(self):
        normal_evidence, normal_target = self.evidence, self.target
        cases = [(self.source / "observation", normal_target),
                 (self.mirror / "observation", normal_target),
                 (normal_evidence, self.source / "target"),
                 (normal_evidence, self.mirror / "target"),
                 (normal_evidence, normal_evidence),
                 (normal_evidence, normal_evidence / "target"),
                 (normal_target / "observation", normal_target)]
        for evidence, target in cases:
            with self.subTest(evidence=evidence, target=target):
                self.evidence, self.target = evidence, target
                with self.assertRaisesRegex(RuntimeError, "overlap|outside|separate"):
                    self.apply()
                self.assertEqual(self.lib.read_bytes(), self.original)
                self.assertFalse(evidence.exists())
                self.assertFalse(target.exists())

    @unittest.skipUnless(os.name == "nt", "Windows junction boundary")
    def test_junction_source_mirror_evidence_and_target_paths_refused(self):
        import _winapi
        source_link = self.root / "source-link"
        mirror_link = self.root / "mirror-link"
        output_parent = self.root / "output-parent"
        output_parent.mkdir()
        output_link = self.root / "output-link"
        for target, link in ((self.source, source_link), (self.mirror, mirror_link),
                             (output_parent, output_link)):
            _winapi.CreateJunction(str(target), str(link))
            # Exact junction removal runs before TemporaryDirectory cleanup.
            self.addCleanup(link.rmdir)
        owner = json.loads(self.owner.read_text())
        cases = [(source_link, self.mirror, self.evidence, self.target),
                 (self.source, mirror_link, self.evidence, self.target),
                 (self.source, self.mirror, output_link / "observation", self.target),
                 (self.source, self.mirror, self.evidence, output_link / "target")]
        for source, mirror, evidence, target in cases:
            with self.subTest(source=source, mirror=mirror, evidence=evidence, target=target):
                owner["source_repo"] = str(source)
                self.owner.write_text(json.dumps(owner))
                with self.assertRaisesRegex(RuntimeError, "link|reparse"):
                    probe.apply_overlay(mirror, evidence, target)
                self.assertEqual(self.lib.read_bytes(), self.original)
                self.assertFalse(evidence.exists())
                self.assertFalse(target.exists())

    @unittest.skipUnless(os.name == "nt", "Windows tracked-source junction boundary")
    def test_matching_frozen_source_behind_directory_junction_refused(self):
        import _winapi
        path = self.mirror / "src-tauri/src/utils/paths.rs"
        path.unlink()
        path.parent.rmdir()
        _winapi.CreateJunction(str(self.source / "src-tauri/src/utils"), str(path.parent))
        self.addCleanup(path.parent.rmdir)
        with self.assertRaisesRegex(RuntimeError, "link|reparse"):
            self.apply()
        self.assertEqual(self.lib.read_bytes(), self.original)
        self.assertFalse(self.evidence.exists())
        self.assertFalse(self.target.exists())

    def test_restore_refuses_later_edits_and_changed_backup(self):
        self.apply()
        transformed = self.lib.read_bytes()
        self.lib.write_bytes(transformed + b"\n// later\n")
        with self.assertRaisesRegex(RuntimeError, "identity"):
            probe.restore(self.mirror, self.evidence)
        self.assertTrue(self.lib.read_bytes().endswith(b"// later\n"))
        self.lib.write_bytes(transformed)
        (self.evidence / "lib.rs.original").write_bytes(b"changed backup")
        with self.assertRaisesRegex(RuntimeError, "identity"):
            probe.restore(self.mirror, self.evidence)

    def artifacts(self):
        """Synthetic unit artifacts; only the fixture trust pin is temporarily replaced."""
        app = self.target / "debug/light-whisper.exe"
        app.parent.mkdir(parents=True)
        app.write_bytes(b"synthetic-unit-only " + probe.MARKER.encode() + b" LIGHT_WHISPER_AGENT_PROTOCOL_1")
        fixture = self.root / "fixture.exe"
        fixture.write_bytes(b"synthetic-unit-fixture")
        expected = hashlib.sha256(app.read_bytes()).hexdigest()
        return app, expected, fixture

    def assert_refused_without_changes(self, action, reason=None):
        before = self.files()
        error = None
        try:
            action()
        except Exception as caught:
            error = caught
        self.assertEqual(self.files(), before, "refused operation changed owned files")
        self.assertIsInstance(error, RuntimeError, "refusal must report a public RuntimeError")
        if reason is not None:
            self.assertRegex(str(error), reason)

    def test_missing_or_malformed_source_owner_refused_without_writes(self):
        owner = json.loads(self.owner.read_text())
        missing = object()
        for value in (missing, None, False, 17, [], {}):
            with self.subTest(source_repo="missing" if value is missing else value):
                changed = dict(owner)
                if value is missing:
                    changed.pop("source_repo")
                else:
                    changed["source_repo"] = value
                self.owner.write_text(json.dumps(changed))
                self.assert_refused_without_changes(self.apply)
                self.assertFalse(self.evidence.exists())
                self.assertFalse(self.target.exists())

    def test_source_and_mirror_files_refused_as_directory_owners(self):
        owner = json.loads(self.owner.read_text())
        ordinary_file = self.root / "ordinary-file"
        ordinary_file.write_bytes(b"not a measurement directory")
        for source, mirror in ((ordinary_file, self.mirror), (self.source, ordinary_file)):
            with self.subTest(source=source.name, mirror=mirror.name):
                self.owner.write_text(json.dumps({**owner, "source_repo": str(source),
                                                 "mirror": str(mirror)}))
                # A file mirror's owner must remain at the public seam's expected location.
                if mirror == ordinary_file:
                    (self.root / "ownership.json").write_bytes(self.owner.read_bytes())
                self.assert_refused_without_changes(
                    lambda: probe.apply_overlay(mirror, self.evidence, self.target))
                self.assertFalse(self.evidence.exists())
                self.assertFalse(self.target.exists())

    def test_receipt_source_or_evidence_mismatch_refuses_restore_and_launch(self):
        receipt = self.apply()
        transformed = self.lib.read_bytes()
        app, expected, fixture = self.artifacts()
        other = self.root / "other-owned-directory"
        other.mkdir()
        with patch.object(probe, "TRUSTED_FIXTURE_SHA256", hashlib.sha256(fixture.read_bytes()).hexdigest()):
            for key in ("source_repo", "evidence"):
                for operation, action in (
                        ("launch", lambda: probe.verify_launch(self.mirror, self.evidence, app, expected, fixture)),
                        ("restore", lambda: probe.restore(self.mirror, self.evidence))):
                    with self.subTest(key=key, operation=operation):
                        self.lib.write_bytes(transformed)
                        (self.evidence / "ownership.json").write_text(json.dumps({**receipt, key: str(other)}))
                        self.assert_refused_without_changes(action)

    def test_missing_or_malformed_receipt_paths_refuse_restore_and_launch(self):
        receipt = self.apply()
        transformed = self.lib.read_bytes()
        app, expected, fixture = self.artifacts()
        missing = object()
        with patch.object(probe, "TRUSTED_FIXTURE_SHA256", hashlib.sha256(fixture.read_bytes()).hexdigest()):
            for key in ("target", "source_repo", "evidence"):
                for value in (missing, None, False, 17, [], {}):
                    changed = dict(receipt)
                    if value is missing:
                        changed.pop(key)
                    else:
                        changed[key] = value
                    for operation, action in (
                            ("launch", lambda: probe.verify_launch(self.mirror, self.evidence, app, expected, fixture)),
                            ("restore", lambda: probe.restore(self.mirror, self.evidence))):
                        with self.subTest(key=key, value="missing" if value is missing else value,
                                          operation=operation):
                            self.lib.write_bytes(transformed)
                            (self.evidence / "ownership.json").write_text(json.dumps(changed))
                            self.assert_refused_without_changes(action)

    def test_matching_artifact_bytes_at_wrong_paths_refused(self):
        self.apply()
        app, expected, fixture = self.artifacts()
        with patch.object(probe, "TRUSTED_FIXTURE_SHA256", hashlib.sha256(fixture.read_bytes()).hexdigest()):
            for wrong in (self.target / "debug/renamed.exe", self.root / "other-debug/light-whisper.exe"):
                with self.subTest(path=wrong.relative_to(self.root)):
                    wrong.parent.mkdir(parents=True, exist_ok=True)
                    wrong.write_bytes(app.read_bytes())
                    self.assertEqual(hashlib.sha256(wrong.read_bytes()).hexdigest(), expected)
                    self.assert_refused_without_changes(
                        lambda: probe.verify_launch(self.mirror, self.evidence, wrong, expected, fixture))

    def test_valid_owned_artifacts_are_accepted_without_launching(self):
        self.apply()
        app, expected, fixture = self.artifacts()
        cohort = json.loads((CHECKOUT / "formal/agent-api-measurements/cohort.json").read_text())
        self.assertEqual(probe.TRUSTED_FIXTURE_SHA256, cohort["trusted_fixture_sha256"])
        with patch.object(probe, "TRUSTED_FIXTURE_SHA256", hashlib.sha256(fixture.read_bytes()).hexdigest()):
            accepted = probe.verify_launch(self.mirror, self.evidence, app, expected, fixture)
        self.assertIsInstance(accepted, dict, "valid owned artifacts must pass identity admission")
        self.assertEqual(accepted["app_sha256"], expected)
        self.assertEqual(accepted["fixture_sha256"], hashlib.sha256(fixture.read_bytes()).hexdigest())

    def test_all_frozen_identities_are_rechecked_before_launch(self):
        self.apply()
        app, expected, fixture = self.artifacts()
        with patch.object(probe, "TRUSTED_FIXTURE_SHA256", hashlib.sha256(fixture.read_bytes()).hexdigest()):
            for root in (self.source, self.mirror):
                for relative in probe.REVIEWED["frozen"]:
                    with self.subTest(root=root.name, path=relative):
                        path = root / relative
                        original = path.read_bytes()
                        try:
                            path.write_bytes(original + b"\n")
                            with self.assertRaisesRegex(RuntimeError, "identity|variant|observer"):
                                probe.verify_launch(self.mirror, self.evidence, app, expected, fixture)
                        finally:
                            path.write_bytes(original)

    def test_launch_requires_exact_frozen_sources_and_owned_binary_identity(self):
        self.apply()
        app, expected, fixture = self.artifacts()
        # A fake fixture and wrong app digest never reach a process launcher.
        for digest in ("0" * 64, expected):
            with self.assertRaisesRegex(RuntimeError, "identity|fixture"):
                probe.verify_launch(self.mirror, self.evidence, app, digest, fixture)
        (self.mirror / "formal/agent-api-host/run.py").write_bytes(b"changed driver")
        with self.assertRaisesRegex(RuntimeError, "identity"):
            probe.verify_launch(self.mirror, self.evidence, app, expected, fixture)

    def test_conditional_observer_refused_even_if_receipt_hash_is_rewritten(self):
        receipt = self.apply()
        transformed = self.lib.read_bytes()
        self.assertTrue(probe.MARKER.encode() in transformed, "observer boundary marker missing")
        mutations = [(b"std::process::exit(86);", b"if false { std::process::exit(86); }"),
                     (b"    { // verification-only", b"    if false { // verification-only")]
        app, expected, fixture = self.artifacts()
        for before, after in mutations:
            with self.subTest(mutation=after):
                corrupted = transformed.replace(before, after, 1)
                self.assertNotEqual(corrupted, transformed)
                self.assertIn(probe.MARKER.encode(), corrupted)
                self.assertIn(b"std::process::exit(86)", corrupted)
                self.lib.write_bytes(corrupted)
                forged = {**receipt, "transformed_sha256": hashlib.sha256(corrupted).hexdigest()}
                (self.evidence / "ownership.json").write_text(json.dumps(forged))
                with patch.object(probe, "TRUSTED_FIXTURE_SHA256", hashlib.sha256(fixture.read_bytes()).hexdigest()):
                    with self.assertRaisesRegex(RuntimeError, "identity|observer"):
                        probe.verify_launch(self.mirror, self.evidence, app, expected, fixture)

    def test_environment_uses_explicit_empty_value_and_owned_windows_paths(self):
        environment = {"LIGHT_WHISPER_DATA_DIR": "private", "APPDATA": "private",
                       "LOCALAPPDATA": "private", "TEMP": "private", "TMP": "private"}
        actual = probe.probe_environment(environment, self.root, empty_data=True)
        self.assertEqual(actual["LIGHT_WHISPER_DATA_DIR"], "")
        for key in ("APPDATA", "LOCALAPPDATA", "TEMP", "TMP"):
            self.assertTrue(Path(actual[key]).is_relative_to(self.root))
            self.assertTrue(Path(actual[key]).is_dir())
        self.assertEqual(environment["LIGHT_WHISPER_DATA_DIR"], "private")

    def test_nonempty_environment_uses_owned_data_and_windows_paths(self):
        environment = {"LIGHT_WHISPER_DATA_DIR": "private", "APPDATA": "private"}
        actual = probe.probe_environment(environment, self.root, empty_data=False)
        for key in ("LIGHT_WHISPER_DATA_DIR", "APPDATA", "LOCALAPPDATA", "TEMP", "TMP"):
            self.assertTrue(actual[key])
            self.assertTrue(Path(actual[key]).is_absolute())
            self.assertTrue(Path(actual[key]).is_relative_to(self.root))
            self.assertTrue(Path(actual[key]).is_dir())
        self.assertEqual(environment["LIGHT_WHISPER_DATA_DIR"], "private")


class ProtocolAssertions(unittest.TestCase):
    """Unit counterexamples challenge probe assertions, never claim native execution."""
    def fake(self, response, exit_code=0):
        self.env = {"LIGHT_WHISPER_DATA_DIR": str(Path.cwd())}
        temporary = tempfile.TemporaryDirectory(prefix="lw-entrypoint-protocol-")
        self.addCleanup(temporary.cleanup)
        self.scratch = Path(temporary.name)
        self.data = self.scratch
        self.audit = lambda: []
        self.endpoint = self.scratch / "agent/endpoint.json"
        self.spawned = []
        case = self
        class Process:
            def __init__(self, owner, command, name, env=None):
                case.spawned.append((command, env or owner.env))
                self.child = SimpleNamespace(pid=19)
            def receive(self): return copy.deepcopy(response)
            def send(self, value): case.sent = value
            def wait(self, expected=0):
                case.assertEqual(exit_code, expected)
                return exit_code
        return SimpleNamespace(Process=Process, MARKER="LIGHT_WHISPER_AGENT_PROTOCOL_1")

    def test_client_empty_environment_requires_client_readiness(self):
        driver = self.fake({"success": True, "surface": "desktop-client",
                            "agent_protocol": "LIGHT_WHISPER_AGENT_PROTOCOL_1", "request_id": 181})
        probe.client_probe(self, driver, Path("owned.exe"))
        self.assertEqual(self.spawned[0][0], ["owned.exe", "--agent-client"])
        self.assertEqual(self.spawned[0][1]["LIGHT_WHISPER_DATA_DIR"], "")
        self.assertEqual(self.sent["action"], "exit")

    def test_client_guard_exit_and_wrong_route_are_assertion_failures(self):
        driver = self.fake({"success": True, "surface": "desktop-host"}, exit_code=2)
        with self.assertRaises(AssertionError):
            probe.client_probe(self, driver, Path("owned.exe"))

    def test_desktop_default_and_minimized_require_boundary_marker_and_exit(self):
        for argv in ([], ["--minimized"]):
            driver = self.fake({"entrypoint_test_only": probe.MARKER,
                                "surface": "gui-boundary-intercept", "pid": 19}, probe.EXIT)
            probe.desktop_probe(self, driver, Path("owned.exe"), argv)
            self.assertEqual(self.spawned[0][0], ["owned.exe", *argv])

    def test_host_readiness_and_noop_never_pass_for_desktop_boundary(self):
        for response in ({"surface": "desktop-host", "success": True, "window_count": 0}, {}):
            driver = self.fake(response)
            with self.assertRaises(AssertionError):
                probe.desktop_probe(self, driver, Path("owned.exe"), [])

    def test_unsupported_desktop_arguments_refused_before_launch(self):
        for arguments in (["--agent-host"], ["--agent-client"], ["--help"],
                          ["--minimized", "--agent-host"], ["--minimized", "--minimized"]):
            with self.subTest(arguments=arguments):
                driver = self.fake({"entrypoint_test_only": probe.MARKER,
                                    "surface": "gui-boundary-intercept", "pid": 19}, probe.EXIT)
                with self.assertRaises(ValueError):
                    probe.desktop_probe(self, driver, Path("owned.exe"), arguments)
                self.assertEqual(self.spawned, [])


if __name__ == "__main__":
    unittest.main()
