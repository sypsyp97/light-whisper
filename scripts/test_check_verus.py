"""Runner acceptance only: fake Verus output is never mathematical proof evidence."""

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest


ROOT = Path(__file__).resolve().parents[1]
RUNNER = Path("scripts/check_verus.py")
DOMAINS = {
    "capture_bounds": Path("src-tauri/src/services/audio_service/capture_bounds.rs"),
    "hotkey_ownership": Path("src-tauri/src/commands/hotkey/start_owner.rs"),
    "interim_interval": Path("src-tauri/src/services/audio_service/interim.rs"),
}
TEMPLATES = {name: Path(f"formal/verus/{name}.template.rs") for name in DOMAINS}
CONSTANTS = Path("src-tauri/src/services/audio_service/mod.rs")
# The pinned verifier counts seven interval constant initializers plus the controller.
PROOF_COUNTS = {"capture_bounds": 1, "hotkey_ownership": 4, "interim_interval": 8}
INTERVAL_CONSTANTS = {
    "INTERIM_INTERVAL_MIN_MS": 140,
    "INTERIM_INTERVAL_BASE_MS": 220,
    "INTERIM_INTERVAL_MAX_MS": 460,
    "INTERIM_INTERVAL_DOWN_STEP_MS": 24,
    "INTERIM_INTERVAL_UP_STEP_MS": 42,
    "INTERIM_HEAVY_COST_MS": 420,
    "INTERIM_LIGHT_COST_MS": 180,
}


def records(value):
    """Inspect evidence without prescribing the runner's nested summary layout."""
    if isinstance(value, dict):
        yield value
        for child in value.values():
            yield from records(child)
    elif isinstance(value, list):
        for child in value:
            yield from records(child)


class VerusRunnerAcceptance(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        required = (RUNNER, *DOMAINS.values(), CONSTANTS, TEMPLATES["capture_bounds"])
        missing = [str(path) for path in required if not (ROOT / path).is_file()]
        if missing:
            raise AssertionError("INTERFACE_PENDING: " + ", ".join(missing))

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="verus-acceptance-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.original = {}
        for relative in (RUNNER, *DOMAINS.values(), CONSTANTS, *TEMPLATES.values()):
            # New templates are intentionally absent during the initial RED run.
            # Required outputs/interfaces are asserted below, never skipped.
            if not (ROOT / relative).is_file():
                continue
            target = self.root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            original = (ROOT / relative).read_bytes()
            target.write_bytes(original)
            self.original[relative] = original.decode("utf-8")
        self.invocation = 0
        fake = self.root / "fake_verus.py"
        fake.write_text(textwrap.dedent('''\
            import json
            import os
            from pathlib import Path
            import sys

            args = sys.argv[1:]
            if "--version" in args:
                print("Verus Version: 0.2026.10.18.7d82994; toolchain: 1.99.1")
                raise SystemExit(0)
            source = next((Path(arg) for arg in args if arg.endswith(".rs")), None)
            if source is None:
                raise SystemExit(2)
            if not source.is_absolute():
                source = Path.cwd() / source
            domain = source.parent.name
            with open(os.environ["FAKE_VERUS_CALLS"], "a", encoding="utf-8") as stream:
                stream.write(json.dumps({"args": args, "name": source.name,
                                         "domain": domain, "path": str(source),
                                         "source": source.read_bytes().decode("utf-8")}) + "\\n")
            mode = os.environ.get("FAKE_VERUS_MODE", "normal")
            if os.environ.get("FAKE_VERUS_DOMAIN", "*") not in ("*", domain):
                mode = "normal"
            positive = source.name == "positive.rs"
            if "--no-verify" in args:
                if mode == "typecheck_failure" and not positive:
                    print("error[E0308]: mismatched types", file=sys.stderr)
                    raise SystemExit(1)
                raise SystemExit(0)
            if positive:
                if mode == "silent_positive":
                    raise SystemExit(0)
                count = {"capture_bounds": 1, "hotkey_ownership": 4, "interim_interval": 8}.get(domain, 1)
                if mode == "zero_proved":
                    count = 0
                if mode == "partial_positive":
                    count -= 1
                if mode == "extra_positive":
                    count += 1
                print(f"verification results:: {count} verified, 0 errors")
                raise SystemExit(1 if mode == "nonzero_positive" else 0)
            if mode == "accepted_negative":
                print("verification results:: 1 verified, 0 errors")
                raise SystemExit(0)
            if mode == "syntax_negative":
                print("error: expected expression, found `}`", file=sys.stderr)
                raise SystemExit(1)
            if mode == "silent_negative":
                raise SystemExit(1)
            if mode != "no_semantic_diagnostic":
                print("error: postcondition not satisfied", file=sys.stderr)
            errors = 0 if mode == "zero_errors_negative" else 1
            print(f"verification results:: 0 verified, {errors} errors")
            raise SystemExit({"negative_exit_2": 2, "negative_exit_minus_9": -9}.get(mode, 1))
            '''), encoding="utf-8")
        if os.name == "nt":
            self.verus = self.root / "fake-verus.cmd"
            self.verus.write_text(f'@"{sys.executable}" "{fake}" %*\r\n', encoding="utf-8")
        else:
            self.verus = fake
            fake.write_text(f"#!{sys.executable}\n" + fake.read_text(encoding="utf-8"), encoding="utf-8")
            fake.chmod(0o700)

    def run_gate(self, mode="normal", verus=None, domain="*"):
        self.invocation += 1
        # Exercise path aliases on every platform, even without Windows 8.3 names.
        self.logs = self.root / ".." / self.root.name / f"logs-{self.invocation}"
        self.calls = self.root / f"calls-{self.invocation}.jsonl"
        environment = os.environ.copy()
        environment.update(FAKE_VERUS_CALLS=str(self.calls), FAKE_VERUS_MODE=mode,
                           FAKE_VERUS_DOMAIN=domain)
        command = [sys.executable, str(self.root / RUNNER), "--verus", str(verus or self.verus),
                   "--logs", str(self.logs)]
        result = subprocess.run(command, cwd=self.root, env=environment,
                                capture_output=True, text=True, timeout=45)
        evidence_root = os.environ.get("VERUS_ACCEPTANCE_EVIDENCE")
        if evidence_root:
            destination = Path(evidence_root) / self._testMethodName / str(self.invocation)
            destination.mkdir(parents=True, exist_ok=True)
            if self.logs.is_dir():
                shutil.copytree(self.logs, destination / "logs", dirs_exist_ok=True)
            if self.calls.is_file():
                shutil.copyfile(self.calls, destination / "calls.jsonl")
            (destination / "result.json").write_text(json.dumps({
                "command": command, "mode": mode, "domain": domain,
                "returncode": result.returncode, "stdout": result.stdout, "stderr": result.stderr,
            }, indent=2) + "\n", encoding="utf-8")
        return result

    def recorded_calls(self):
        self.assertTrue(self.calls.is_file(), "the verifier must receive generated programs")
        return [json.loads(line) for line in self.calls.read_text(encoding="utf-8").splitlines()]

    def production_block(self, text, pattern, label):
        matches = list(re.finditer(pattern, text))
        self.assertEqual(len(matches), 1, f"INTERFACE_PENDING: unique {label}")
        start = matches[0].start()
        opening = text.index("{", matches[0].end())
        depth = 1
        end = opening + 1
        while end < len(text) and depth:
            depth += (text[end] == "{") - (text[end] == "}")
            end += 1
        self.assertEqual(depth, 0, f"unbalanced production {label}")
        return text[start:end], text[opening + 1:end - 1]

    def production_bodies(self, domain):
        source = self.original[DOMAINS[domain]].split("#[cfg(test)]", 1)[0]
        if domain == "hotkey_ownership":
            _, source = self.production_block(source, r"\bimpl\s+StartOwnership\s*(?=\{)",
                                               "impl StartOwnership")
            names = ("begin", "bind", "release", "fail")
        else:
            names = ("capped_frame_count" if domain == "capture_bounds" else "adjust_interval",)
        return {name: self.production_block(source, rf"\bfn\s+{name}\s*\(", name)[1]
                for name in names}

    def successful_proofs(self):
        result = self.run_gate()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        calls = self.recorded_calls()
        proofs = [call for call in calls if "--no-verify" not in call["args"]]
        # This assertion is semantic RED on the old, otherwise successful CLI.
        self.assertEqual({call["domain"] for call in proofs}, set(DOMAINS),
                         "default CLI must verify exactly the three production domains")
        return calls, proofs

    def test_default_checks_all_domains_and_typechecks_each_distinct_control(self):
        calls, proofs = self.successful_proofs()
        minimum_negatives = {"capture_bounds": 3, "hotkey_ownership": 3, "interim_interval": 2}
        for domain, minimum in minimum_negatives.items():
            with self.subTest(domain=domain):
                programs = [call for call in proofs if call["domain"] == domain]
                names = [call["name"] for call in programs]
                self.assertEqual(names.count("positive.rs"), 1)
                negatives = set(names) - {"positive.rs"}
                self.assertGreaterEqual(len(negatives), minimum)
                if domain == "capture_bounds":
                    self.assertEqual(len(negatives), 3)
                self.assertTrue(all(name.startswith("negative-") for name in negatives))
                self.assertEqual(len(set(names)), len(names), "verify every program once")
                self.assertEqual(len({call["source"] for call in programs}), len(programs))
                for proof in programs:
                    checked = [call for call in calls if "--no-verify" in call["args"]
                               and (call["domain"], call["name"]) == (domain, proof["name"])]
                    self.assertEqual(len(checked), 1)
                    self.assertEqual(checked[0]["source"], proof["source"])
                    self.assertLess(calls.index(checked[0]), calls.index(proof))
                    self.assertEqual(Path(proof["path"]).resolve(),
                                     (self.logs / domain / proof["name"]).resolve())
                    self.assertEqual(Path(proof["path"]).read_bytes().decode("utf-8"), proof["source"])
                checked_names = {call["name"] for call in calls if call["domain"] == domain
                                 and "--no-verify" in call["args"]}
                self.assertEqual(checked_names, set(names))

    def test_production_bodies_struct_and_constants_are_verbatim_and_hashed(self):
        calls, proofs = self.successful_proofs()
        positives = {call["domain"]: call["source"] for call in proofs if call["name"] == "positive.rs"}
        for domain in DOMAINS:
            for name, body in self.production_bodies(domain).items():
                with self.subTest(domain=domain, function=name):
                    self.assertIn(body, positives[domain], "copy production body bytes verbatim")
            self.assertNotRegex(positives[domain], r"/\*__\w+__\*/")
        structure, _ = self.production_block(self.original[DOMAINS["hotkey_ownership"]],
                                             r"\bstruct\s+StartOwnership\s*(?=\{)", "StartOwnership")
        self.assertIn(structure, positives["hotkey_ownership"])
        for name, value in INTERVAL_CONSTANTS.items():
            declaration = re.search(rf"(?m)^(?:pub\(crate\)\s+)?const\s+{name}:\s*u64\s*=\s*{value};",
                                    self.original[CONSTANTS])
            self.assertIsNotNone(declaration, f"production constant changed: {name}")
            self.assertIn(declaration[0], positives["interim_interval"])
        summary_paths = list(self.logs.rglob("*.json"))
        self.assertTrue(summary_paths, "retain machine-readable evidence")
        summaries = [json.loads(path.read_text(encoding="utf-8")) for path in summary_paths]
        all_records = list(records(summaries))
        hashed_files = {str((self.root / relative).resolve())
                        for relative in (*DOMAINS.values(), CONSTANTS, *TEMPLATES.values())}
        hashed_files.update(call["path"] for call in proofs)
        for filename in hashed_files:
            digest = hashlib.sha256(Path(filename).read_bytes()).hexdigest()
            self.assertTrue(any(record.get("path") == filename and record.get("sha256") == digest
                                for record in all_records), f"missing source/program hash: {filename}")
        for call in calls:
            matching = [record for record in all_records if isinstance(record.get("command"), list)
                        and record["command"][1:] == call["args"]]
            self.assertEqual(len(matching), 1, f"retain every invocation: {call['domain']}/{call['name']}")
            record = matching[0]
            self.assertIn("exit_code", record)
            for stream in ("stdout", "stderr"):
                self.assertTrue(Path(record[stream]).is_file())
                self.assertEqual(Path(record[stream]).resolve().parent,
                                 (self.logs / call["domain"]).resolve())
            if call["name"] == "positive.rs" and "--no-verify" not in call["args"]:
                self.assertEqual(record.get("verified"), PROOF_COUNTS[call["domain"]])

    def test_required_controls_mutate_the_intended_production_body(self):
        _, proofs = self.successful_proofs()
        for domain in DOMAINS:
            programs = [call["source"] for call in proofs if call["domain"] == domain]
            positive = next(call["source"] for call in proofs
                            if call["domain"] == domain and call["name"] == "positive.rs")
            negatives = [source for source in programs if source != positive]
            bodies = self.production_bodies(domain)
            if domain == "capture_bounds":
                for anchor, replacement in (("channels.max(1)", "channels"),
                                            ("frame_count.min(allowed_mono_samples)", "frame_count"),
                                            ("cap.saturating_sub(used)", "(cap - used)")):
                    self.assertIn(positive.replace(anchor, replacement, 1), negatives)
            elif domain == "interim_interval":
                for anchor in (".min(INTERIM_INTERVAL_MAX_MS)", ".max(INTERIM_INTERVAL_MIN_MS)"):
                    self.assertIn(positive.replace(anchor, "", 1), negatives)
            else:
                for anchor in ("self.generation != generation", "self.session_id.is_some()"):
                    self.assertTrue(any(source.count(anchor) == positive.count(anchor) - 1
                                        and bodies["bind"] not in source
                                        and all(bodies[name] in source for name in ("begin", "release", "fail"))
                                        for source in negatives), f"missing isolated bind control: {anchor}")
                self.assertTrue(any(bodies["begin"] not in source
                                    and "wrapping_add(1)" in source
                                    and all(bodies[name] in source for name in ("bind", "release", "fail"))
                                    for source in negatives), "missing isolated generation-overflow control")

    def test_missing_verifier_fails_closed(self):
        result = self.run_gate(verus=self.root / "missing-verus")
        self.assertNotEqual(result.returncode, 0)

    def test_positive_requires_successful_exit_and_full_proof_count_in_every_domain(self):
        cases = [(domain, mode) for domain in DOMAINS
                 for mode in ("zero_proved", "silent_positive", "nonzero_positive", "extra_positive")]
        cases.extend((domain, "partial_positive") for domain in ("hotkey_ownership", "interim_interval"))
        for domain, mode in cases:
            with self.subTest(domain=domain, mode=mode):
                result = self.run_gate(mode, domain=domain)
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_negative_requires_typechecking_and_actual_verification_failure_in_every_domain(self):
        for domain in DOMAINS:
            for mode in ("typecheck_failure", "accepted_negative", "syntax_negative", "silent_negative",
                         "no_semantic_diagnostic", "zero_errors_negative"):
                with self.subTest(domain=domain, mode=mode):
                    result = self.run_gate(mode, domain=domain)
                    self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_negative_rejects_abnormal_exit_even_with_verification_errors_in_every_domain(self):
        for domain in DOMAINS:
            for mode in ("negative_exit_2", "negative_exit_minus_9"):
                with self.subTest(domain=domain, mode=mode):
                    result = self.run_gate(mode, domain=domain)
                    self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_missing_source_template_or_constants_fails_closed(self):
        for relative in (*DOMAINS.values(), CONSTANTS, *TEMPLATES.values()):
            with self.subTest(path=str(relative)):
                path = self.root / relative
                path.unlink(missing_ok=True)
                try:
                    result = self.run_gate()
                    self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                finally:
                    if relative in self.original:
                        path.write_bytes(self.original[relative].encode("utf-8"))

    def test_template_body_anchors_are_unique_and_required(self):
        for domain, relative in TEMPLATES.items():
            with self.subTest(domain=domain):
                self.assertTrue(relative in self.original, f"INTERFACE_PENDING: {relative}")
                original = self.original[relative]
                tokens = re.findall(r"/\*__\w+__\*/", original)
                self.assertTrue(tokens, f"INTERFACE_PENDING: production anchors in {relative}")
                for token in set(tokens):
                    for replacement in ("", token + token):
                        with self.subTest(token=token, replacement=replacement):
                            (self.root / relative).write_bytes(original.replace(token, replacement).encode("utf-8"))
                            result = self.run_gate()
                            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                (self.root / relative).write_bytes(original.encode("utf-8"))

    def test_source_signature_duplicate_or_mutation_anchor_drift_fails_closed(self):
        changes = {
            "capture_bounds": (("channels: usize", "channel_count: usize"),
                               ("channels.max(1)", "channels.max(2)")),
            "hotkey_ownership": (("generation: u64", "generation: u32"),
                                 ("fn bind(", "fn renamed_bind("),
                                 ("session_id: Option<u64>", "session_id: u64")),
            "interim_interval": (("current: u64", "current: u32"),
                                 ("fn adjust_interval(", "fn renamed_adjust_interval("),
                                 (".min(INTERIM_INTERVAL_MAX_MS)", ".min(INTERIM_INTERVAL_BASE_MS)")),
        }
        for domain, edits in changes.items():
            relative = DOMAINS[domain]
            original = self.original[relative]
            variants = [original.replace(before, after) for before, after in edits]
            # Duplicate before the test boundary, not hidden behind #[cfg(test)].
            production, separator, tests = original.partition("#[cfg(test)]")
            variants.append(production + production + separator + tests)
            for index, source in enumerate(variants):
                with self.subTest(domain=domain, variant=index):
                    self.assertNotEqual(source, original, "fixture must actually change source")
                    (self.root / relative).write_bytes(source.encode("utf-8"))
                    result = self.run_gate()
                    self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
            (self.root / relative).write_bytes(original.encode("utf-8"))

    def test_production_constant_drift_or_duplicate_fails_closed(self):
        original = self.original[CONSTANTS]
        for name, value in INTERVAL_CONSTANTS.items():
            declaration = re.search(rf"(?m)^pub\(crate\) const {name}: u64 = {value};", original)
            self.assertIsNotNone(declaration, name)
            for replacement in (declaration[0].replace(f"= {value};", f"= {value + 1};"),
                                declaration[0] + "\n" + declaration[0]):
                with self.subTest(constant=name, replacement=replacement):
                    (self.root / CONSTANTS).write_bytes(original.replace(declaration[0], replacement).encode("utf-8"))
                    result = self.run_gate()
                    self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        (self.root / CONSTANTS).write_bytes(original.encode("utf-8"))

    def test_cfg_alias_and_local_shadow_context_fails_closed(self):
        for domain in ("hotkey_ownership", "interim_interval"):
            relative = DOMAINS[domain]
            original = self.original[relative]
            if domain == "hotkey_ownership":
                core_impl = "impl StartOwnership" if "impl StartOwnership" in original else "impl HotkeyStartOwner"
                variants = [
                    original.replace(core_impl, "#[cfg(any())]\n" + core_impl, 1),
                    original.replace("struct StartOwnership", "#[cfg(any())]\nstruct StartOwnership", 1),
                    "type u64 = u32;\n" + original,
                    "use std::primitive::u32 as u64;\n" + original,
                    "type Option<T> = Result<T, ()>;\n" + original,
                ]
            else:
                variants = [
                    original.replace("fn adjust_interval(", "#[cfg(any())]\nfn adjust_interval(", 1),
                    "type u64 = u32;\n" + original,
                    "mod std { pub use ::std::*; }\n" + original,
                    original.replace("if !executed {", "const INTERIM_INTERVAL_MAX_MS: u64 = 1;\n    if !executed {", 1),
                ]
            for index, source in enumerate(variants):
                with self.subTest(domain=domain, variant=index):
                    self.assertNotEqual(source, original)
                    (self.root / relative).write_bytes(source.encode("utf-8"))
                    result = self.run_gate()
                    self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
            (self.root / relative).write_bytes(original.encode("utf-8"))
        original = self.original[CONSTANTS]
        for source in (original.replace("pub(crate) const INTERIM_INTERVAL_MAX_MS",
                                        "#[cfg(any())]\npub(crate) const INTERIM_INTERVAL_MAX_MS", 1),
                       "type u64 = u32;\n" + original):
            with self.subTest(constants_context=source[:80]):
                (self.root / CONSTANTS).write_bytes(source.encode("utf-8"))
                result = self.run_gate()
                self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        (self.root / CONSTANTS).write_bytes(original.encode("utf-8"))

    def test_trust_bypass_code_is_rejected_before_verifier_execution(self):
        for relative in (*DOMAINS.values(), CONSTANTS, *TEMPLATES.values()):
            with self.subTest(path=str(relative)):
                self.assertTrue(relative in self.original, f"INTERFACE_PENDING: {relative}")
                original = self.original[relative]
                # Prefixes remain inside the production scope, before #[cfg(test)].
                for escape in ("#[verifier::external_body]", "#[verifier::external]", "#[verifier::axiom]",
                               "assume(false);", "admit();",
                               "assume_specification[core::mem::replace::<u64>](dst: &mut u64, src: u64) -> u64;"):
                    with self.subTest(escape=escape):
                        (self.root / relative).write_bytes((escape + "\n" + original).encode("utf-8"))
                        result = self.run_gate()
                        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
                        self.assertFalse(self.calls.exists(), "trust bypass must fail before running a proof")
                (self.root / relative).write_bytes(original.encode("utf-8"))


if __name__ == "__main__":
    unittest.main()
