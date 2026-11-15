import argparse
import importlib.util
import json
import math
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import uuid


ROOT = Path(__file__).resolve().parents[2]
TESTS = (
    "scripts.test_light_whisper_cli",
    "scripts.test_cli_bootstrap",
    "scripts.test_cli_configuration",
    "scripts.test_cli_session",
    "scripts.test_cli_target",
    "scripts.test_cli_native_threads",
    "scripts.test_cli_commands",
    "scripts.test_cli_json_numbers",
    "scripts.test_build_cli",
    "scripts.test_cli_measurement_gaps",
    "scripts.test_cli_measurements",
    "test_native_thread_budget",
    "test_native_constructor_gaps",
    "test_qwen3_native.QwenNativeRuntimeTests",
    "test_qwen_cpu_cuda_backends.CpuCudaAbiTests",
    "test_r2t2_finish_context.FinishContextTests",
    "test_r2t2_startup.NativeStartupTests",
)


def new_windows_job():
    spec = importlib.util.spec_from_file_location(
        "cli_measurement_native_host", ROOT / "formal/agent-api-host/run.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.Job()


def stop_owned(child, job):
    cleanup_exit = None
    failures = []
    try:
        if job is not None:
            job.close()
            cleanup_exit = 0
        elif child.poll() is None:
            os.killpg(child.pid, signal.SIGKILL)
    except BaseException as error:
        failures.append(f"{type(error).__name__}: {error}")
    try:
        if child.poll() is None:
            child.kill()
    except BaseException as error:
        failures.append(f"{type(error).__name__}: {error}")
    try:
        child.wait(timeout=2)
    except BaseException as error:
        failures.append(f"{type(error).__name__}: {error}")
    return cleanup_exit, "; ".join(failures) or None


def main():
    parser = argparse.ArgumentParser(description="Run mutation tests with bounded owned-process cleanup")
    parser.add_argument("--timeout", type=float, default=30)
    parser.add_argument("--logs", type=Path, default=Path("tmp/cli-mutation/logs"))
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error("timeout must be positive and finite")
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        command = [sys.executable, "-m", "unittest", "-f", *TESTS]
    environment = {key: value for key, value in os.environ.items()
                   if not key.upper().startswith("COVERAGE_") and key.upper() != "PYTHONPATH"}
    environment["PYTHONPATH"] = str(ROOT / "src-tauri/resources")
    args.logs.mkdir(parents=True, exist_ok=True)
    stem = uuid.uuid4().hex
    stdout_path = args.logs / (stem + ".stdout.log")
    stderr_path = args.logs / (stem + ".stderr.log")
    start = time.monotonic()
    timed_out = False
    cleanup_exit = None
    cleanup_error = None
    pending_error = None
    job = new_windows_job() if os.name == "nt" else None
    with stdout_path.open("wb") as output, stderr_path.open("wb") as errors:
        child = subprocess.Popen(command, stdout=output, stderr=errors, env=environment,
                                 start_new_session=os.name != "nt",
                                 creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        try:
            if job is not None:
                job.add(child)
            code = child.wait(timeout=args.timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            cleanup_exit, cleanup_error = stop_owned(child, job)
            code = 1
        except BaseException as error:
            pending_error = error
            cleanup_exit, cleanup_error = stop_owned(child, job)
            code = 130 if isinstance(error, KeyboardInterrupt) else 1
        finally:
            if job is not None:
                try:
                    job.close()
                except BaseException as error:
                    cleanup_error = "; ".join(filter(None, (
                        cleanup_error, f"{type(error).__name__}: {error}")))
                    if code == 0:
                        code = 1
    receipt = {"exit": code, "timeout": timed_out, "cleanup_exit": cleanup_exit,
               "cleanup_error": cleanup_error, "root_reaped": child.poll() is not None,
               "error_type": type(pending_error).__name__ if pending_error is not None else None,
               "error_message": str(pending_error) if pending_error is not None else None,
               "elapsed": time.monotonic() - start, "stdout": str(stdout_path),
               "stderr": str(stderr_path)}
    (args.logs / (stem + ".json")).write_text(json.dumps(receipt), encoding="utf-8")
    print(json.dumps(receipt))
    if pending_error is not None:
        raise pending_error
    return code


if __name__ == "__main__":
    sys.exit(main())
