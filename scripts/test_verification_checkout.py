"""Real Git checkouts must retain bytes bound by active verification pins."""

import ast
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]


def source_pin(relative):
    """Read a driver's literal binding without importing its executable code."""
    tree = ast.parse((ROOT / relative).read_text(encoding="utf-8"))
    for node in tree.body:
        if isinstance(node, ast.Assign) and any(
            isinstance(target, ast.Name) and target.id == "SOURCE_SHA256"
            for target in node.targets
        ):
            return ast.literal_eval(node.value)
    raise AssertionError(f"SOURCE_SHA256 missing from {relative}")


def verification_pins():
    pins = {
        "scripts/light_whisper_cli.py": source_pin("formal/cli/json-numbers/check_model.py"),
    }
    reviewed = json.loads(
        (ROOT / "formal/agent-api-measurements/entrypoint/reviewed.json").read_text(
            encoding="utf-8"
        )
    )["frozen"]
    admission = json.loads(
        (ROOT / "formal/agent-api-host/admission/source-bindings.json").read_text(
            encoding="utf-8"
        )
    )
    cohort = json.loads(
        (ROOT / "formal/agent-api-measurements/cohort.json").read_text(encoding="utf-8")
    )
    cohort_bindings = {entry["path"]: entry["sha256"] for entry in cohort["source"]}
    for entry in cohort["tests"].values():
        cohort_bindings[entry["path"]] = entry["sha256"]
    for bindings in (reviewed, admission, cohort_bindings):
        for relative, expected in bindings.items():
            if relative in pins and pins[relative] != expected:
                raise AssertionError(f"Conflicting active verification pins: {relative}")
            pins[relative] = expected
    return pins


class VerificationCheckoutTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.pins = verification_pins()
        cls.original = {relative: (ROOT / relative).read_bytes() for relative in cls.pins}
        for relative, expected in cls.pins.items():
            actual = hashlib.sha256(cls.original[relative]).hexdigest()
            if actual != expected:
                raise AssertionError(
                    f"Current source differs from its active pin before checkout: "
                    f"{relative}: {actual} != {expected}"
                )

        temporary = tempfile.TemporaryDirectory(prefix="light-whisper-checkout-pins-")
        cls.addClassCleanup(temporary.cleanup)
        cls.temporary = Path(temporary.name)
        cls.publisher = cls.temporary / "publisher"
        cls.publisher.mkdir()
        cls.git_environment = os.environ.copy()
        cls.git_environment.update(
            GIT_CONFIG_GLOBAL=os.devnull,
            GIT_CONFIG_SYSTEM=os.devnull,
            GIT_ATTR_NOSYSTEM="1",
        )
        cls.git("init", "--quiet")
        (cls.publisher / ".gitattributes").write_bytes((ROOT / ".gitattributes").read_bytes())
        for relative, content in cls.original.items():
            destination = cls.publisher / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(content)
        # A Windows publication cleans text to LF in Git's index. Staging with
        # autocrlf=false could retain CRLF blobs and conceal a Linux checkout bug.
        cls.git("add", "--", ".gitattributes", *sorted(cls.pins), autocrlf="true")

    @classmethod
    def git(cls, *arguments, autocrlf="false", eol="lf"):
        return subprocess.run(
            [
                "git", "-c", f"core.autocrlf={autocrlf}", "-c", f"core.eol={eol}",
                "-c", "core.safecrlf=false", "-c", f"core.attributesFile={os.devnull}",
                *arguments,
            ],
            cwd=cls.publisher,
            env=cls.git_environment,
            capture_output=True,
            check=True,
            timeout=15,
        )

    def test_current_sources_match_active_verification_pins(self):
        for relative, expected in sorted(self.pins.items()):
            with self.subTest(path=relative):
                self.assertEqual(hashlib.sha256(self.original[relative]).hexdigest(), expected)

    def assert_checkout_matches_pins(self, autocrlf, eol):
        checkout = self.temporary / f"checkout-{autocrlf}-{eol}"
        checkout.mkdir()
        self.git(
            "checkout-index", "--all", f"--prefix={checkout.as_posix()}/",
            autocrlf=autocrlf, eol=eol,
        )
        for relative, expected in sorted(self.pins.items()):
            with self.subTest(path=relative, autocrlf=autocrlf, eol=eol):
                self.assertEqual(
                    hashlib.sha256((checkout / relative).read_bytes()).hexdigest(),
                    expected,
                    f"Git checkout changed bytes required by an active verification pin: {relative}",
                )

    def test_lf_checkout_retains_verification_identity(self):
        self.assert_checkout_matches_pins(autocrlf="false", eol="lf")

    def test_crlf_checkout_retains_verification_identity(self):
        self.assert_checkout_matches_pins(autocrlf="true", eol="crlf")


if __name__ == "__main__":
    unittest.main()
