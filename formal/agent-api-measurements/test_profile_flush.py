"""Pure metadata/filesystem contracts; no app processes or compiler invocation."""

from __future__ import annotations

import copy
import hashlib
import json
import os
from pathlib import Path
import tempfile
import unittest

import profile_flush


HOST = "src-tauri/src/agent_api/host.rs"
FROZEN = json.loads(Path(__file__).with_name("cohort.json").read_text(encoding="utf-8"))
CHECKOUT = Path(__file__).resolve().parents[2]


class ProfileFlush(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="lw-flush-helper-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / "shipping"
        self.source.mkdir()
        self.mirror = self.root / "measurement/repo"
        self.mirror.mkdir(parents=True)
        self.cohort = copy.deepcopy(FROZEN)
        self.entries = [*self.cohort["source"], *self.cohort["tests"].values()]
        for entry in self.entries:
            for root in (self.source, self.mirror):
                path = root / entry["path"]
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes((CHECKOUT / entry["path"]).read_bytes())
        self.host = self.mirror / HOST
        self.original = self.host.read_bytes()
        self.owner = self.mirror.parent / "ownership.json"
        self.owner.write_text(json.dumps({
            "kind": "native-agent-api-measurement", "source_repo": str(self.source),
            "mirror": str(self.mirror), "observed_head": "2" * 40,
            "copied_source_sha256": {e["path"]: e["sha256"] for e in self.entries},
        }), encoding="utf-8")
        self.target = self.root / "coverage-target"
        self.target.mkdir()
        (self.target / "ownership.json").write_text(json.dumps({
            "kind": "direct-instrumentation-target", "mirror": str(self.mirror),
        }), encoding="utf-8")
        self.profiles = self.target / "fresh-flush"
        self.environment = {
            "CARGO_TARGET_DIR": str(self.target), "CARGO_LLVM_COV_TARGET_DIR": str(self.target),
            "LLVM_PROFILE_FILE": str(self.profiles / "flush-%p-%m.profraw"),
            "RUSTFLAGS": "-C instrument-coverage -Z coverage-options=branch --cfg=coverage --cfg=coverage_nightly",
        }

    def apply(self):
        return profile_flush.apply_overlay(self.mirror, self.cohort, self.profiles, self.environment)

    def refused(self, pattern):
        with self.assertRaisesRegex(RuntimeError, pattern):
            self.apply()
        self.assertEqual(self.host.read_bytes(), self.original)
        self.assertFalse(self.profiles.exists())

    def test_overlay_preserves_original_bytes_and_maps_only_eleven_inserted_lines(self):
        receipt = self.apply()
        generated = self.host.read_bytes().splitlines(keepends=True)
        original = self.original.splitlines(keepends=True)
        self.assertEqual(len(generated), len(original) + 11)
        for line, expected in enumerate(original, 1):
            actual = line if line < 75 else line + 11
            self.assertEqual(generated[actual - 1], expected)
            self.assertEqual(profile_flush.original_line(actual), line)
        for line in range(75, 86):
            self.assertIsNone(profile_flush.original_line(line))
        hook = b"".join(generated[74:85])
        self.assertIn(b"#[cfg(coverage)]", hook)
        self.assertIn(b"__llvm_profile_write_file()", hook)
        self.assertIn(b"// SAFETY:", hook)
        self.assertLess(b"".join(generated).index(b"stop_server(state.inner())"),
                        b"".join(generated).index(b"__llvm_profile_write_file()"))
        self.assertEqual(receipt["original_host_sha256"], hashlib.sha256(self.original).hexdigest())
        self.assertEqual(receipt["transformed_host_sha256"], hashlib.sha256(self.host.read_bytes()).hexdigest())
        self.assertNotEqual(receipt["original_host_sha256"], receipt["transformed_host_sha256"])
        self.assertEqual(receipt["frozen_source_sha256"], {e["path"]: e["sha256"] for e in self.cohort["source"]})
        self.assertEqual((self.profiles / "host.rs.original").read_bytes(), self.original)
        self.assertIn("__llvm_profile_write_file", (self.profiles / "overlay.diff").read_text())
        self.assertEqual((self.source / HOST).read_bytes(), self.original)
        self.assertEqual(json.loads((self.profiles / "ownership.json").read_text()), receipt)

    def test_missing_or_mismatched_mirror_ownership_is_refused(self):
        self.owner.unlink()
        self.refused("ownership")
        self.owner.write_text(json.dumps({"kind": "other", "mirror": str(self.mirror)}))
        self.refused("ownership")

    def test_original_checkout_cannot_be_used_as_mirror(self):
        data = json.loads(self.owner.read_text())
        data["source_repo"] = str(self.mirror)
        self.owner.write_text(json.dumps(data))
        self.refused("outside|shipping|source")

    def test_hardlinked_host_cannot_modify_shipping_source(self):
        self.host.unlink()
        os.link(self.source / HOST, self.host)
        self.assertTrue(self.host.samefile(self.source / HOST))
        self.refused("shared|hardlink|identity")
        self.assertEqual((self.source / HOST).read_bytes(), self.original)

    def test_changed_frozen_source_and_frozen_test_are_refused(self):
        for entry in (self.cohort["source"][0], self.cohort["tests"]["native20"]):
            with self.subTest(path=entry["path"]):
                path = self.mirror / entry["path"]
                before = path.read_bytes()
                path.write_bytes(before + b"changed\n")
                self.refused("identity")
                path.write_bytes(before)

    def test_changed_or_duplicated_shutdown_anchor_is_refused_even_with_updated_hash(self):
        for data in (self.original.replace(b"shutdown(app);", b"other_shutdown(app);"),
                     self.original + self.original):
            with self.subTest(data_length=len(data)):
                self.host.write_bytes(data)
                self.original = data
                entry = next(e for e in self.cohort["source"] if e["path"] == HOST)
                entry["sha256"] = hashlib.sha256(data).hexdigest()
                owner = json.loads(self.owner.read_text())
                owner["copied_source_sha256"][HOST] = entry["sha256"]
                self.owner.write_text(json.dumps(owner))
                self.refused("anchor")

    def test_existing_profile_directory_is_preserved_and_refused(self):
        self.profiles.mkdir()
        marker = self.profiles / "keep.profraw"
        marker.write_bytes(b"old profile must never merge")
        with self.assertRaisesRegex(RuntimeError, "fresh"):
            self.apply()
        self.assertEqual(marker.read_bytes(), b"old profile must never merge")
        self.assertEqual(self.host.read_bytes(), self.original)

    def test_profile_template_must_match_fresh_owned_external_target(self):
        original = self.environment.copy()
        for variable, value in (("LLVM_PROFILE_FILE", str(self.target / "old-%p.profraw")),
                                ("CARGO_TARGET_DIR", str(self.root / "different"))):
            with self.subTest(variable=variable):
                self.environment = original | {variable: value}
                self.refused("profile|target")
        self.environment = original
        (self.target / "ownership.json").unlink()
        self.refused("ownership")

    def test_profile_destination_inside_source_is_refused(self):
        self.target = self.mirror / "target"
        self.target.mkdir()
        (self.target / "ownership.json").write_text(json.dumps({
            "kind": "direct-instrumentation-target", "mirror": str(self.mirror)}))
        self.profiles = self.target / "fresh"
        self.environment.update(CARGO_TARGET_DIR=str(self.target), CARGO_LLVM_COV_TARGET_DIR=str(self.target),
                                LLVM_PROFILE_FILE=str(self.profiles / "flush-%p-%m.profraw"))
        self.refused("outside|external")

    def test_uninstrumented_flags_are_refused(self):
        for flags in ("", "-C instrument-coverage", "--cfg=coverage"):
            with self.subTest(flags=flags):
                self.environment["RUSTFLAGS"] = flags
                self.refused("coverage")

    @unittest.skipUnless(os.name == "nt", "Windows reparse boundary")
    def test_same_target_junction_profile_ancestor_is_refused(self):
        import _winapi

        linked = self.root / "linked-target"
        _winapi.CreateJunction(str(self.target), str(linked))
        self.addCleanup(linked.rmdir)
        self.profiles = linked / "fresh-flush"
        self.environment.update(CARGO_TARGET_DIR=str(linked), CARGO_LLVM_COV_TARGET_DIR=str(linked),
                                LLVM_PROFILE_FILE=str(self.profiles / "flush-%p-%m.profraw"))
        self.refused("link|reparse")

    @unittest.skipUnless(os.name == "nt", "Windows reparse boundary")
    def test_same_mirror_junction_is_refused_before_resolving_ownership(self):
        import _winapi

        linked = self.root / "linked-mirror"
        _winapi.CreateJunction(str(self.mirror), str(linked))
        self.addCleanup(linked.rmdir)
        self.mirror = linked
        self.refused("link|reparse")

    def test_second_overlay_cannot_overwrite_original_receipt(self):
        self.apply()
        transformed = self.host.read_bytes()
        backup = (self.profiles / "host.rs.original").read_bytes()
        with self.assertRaisesRegex(RuntimeError, "fresh|identity"):
            self.apply()
        self.assertEqual(self.host.read_bytes(), transformed)
        self.assertEqual((self.profiles / "host.rs.original").read_bytes(), backup)

    def test_restore_is_exact_and_does_not_overwrite_later_edits(self):
        self.apply()
        transformed = self.host.read_bytes()
        self.host.write_bytes(transformed + b"later edit\n")
        with self.assertRaisesRegex(RuntimeError, "identity"):
            profile_flush.restore(self.mirror, self.profiles)
        self.assertTrue(self.host.read_bytes().endswith(b"later edit\n"))
        self.host.write_bytes(transformed)
        profile_flush.restore(self.mirror, self.profiles)
        self.assertEqual(self.host.read_bytes(), self.original)
        self.assertTrue((self.profiles / "ownership.json").is_file())

    def test_flush_failure_marker_invalidates_measurement(self):
        evidence = self.root / "native-evidence"
        evidence.mkdir()
        with self.assertRaisesRegex(RuntimeError, "stderr|logs"):
            profile_flush.check_logs(evidence)
        (evidence / "host-1-stderr.txt").write_bytes(b"ordinary diagnostic\n")
        self.assertEqual(profile_flush.check_logs(evidence), 1)
        (evidence / "host-2-stderr.txt").write_bytes(b"Native coverage flush failed: 1\n")
        with self.assertRaisesRegex(RuntimeError, "flush failed"):
            profile_flush.check_logs(evidence)


if __name__ == "__main__":
    unittest.main()
