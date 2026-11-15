"""Safety contracts for the disposable native measurement workspace."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import prepare


class Preparation(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="lw-measurement-prepare-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.repo = self.root / "source"
        self.repo.mkdir()
        self.output = self.root / "measurement"
        self.files = {
            "src-tauri/src/agent_api/controller.rs": b"// synthetic source fixture\n",
            "src-tauri/Cargo.toml": b'[package]\nname="fixture-app"\nversion="0.1.0"\n',
            "src-tauri/Cargo.lock": b'version = 4\n',
            "formal/agent-api-measurements/native_measurement.rs": b"// bridge fixture\n",
        }
        for relative, data in self.files.items():
            path = self.repo / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        self.cohort = {
            "source": [{"path": name, "sha256": hashlib.sha256(data).hexdigest()}
                       for name, data in self.files.items() if name.endswith("controller.rs")]
        }

    def build(self, files=None):
        return prepare.prepare(self.repo, self.output, self.cohort,
                               list(self.files) if files is None else files)

    def test_changed_source_is_refused_before_output_creation(self):
        (self.repo / self.cohort["source"][0]["path"]).write_bytes(b"changed production\n")
        with self.assertRaisesRegex(RuntimeError, "source identity"):
            self.build()
        self.assertFalse(self.output.exists())

    def test_changed_frozen_test_is_refused_before_output_creation(self):
        relative = "formal/native_test.py"
        path = self.repo / relative
        path.write_bytes(b"frozen test")
        self.cohort["tests"] = {"native": {
            "path": relative, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}}
        path.write_bytes(b"changed assertion")
        with self.assertRaisesRegex(RuntimeError, "source identity"):
            self.build(list(self.files) + [relative])
        self.assertFalse(self.output.exists())

    def test_existing_output_is_preserved_and_refused(self):
        self.output.mkdir()
        marker = self.output / "keep"
        marker.write_bytes(b"existing owned evidence")
        with self.assertRaisesRegex(RuntimeError, "fresh"):
            self.build()
        self.assertEqual(marker.read_bytes(), b"existing owned evidence")
        self.assertEqual(list(self.output.iterdir()), [marker])

    def test_output_below_source_is_refused(self):
        self.output = self.repo / "new-measurement"
        with self.assertRaisesRegex(RuntimeError, "outside"):
            self.build()
        self.assertFalse(self.output.exists())

    @unittest.skipUnless(__import__("os").name == "nt", "Windows junction contract")
    def test_source_junction_cannot_copy_external_data(self):
        import _winapi

        outside = self.root / "outside"
        outside.mkdir()
        (outside / "private.txt").write_bytes(b"must remain outside mirror")
        junction = self.repo / "linked"
        _winapi.CreateJunction(str(outside), str(junction))
        self.addCleanup(junction.rmdir)
        with self.assertRaisesRegex(RuntimeError, "link|outside"):
            self.build(list(self.files) + ["linked/private.txt"])
        self.assertFalse(self.output.exists())

    @unittest.skipUnless(__import__("os").name == "nt", "Windows junction contract")
    def test_same_checkout_junction_is_refused_even_with_matching_frozen_bytes(self):
        import _winapi

        target = self.repo / "src-tauri/src/agent_api"
        junction = self.repo / "same-target"
        _winapi.CreateJunction(str(target), str(junction))
        self.addCleanup(junction.rmdir)
        relative = "same-target/controller.rs"
        expected = hashlib.sha256((self.repo / relative).read_bytes()).hexdigest()
        self.cohort["source"].append({"path": relative, "sha256": expected})
        self.assertTrue((self.repo / relative).resolve().is_relative_to(self.repo))
        with self.assertRaisesRegex(RuntimeError, "link|reparse"):
            self.build(list(self.files) + [relative])
        self.assertFalse(self.output.exists())

    def test_traversal_is_refused_before_output_creation(self):
        (self.root / "secret.txt").write_bytes(b"must not copy")
        with self.assertRaisesRegex(RuntimeError, "relative|outside"):
            self.build(list(self.files) + ["../secret.txt"])
        self.assertFalse(self.output.exists())

    def test_mirror_does_not_alias_source_or_change_shipping_manifest(self):
        (self.repo / "untracked-secret.txt").write_bytes(b"exclude local data")
        mirror = self.build()
        production = "src-tauri/src/agent_api/controller.rs"
        self.assertEqual((mirror / production).read_bytes(), self.files[production])
        (mirror / production).write_bytes(b"disposable source mutant")
        self.assertEqual((self.repo / production).read_bytes(), self.files[production])
        self.assertEqual((self.repo / "src-tauri/Cargo.toml").read_bytes(),
                         self.files["src-tauri/Cargo.toml"])
        self.assertFalse((mirror / "untracked-secret.txt").exists())
        self.assertFalse((self.repo / "Cargo.toml").exists())
        self.assertIn(b'[[test]]', (mirror / "src-tauri/Cargo.toml").read_bytes())
        self.assertEqual((mirror / "Cargo.lock").read_bytes(), self.files["src-tauri/Cargo.lock"])

    def test_same_frozen_bytes_under_different_head_are_reproducible(self):
        historical = "1" * 40
        observed = "2" * 40
        self.cohort["baseline"] = historical
        cohort_file = self.root / "cohort.json"
        cohort_file.write_text(json.dumps(self.cohort), encoding="utf-8")
        argv = ["prepare.py", "prepare", "--repo", str(self.repo),
                "--output", str(self.output), "--cohort", str(cohort_file)]
        with patch("sys.argv", argv), patch.object(prepare, "__file__", str(
                self.repo / "formal/agent-api-measurements/prepare.py")), \
                patch.object(prepare.subprocess, "check_output", side_effect=[
                    observed + "\n", ("\0".join(self.files) + "\0").encode()]), \
                patch.object(prepare.subprocess, "run") as git_run, patch("builtins.print"):
            prepare.main()
        receipt = json.loads((self.output / "ownership.json").read_text())
        self.assertEqual(receipt["baseline"], historical)
        self.assertEqual(receipt["observed_head"], observed)
        mirror = self.output / "repo"
        production = self.cohort["source"][0]
        self.assertEqual(hashlib.sha256((mirror / production["path"]).read_bytes()).hexdigest(),
                         production["sha256"])
        self.assertEqual(git_run.call_args_list[-2].args[0][-1], observed)
        self.assertEqual(git_run.call_args_list[-1].args[0][-1], observed)


def lock(version="1.0.0", checksum="abc", extra=""):
    return (f'version = 4\n[[package]]\nname = "dependency"\nversion = "{version}"\n'
            f'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
            f'checksum = "{checksum}"\n{extra}').encode()


class DependencyIdentity(unittest.TestCase):
    def test_changed_or_removed_registry_dependency_is_refused(self):
        for changed in (lock(version="2.0.0"), lock(checksum="different"), b"version=4\n"):
            with self.subTest(changed=changed), self.assertRaisesRegex(RuntimeError, "dependency identity"):
                prepare.verify_dependency_identity(lock(), changed)

    def test_unchanged_registry_dependency_and_new_workspace_member_are_accepted(self):
        prepare.verify_dependency_identity(
            lock(), lock(extra='[[package]]\nname="new-local-member"\nversion="0.1.0"\n'))


if __name__ == "__main__":
    unittest.main()
