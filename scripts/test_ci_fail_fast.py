
import re
import shutil
import subprocess
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
STEPS = ("Compile and test Python sources", "Set up Rust", "Check Rust formatting and lints")


def step_script(name):
    workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    following = workflow.split(f"      - name: {name}\n", 1)[1]
    body = following.split("        run: |\n", 1)[1]
    lines = []
    for line in body.splitlines():
        if line and not line.startswith("          "):
            break
        lines.append(line[10:])
    return "\n".join(lines)


@unittest.skipUnless(shutil.which("pwsh"), "requires PowerShell, as used by Windows CI")
class NativeFailureTests(unittest.TestCase):
    def run_step(self, name, fail_at):
        script = step_script(name)
        python = str(Path(sys.executable)).replace("'", "''")
        mock = f"""
$ErrorActionPreference = 'Stop'
$script:call = 0
function Invoke-TestNative {{
  $script:call++
  $code = if ($script:call -eq {fail_at}) {{ 17 }} else {{ 0 }}
  & '{python}' -c "import sys; sys.exit($code)"
}}
function uv {{ Invoke-TestNative }}
function cargo {{ Invoke-TestNative }}
function rustup {{ Invoke-TestNative }}
function rustc {{ Invoke-TestNative }}
{script}
exit $LASTEXITCODE
"""
        return subprocess.run([shutil.which("pwsh"), "-NoProfile", "-NonInteractive", "-Command", mock],
                              capture_output=True, text=True, timeout=30)

    def test_each_native_failure_stops_the_step(self):
        for name in STEPS:
            count = len(re.findall(r"^(?:uv|cargo|rustup|rustc) ", step_script(name), re.M))
            self.assertGreater(count, 1)
            for position in range(1, count + 1):
                with self.subTest(step=name, failure=position):
                    self.assertNotEqual(self.run_step(name, position).returncode, 0)

    def test_successful_steps_still_pass(self):
        for name in STEPS:
            with self.subTest(step=name):
                result = self.run_step(name, 0)
                self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
