"""Protect CLI CI/release wiring; runtime cohorts remain owned by their real drivers."""

import json
import re
import shutil
import unittest
from pathlib import Path

import test_ci_fail_fast


ROOT = Path(__file__).resolve().parents[1]
JAR_SHA256 = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"
MODEL_RUNNERS = (
    "formal/cli/run_models.py",
    "formal/cli/run_configuration_models.py",
    "formal/cli/run_session_models.py",
    "formal/cli/run_target_models.py",
    "formal/cli/run_bootstrap_models.py",
    "formal/cli/run_native_thread_models.py",
    "formal/cli/CommandSurfaceCheck.py",
    "formal/cli/json-numbers/check_model.py",
    "formal/cli-packaging/run_models.py",
    "formal/agent-api-security/check_model.py",
    "formal/agent-api-transport/check_model.py",
    "formal/agent-api-app/check_model.py",
    "formal/agent-api-host/check_model.py",
    "formal/cli-ci/check_model.py",
)
HARNESSES = (
    "formal/agent-api-security/Cargo.toml",
    "formal/agent-api-transport/Cargo.toml",
)


def workflow_job(name):
    source = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    jobs = re.split(r"(?m)^  [a-zA-Z0-9_-]+:\s*$", source.split("\njobs:\n", 1)[1])[1:]
    matching = [job for job in jobs if re.search(rf"(?m)^    name: {re.escape(name)}\s*$", job)]
    if len(matching) != 1:
        raise AssertionError(f"expected one required {name!r} job; found {len(matching)}")
    return matching[0]


def commands(job):
    # Join YAML folded/shell continued commands without adding a YAML dependency.
    return re.sub(r"[`\\]\s*\n\s*", " ", job)


class NativeProductCiTests(unittest.TestCase):
    def test_windows_workspace_tests_load_the_pinned_dlls(self):
        job = commands(workflow_job("CLI contracts"))
        cargo = next((m for m in re.finditer(r"(?m)^\s*cargo test [^\n]+", job)
                      if "--workspace" in m[0] and "--locked" in m[0] and "rust-runtime/Cargo.toml" in m[0]), None)
        self.assertIsNotNone(cargo, "run the locked native workspace test suite on Windows")
        setup = job[:cargo.start()].lower().replace("\\", "/")
        for filename in ("onnxruntime.dll", "kaldi-native-fbank-core.dll"):
            self.assertIn(filename, setup, f"make {filename} loadable before starting workspace tests")
        self.assertIn("debug/deps", setup, "place the pinned libraries next to Cargo's test binaries")

    def test_native_formal_matrix_executes_with_existing_pinned_tools(self):
        job = commands(workflow_job("CLI contracts"))
        invocation = next((line for line in job.splitlines()
                           if re.search(r"\bpython\s+formal/rust-runtime/check_models\.py\b",line)), None)
        self.assertIsNotNone(invocation, "execute the full native runtime model matrix")
        for argument in ("--jar", "--output"):
            self.assertIn(argument, invocation)
        self.assertIn(JAR_SHA256, job[:job.index(invocation)])

class CliCiTests(unittest.TestCase):
    def test_cli_contracts_is_required_windows_job(self):
        job = workflow_job("CLI contracts")
        self.assertRegex(job, r"(?m)^    runs-on: windows-latest\s*$")
        self.assertNotRegex(job, r"(?m)^    (?:if|continue-on-error):")
        self.assertNotRegex(job, r"continue-on-error:\s*true|\|\|\s*(?:true|exit\s+0)")

    def test_required_cli_execution_steps_cannot_be_skipped_or_ignore_failures(self):
        job = workflow_job("CLI contracts")
        steps = re.split(r"(?m)^      - ", job)[1:]
        self.assertTrue(steps)
        for step in steps:
            with self.subTest(step=step.splitlines()[0]):
                self.assertFalse(step.lstrip().startswith("{"), "use inspectable block-style steps")
                normalized = "        " + step
                conditions = re.findall(r"(?m)^        if:\s*([^\n]+)", normalized)
                if "uses: actions/upload-artifact@" in step:
                    self.assertEqual(conditions, ["always()"])
                else:
                    self.assertEqual(conditions, [], "required execution cannot be conditional")
                failure_policies = re.findall(r"(?m)^        continue-on-error:\s*([^\n]+)", normalized)
                self.assertTrue(all(value.strip() == "false" for value in failure_policies),
                                "required steps must propagate failures")

    def test_cli_job_reuses_pinned_existing_tools(self):
        job = workflow_job("CLI contracts")
        trusted = {
            "actions/checkout": "3d3c42e5aac5ba805825da76410c181273ba90b1",
            "actions/setup-python": "5fda3b95a4ea91299a34e894583c3862153e4b97",
            "astral-sh/setup-uv": "c771a70e6277c0a99b617c7a806ffedaca235ff9",
            "actions/setup-java": "de7274f081f381c8f8158605e0321c36c376e2e6",
            "actions/cache": "caa296126883cff596d87d8935842f9db880ef25",
            "actions/upload-artifact": "043fb46d1a93c77aae656e7c1c64a875d1fc6a0a",
        }
        for action, digest in trusted.items():
            with self.subTest(action=action):
                self.assertIn(f"uses: {action}@{digest}", job)
        self.assertIn("persist-credentials: false", job)
        self.assertIn("python-version-file: .python-version", job)
        self.assertEqual((ROOT / ".python-version").read_text().strip(), "3.11")
        self.assertIn("version: 0.11.30", job)
        self.assertIn("distribution: temurin", job)
        self.assertRegex(job, r"java-version:\s*['\"]?21")
        self.assertIn("rustup toolchain install 1.93.0 --profile minimal", job)
        self.assertIn("rustup default 1.93.0", job)
        self.assertIn("uv sync --frozen", job)
        self.assertNotRegex(job, r"(?:pip install|uv add|pnpm add|secrets\.|gh release|git push)")

    def test_all_cli_unit_and_packaging_tests_are_discovered(self):
        job = commands(workflow_job("CLI contracts"))
        self.assertRegex(job, r"python -m unittest discover -s scripts -p [\"']?test_\*cli\*\.py")
        # The existing required Python job already executes SDK constructors in resources.
        python = commands(workflow_job("Python"))
        self.assertRegex(python, r"python -m unittest discover -s src-tauri/resources -p [\"']test_\*\.py")
        self.assertTrue((ROOT / "src-tauri/resources/test_native_thread_budget.py").is_file())

    def test_required_cli_job_excludes_withdrawn_transition_model(self):
        self.assertNotIn("formal/asr-transition-ownership", workflow_job("CLI contracts"))

    def test_admission_runner_unit_tests_execute_in_required_cli_unit_step(self):
        step = next(step for step in workflow_job("CLI contracts").split("      - name: ")
                    if step.startswith("Run CLI unit contracts\n"))
        self.assertRegex(step, r"(?m)^          uv run --no-sync python -m unittest discover -s formal/agent-api-host/admission -p test_runner\.py\s*$")

    def test_json_numbers_runner_unit_tests_execute_in_required_cli_unit_step(self):
        step = next(step for step in workflow_job("CLI contracts").split("      - name: ")
                    if step.startswith("Run CLI unit contracts\n"))
        self.assertRegex(step, r"(?m)^          uv run --no-sync python -m unittest discover -s formal/cli/json-numbers -p test_runner\.py\s*$")

    def test_entrypoint_unit_contracts_execute_in_required_cli_unit_step(self):
        step = next((step for step in workflow_job("CLI contracts").split("      - name: ")
                     if step.startswith("Run CLI unit contracts\n")), "")
        self.assertTrue(step)
        expected = ('uv run --no-sync python -m unittest discover '
                    '-s formal/agent-api-measurements/entrypoint -p "test_*.py" -v')
        actual = [line.strip() for line in step.splitlines()
                  if "formal/agent-api-measurements/entrypoint" in line]
        self.assertEqual(actual, [expected])

    def test_both_real_native_harnesses_are_serial_and_locked(self):
        job = commands(workflow_job("CLI contracts"))
        for manifest in HARNESSES:
            with self.subTest(manifest=manifest):
                line = next((line for line in job.splitlines()
                             if "cargo test " in line and manifest in line), "")
                self.assertTrue(line, f"missing actual cargo test for {manifest}")
                self.assertIn("--locked", line)
                self.assertRegex(line, r"(?:-j\s*2|--jobs(?:=|\s+)2)(?:\s|$)")
                self.assertRegex(line, r"--test-threads(?:=|\s+)1(?:\s|$)")
        rust = commands(workflow_job("Rust"))
        self.assertRegex(rust, r"cargo test[^\n]*src-tauri/Cargo.toml[^\n]*--all-targets[^\n]*--locked")

    def test_all_maintained_model_matrices_run_without_case_filters(self):
        job = commands(workflow_job("CLI contracts"))
        for runner in MODEL_RUNNERS:
            with self.subTest(runner=runner):
                line = next((line for line in job.splitlines()
                             if re.search(rf"python\s+{re.escape(runner)}(?:\s|$)", line)), "")
                self.assertTrue(line, f"missing complete matrix: {runner}")
                self.assertIn("--jar", line)
                self.assertIn("--output", line)
                self.assertNotRegex(line, r"--(?:only|case|sany-only)(?:\s|=|$)")
        self.assertRegex(job, r"pwsh -NoProfile -File formal/asr-agent-settings/run_checks\.ps1[^\n]*-Java[^\n]*-Jar[^\n]*-OutputDirectory")
        self.assertTrue((ROOT / "formal/asr-agent-settings/run_checks.ps1").is_file(),
                        "required UI model runner is not integrated")

    def test_controller_admission_matrix_runs_unfiltered_in_required_cli_step(self):
        self.assertRegex(next(step for step in workflow_job("CLI contracts").split("      - name: ")
                              if step.startswith("Run complete CLI formal matrices\n")),
                         r"(?m)^          uv run --no-sync python formal/agent-api-host/admission/check_model\.py --java \$java --jar \$jar --output \(Join-Path \$receipts 'native-admission'\)\s*$")

    def test_tlc_is_hash_checked_before_any_model_runs(self):
        job = workflow_job("CLI contracts")
        url = "https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar"
        self.assertIn(url, job)
        self.assertIn(JAR_SHA256, job.lower())
        self.assertRegex(job, r"Get-FileHash[^\n]*SHA256")
        self.assertRegex(job, r"(?s)Get-FileHash.*?throw\s+['\"][^'\"]*(?:SHA256|digest|hash|TLA)")
        for runner in MODEL_RUNNERS:
            self.assertLess(job.index(JAR_SHA256), job.index(runner))
        # Each maintained runner supplies the actual resource bounds, not a comment in CI.
        for runner in MODEL_RUNNERS:
            path = ROOT / runner
            self.assertTrue(path.is_file(), f"required model runner is not integrated: {runner}")
            source = path.read_text(encoding="utf-8")
            self.assertIn("-Xmx512m", source, runner)
            self.assertRegex(source, r"[\"']-workers[\"'],\s*[\"']1[\"']", runner)

    def test_packaged_cli_is_actually_built_source_verified_and_doctored(self):
        job = commands(workflow_job("CLI contracts"))
        build = re.search(r"python scripts/build_cli\.py\s*$", job, re.M)
        verify = re.search(r"python scripts/build_cli\.py --verify(?:\s|$)", job)
        self.assertIsNotNone(build, "build the actual standalone executable")
        self.assertIsNotNone(verify, "verify packaged Python entry code against source")
        self.assertLess(build.start(), verify.start())
        self.assertRegex(job, r"light-whisper-cli\.exe[^\n]*\sdoctor(?:\s|$)")
        self.assertNotRegex(job, r"light-whisper-cli\.exe[^\n]*(?:transcribe|engine start)")

    def test_actual_headless_wry_driver_runs_all_twenty_cases(self):
        job = commands(workflow_job("CLI contracts"))
        for manifest in ("src-tauri/Cargo.toml", "formal/agent-api-host/native-fixture/Cargo.toml"):
            line = next((line for line in job.splitlines()
                         if "cargo build " in line and manifest in line), "")
            self.assertTrue(line, f"actual native build absent: {manifest}")
            self.assertIn("--locked", line)
            self.assertRegex(line, r"(?:-j\s*2|--jobs(?:=|\s+)2)(?:\s|$)")
            self.assertNotIn("--release", line)
        line = next((line for line in job.splitlines() if "python formal/agent-api-host/run.py " in line), "")
        self.assertTrue(line, "real windowless Wry host driver must run")
        self.assertRegex(line, r"--app-exe[^\n]*debug[/\\]light-whisper\.exe")
        self.assertRegex(line, r"--fixture-exe[^\n]*debug[/\\]agent-host-native-fixture\.exe")
        self.assertIn("--evidence", line)
        self.assertNotRegex(line, r"--case(?:\s|=)")
        self.assertNotRegex(job, r"tauri build|build_installer|release\.sh|--rebuild-engine")

    def test_evidence_upload_is_always_json_and_logs_only(self):
        job = workflow_job("CLI contracts")
        upload = next((step for step in job.split("      - name: ")
                       if "uses: actions/upload-artifact@" in step), "")
        self.assertTrue(upload)
        self.assertIn("if: always()", upload)
        path = re.search(r"(?m)^          path: \|\n((?:            [^\n]*\n)+)", upload)
        self.assertIsNotNone(path, "upload explicit receipt/log globs only")
        paths = [line.strip() for line in path[1].splitlines() if line.strip()]
        self.assertTrue(paths)
        for value in paths:
            self.assertRegex(value, r"\.(?:json|log)$")
            self.assertNotRegex(value, r"endpoint|user_profile|audio|fixture|engine\.json")

    def test_release_local_checks_include_cli_tests_and_both_native_harnesses(self):
        source = (ROOT / "scripts/release.sh").read_text(encoding="utf-8")
        body = re.search(r"^run_local_checks\(\) \{\n.*?^\}", source, re.M | re.S)[0]
        body = commands(body)
        self.assertRegex(body, r"python -m unittest discover -s scripts -p [\"']?test_\*cli\*\.py")
        for manifest in HARNESSES:
            line = next((line for line in body.splitlines() if "cargo test " in line and manifest in line), "")
            self.assertTrue(line, f"local release omits {manifest}")
            self.assertIn("--locked", line)
            self.assertRegex(line, r"(?:-j\s*2|--jobs(?:=|\s+)2)(?:\s|$)")
            self.assertRegex(line, r"--test-threads(?:=|\s+)1(?:\s|$)")


class CliAsrSettingsFormalScopeTests(unittest.TestCase):
    def test_registry_excludes_withdrawn_model_and_keeps_qualified_bindings(self):
        registry = json.loads((ROOT / "formal/verification.json").read_text(encoding="utf-8"))
        contract = registry["contracts"]["saved-engine-invalidation"]
        self.assertEqual(set(contract["models"]), {
            "asr-agent-settings/AsrAgentSettings",
            "asr-subscription-bootstrap/AsrSubscriptionBootstrap",
        })
        for model, properties in contract["models"].items():
            with self.subTest(model=model):
                self.assertTrue(properties)
        self.assertTrue({
            "src-tauri/src/commands/funasr.rs#pub async fn set_engine(",
            "src-tauri/src/agent_api/controller.rs#async fn dispatch_request(",
            "src/hooks/useAsrEngineSettings.ts#const readAsrConfig = useCallback",
            "src/hooks/useAsrEngineSettings.ts#const hydrateInitialConfig = () =>",
            'src/hooks/useAsrEngineSettings.ts#void listen("engine-config-changed"',
        }.issubset(contract["implementation"]))
        self.assertTrue({
            "src/hooks/__tests__/useAsrEngineSettingsExternalChanges.test.tsx",
            "formal/asr-agent-settings/README.md",
            "formal/asr-agent-settings/run_checks.ps1",
            "src/hooks/__tests__/useAsrEngineSettingsSubscriptionBootstrap.test.tsx",
            "formal/asr-subscription-bootstrap/verify.py",
            "formal/asr-subscription-bootstrap/SourceControls.md",
            "src/hooks/__tests__/useAsrEngineSettingsTransitionOwnership.test.tsx",
            "formal/agent-api-host/run.py",
        }.issubset(contract["evidence"]))
        for evidence in contract["evidence"]:
            with self.subTest(evidence=evidence):
                self.assertFalse(evidence.startswith("formal/asr-transition-ownership/"))
                self.assertTrue((ROOT / evidence.split("#", 1)[0]).is_file())


class CliAsrSubscriptionBootstrapCiTests(unittest.TestCase):
    def test_complete_subscription_bootstrap_matrix_runs_in_cli_formal_step(self):
        job = commands(workflow_job("CLI contracts"))
        step = next((step for step in job.split("      - name: ")
                     if step.startswith("Run complete CLI formal matrices\n")), "")
        self.assertTrue(step, "subscription bootstrap belongs in the existing CLI formal step")
        runner = "formal/asr-subscription-bootstrap/verify.py"
        expected = (f"uv run --no-sync python {runner} --java $java --jar $jar "
                    "--output (Join-Path $receipts 'asr-subscription-bootstrap')")
        self.assertIn(expected, [line.strip() for line in step.splitlines()],
                      "run the complete subscription bootstrap matrix without a case/SANY filter")
        self.assertLess(step.index(JAR_SHA256), step.index(runner),
                        "verify the pinned TLC JAR before the subscription matrix")
        source = (ROOT / runner).read_text(encoding="utf-8")
        self.assertIn("-Xmx512m", source)
        self.assertRegex(source, r"[\"']-workers[\"'],\s*[\"']1[\"']")


@unittest.skipUnless(shutil.which("pwsh"), "requires Windows CI PowerShell")
class CliNativeFailureTests(unittest.TestCase):
    def test_each_cli_native_failure_stops_the_actual_step(self):
        workflow_job("CLI contracts")
        for name in ("Run CLI unit contracts", "Run native CLI contracts"):
            script = test_ci_fail_fast.step_script(name)
            self.assertIn("$ErrorActionPreference = 'Stop'", script)
            self.assertIn("$PSNativeCommandUseErrorActionPreference = $true", script)
            count = len(re.findall(r"^(?:uv|cargo|rustup|rustc) ", script, re.M))
            self.assertGreaterEqual(count, 2, name)
            for position in range(1, count + 1):
                with self.subTest(step=name, failure=position):
                    result = test_ci_fail_fast.NativeFailureTests.run_step(self, name, position)
                    self.assertNotEqual(result.returncode, 0)
            result = test_ci_fail_fast.NativeFailureTests.run_step(self, name, 0)
            self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
