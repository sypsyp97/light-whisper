import argparse
import contextlib
import json
import math
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
import time


FROZEN = bool(getattr(sys, "frozen", False))
ROOT = Path(sys._MEIPASS if FROZEN else Path(__file__).resolve().parents[1]).resolve()
ENGINE_SCRIPT = ROOT / "src-tauri" / "resources" / "engine.py"
DEFAULT_PYTHON = ROOT / ".venv" / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
CLEANUP_TIMEOUT = 2
CLEANUP_GRACE = .25
LOCAL_ENGINES = ("qwen3-asr-0.6b", "confucius4-r2t2")
DESKTOP_ACTIONS = ("models", "devices", "engine_get", "engine_set", "engine_start", "engine_restart")
DESKTOP_REQUEST_MAX = 64 * 1024
AGENT_PROTOCOL = b"LIGHT_WHISPER_AGENT_PROTOCOL_1"


class EngineClient:
    def __init__(self, command, timeout, env=None):
        self.command = command
        self.timeout = timeout
        self.env = env
        self.child = None
        self.ready = False
        self.sequence = 0
        self.messages = queue.Queue()
        self.reader = None
        self.sender = None

    def _read_stdout(self):
        try:
            for line in self.child.stdout:
                self.messages.put(line)
        finally:
            self.messages.put(None)

    def _receive(self, request_id, deadline):
        while True:
            try:
                line = self.messages.get(timeout=max(0, deadline - time.monotonic()))
            except queue.Empty as error:
                raise TimeoutError(f"Engine response exceeded {self.timeout:g} seconds") from error
            if time.monotonic() > deadline:
                raise TimeoutError(f"Engine response exceeded {self.timeout:g} seconds")
            if line is None:
                raise EOFError("Engine stdout closed before a matching response")
            try:
                response = json.loads(line)
            except json.JSONDecodeError:
                continue
            if not isinstance(response, dict) or type(response.get("success")) is not bool:
                continue
            if request_id is None or (
                type(response.get("request_id")) is int
                and response["request_id"] == request_id
            ):
                try:
                    json.dumps(response, allow_nan=False)
                except ValueError as error:
                    raise RuntimeError("Engine response contains a nonfinite JSON number") from error
                if response.get("fatal") is True:
                    raise RuntimeError(response.get("error") or "Desktop connection failed")
                return response

    def _send(self, message, deadline):
        result = queue.Queue()

        def write():
            try:
                self.child.stdin.write(json.dumps(message, ensure_ascii=True) + "\n")
                self.child.stdin.flush()
            except OSError as error:
                result.put(error)
            else:
                result.put(None)

        self.sender = threading.Thread(target=write, daemon=True)
        self.sender.start()
        try:
            error = result.get(timeout=max(0, deadline - time.monotonic()))
        except queue.Empty as error:
            raise TimeoutError("Engine stdin write exceeded the response deadline") from error
        if error is not None:
            raise EOFError(f"Engine stdin closed: {error}") from error
        self.sender.join(timeout=max(0, deadline - time.monotonic()))
        if self.sender.is_alive():
            raise TimeoutError("Engine stdin worker exceeded the response deadline")

    def __enter__(self):
        try:
            self.child = subprocess.Popen(
                self.command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                text=True, encoding="utf-8", errors="replace", bufsize=1, env=self.env,
                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
            )
            self.reader = threading.Thread(target=self._read_stdout, daemon=True)
            self.reader.start()
            response = self._receive(None, time.monotonic() + self.timeout)
            if not response["success"]:
                raise RuntimeError(f"Engine startup failed: {json.dumps(response, ensure_ascii=False)}")
            self.ready = True
            return self
        except BaseException:
            if self.child is not None:
                self.__exit__(*sys.exc_info())
            raise

    def __exit__(self, exc_type, exc_value, traceback):
        self.ready = False
        deadline = time.monotonic() + CLEANUP_TIMEOUT
        cleanup_error = None
        try:
            if self.child.poll() is None:
                try:
                    if self.sender is None or not self.sender.is_alive():
                        self._send({"action": "exit", "request_id": self.sequence + 1},
                                   time.monotonic() + CLEANUP_GRACE)
                    self.child.wait(timeout=CLEANUP_GRACE)
                except BaseException as error:
                    if not isinstance(error, (EOFError, TimeoutError, subprocess.TimeoutExpired)):
                        cleanup_error = error
                    self.child.kill()
                    self.child.wait(timeout=max(0, deadline - time.monotonic()))
            else:
                self.child.wait(timeout=max(0, deadline - time.monotonic()))
        finally:
            for worker in (self.sender, self.reader):
                if worker is not None and worker.ident is not None:
                    worker.join(timeout=max(0, deadline - time.monotonic()))
            for pipe in (self.child.stdin, self.child.stdout):
                try:
                    pipe.close()
                except OSError:
                    pass
        if cleanup_error is not None and exc_type is None:
            raise cleanup_error
        return False

    def request(self, action, **fields):
        if not self.ready:
            raise RuntimeError("Engine client has not completed startup")
        self.sequence += 1
        deadline = time.monotonic() + self.timeout
        self._send({**fields, "action": action, "request_id": self.sequence}, deadline)
        return self._receive(self.sequence, deadline)


class JsonArgumentParser(argparse.ArgumentParser):
    def error(self, message):
        raise ValueError(message)


def positive_timeout(value):
    try:
        number = float(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("timeout must be a positive finite number") from error
    if not math.isfinite(number) or number <= 0:
        raise argparse.ArgumentTypeError("timeout must be a positive finite number")
    return number


def positive_threads(value):
    try:
        number = int(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("threads must be a positive integer") from error
    if not 1 <= number <= 2 ** 31 - 1:
        raise argparse.ArgumentTypeError("threads must be an integer from 1 to 2147483647")
    return number


def parser():
    arguments = JsonArgumentParser(description="Light Whisper desktop and local engine CLI")
    arguments.add_argument("--target", choices=("app", "engine"))
    arguments.add_argument("--app-exe", type=Path)
    arguments.add_argument("--engine", choices=LOCAL_ENGINES)
    runtime = arguments.add_mutually_exclusive_group()
    runtime.add_argument("--python", type=Path)
    runtime.add_argument("--engine-exe", type=Path)
    arguments.add_argument("--timeout", type=positive_timeout, default=120)
    arguments.add_argument("--threads", type=positive_threads, default=2)
    arguments.add_argument("--data-dir", type=Path)
    commands = arguments.add_subparsers(dest="command", required=True)
    for command in ("doctor", "status", "stats", "models", "devices"):
        commands.add_parser(command)
    engine = commands.add_parser("engine")
    engine_commands = engine.add_subparsers(dest="engine_command", required=True)
    for command in ("get", "start", "restart"):
        engine_commands.add_parser(command)
    engine_set = engine_commands.add_parser("set")
    engine_set.add_argument("value", choices=LOCAL_ENGINES)
    commands.add_parser("session")
    transcribe = commands.add_parser("transcribe")
    transcribe.add_argument("audio", type=Path)
    transcribe.add_argument("--language")
    transcribe.add_argument("--context")
    transcribe.add_argument("--hot-word", action="append")
    return arguments


def resolve_engine_selection(args):
    default_data = Path(os.environ.get("APPDATA", Path.home() / ".local" / "share")) / "com.light-whisper.app"
    args.config_dir = Path(os.environ.get("LIGHT_WHISPER_DATA_DIR", default_data)).expanduser().resolve()
    config_path = args.config_dir / "engine.json"
    try:
        config = json.loads(config_path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        config = {}
    except (UnicodeError, json.JSONDecodeError) as error:
        raise ValueError(f"Invalid engine configuration: {config_path}") from error
    if not isinstance(config, dict):
        raise ValueError(f"Engine configuration must be a JSON object: {config_path}")
    if args.engine is not None:
        args.engine_source = "argument"
    elif "engine" not in config:
        args.engine, args.engine_source = LOCAL_ENGINES[0], "default"
    else:
        saved = config["engine"]
        if saved == "qwen3-asr-1.7b":
            saved = "confucius4-r2t2"
        if not isinstance(saved, str) or saved not in (*LOCAL_ENGINES, "glm-asr", "alibaba-asr"):
            raise ValueError(f"Invalid saved engine in {config_path}; select a local --engine")
        args.engine, args.engine_source = saved, "desktop_config"
    args.local_engine_supported = args.engine in LOCAL_ENGINES
    custom_models = config.get("models_dir")
    if isinstance(custom_models, str) and custom_models:
        models, args.models_dir_source = Path(custom_models), "desktop_config"
    elif "HF_HOME" in os.environ:
        models, args.models_dir_source = Path(os.environ["HF_HOME"]) / "hub", "HF_HOME"
    else:
        models, args.models_dir_source = Path.home() / ".cache" / "huggingface" / "hub", "default"
    args.models_dir = models.expanduser().resolve()
    return args


def resolve_target(args):
    independent = any(value is not None for value in (
        args.engine, args.python, args.engine_exe, args.data_dir,
    ))
    if ((args.target == "app" or args.app_exe is not None) and independent
            or args.target == "engine" and args.app_exe is not None):
        raise ValueError("Desktop and independent-engine options cannot be combined")
    return args.target or ("engine" if independent else "app")


def app_path(args):
    if args.app_exe is not None:
        return args.app_exe.expanduser().resolve()
    candidates = [ROOT / "src-tauri" / "target" / build / "light-whisper.exe"
                  for build in ("release", "debug")]
    if FROZEN:
        executable = Path(sys.executable).resolve()
        candidates[:0] = [executable.parent / "light-whisper.exe",
                          executable.parent.parent / "light-whisper.exe"]
        candidates = [path for path in candidates if path.resolve() != executable]
    if os.environ.get("LOCALAPPDATA"):
        candidates.append(Path(os.environ["LOCALAPPDATA"]) / "轻语 Whisper" / "light-whisper.exe")
    return next((path.resolve() for path in candidates if path.is_file()), None)


def app_protocol_supported(executable):
    if executable is None or not executable.is_file():
        return False
    tail = b""
    with executable.open("rb") as stream:
        while block := stream.read(64 * 1024):
            block = tail + block
            if AGENT_PROTOCOL in block:
                return True
            tail = block[-(len(AGENT_PROTOCOL) - 1):]
    return False


def app_command(args):
    executable = app_path(args)
    if executable is None or not executable.is_file():
        raise ValueError("Desktop executable not found; specify --app-exe")
    if not app_protocol_supported(executable):
        raise ValueError("Desktop CLI protocol is unavailable; upgrade Light Whisper or specify a compatible --app-exe")
    return [str(executable), "--agent-client"]


def engine_runtime(args):
    if args.engine_exe is not None:
        runtime, python = args.engine_exe, False
    elif args.python is not None:
        runtime, python = args.python, True
    elif DEFAULT_PYTHON.is_file() and ENGINE_SCRIPT.is_file():
        runtime, python = DEFAULT_PYTHON, True
    else:
        installed = args.config_dir / "engine" / "engine.exe"
        runtime, python = (installed, False) if installed.is_file() else (DEFAULT_PYTHON, True)
    return runtime.expanduser().resolve(), python


def engine_command(args):
    if not args.local_engine_supported:
        raise ValueError("The saved engine is a cloud service; select a local --engine")
    runtime, python = engine_runtime(args)
    if not runtime.is_file():
        raise ValueError(f"Runtime file does not exist: {runtime}")
    command = [str(runtime)]
    if python:
        if not ENGINE_SCRIPT.is_file():
            raise ValueError(f"Engine script does not exist: {ENGINE_SCRIPT}")
        command.append(str(ENGINE_SCRIPT))
    return command + ["serve", "--engine", args.engine]


def doctor(args):
    version = json.loads((ROOT / "package.json").read_text(encoding="utf-8"))["version"]
    application = app_path(args)
    runtime = application if args.target == "app" else engine_runtime(args)[0]
    return {
        "success": True, "version": version, "engine": args.engine,
        "engine_source": args.engine_source, "config_dir": str(args.config_dir),
        "local_engine_supported": args.local_engine_supported,
        "target": args.target,
        "surface": ("running desktop instance" if args.target == "app" else
                    "separate local engine process; not the running desktop instance"),
        "repository": None if FROZEN else str(ROOT), "resource_root": str(ROOT), "frozen": FROZEN,
        "engine_script": str(ENGINE_SCRIPT),
        "app_protocol_supported": app_protocol_supported(application),
        "models_dir": str(args.models_dir), "models_dir_source": args.models_dir_source,
        "default_python": str(DEFAULT_PYTHON), "runtime": str(runtime) if runtime is not None else None,
        "runtime_present": runtime is not None and runtime.is_file(),
        "engine_script_present": ENGINE_SCRIPT.is_file(),
        "model_load_checked": False,
    }


@contextlib.contextmanager
def owned_engine(args):
    command = engine_command(args)
    data_context = (
        contextlib.nullcontext(str(args.data_dir.expanduser().resolve()))
        if args.data_dir is not None
        else tempfile.TemporaryDirectory(prefix="light-whisper-cli-")
    )
    with data_context as data_dir:
        Path(data_dir).mkdir(parents=True, exist_ok=True)
        environment = {**os.environ, "LIGHT_WHISPER_DATA_DIR": data_dir,
                       "HF_HUB_OFFLINE": "1", "TRANSFORMERS_OFFLINE": "1",
                       "HF_HUB_CACHE": str(args.models_dir),
                       "LIGHT_WHISPER_NATIVE_THREADS": str(args.threads)}
        for key in ("OMP_NUM_THREADS", "MKL_NUM_THREADS", "OPENBLAS_NUM_THREADS"):
            environment[key] = str(args.threads)
        with EngineClient(command, args.timeout, env=environment) as client:
            yield client


def request_client(args):
    if args.target == "app":
        return EngineClient(app_command(args), args.timeout, env={
            **os.environ, "LIGHT_WHISPER_DATA_DIR": str(args.config_dir),
        })
    return owned_engine(args)


def validate_desktop_request(action, fields, request_id):
    message = {**fields, "action": action, "request_id": request_id}
    try:
        json.dumps(message, ensure_ascii=False).encode("utf-8")
    except UnicodeError as error:
        raise ValueError("Desktop request contains invalid Unicode") from error
    size = len(json.dumps(message, ensure_ascii=True).encode("ascii")) + len(os.linesep)
    if size > DESKTOP_REQUEST_MAX:
        raise ValueError("Desktop request exceeds the 64 KiB message limit")


def session_request(message, target="app"):
    if not isinstance(message, dict) or type(message.get("request_id")) is not int:
        raise ValueError("Each request must be an object with an integer request_id")
    action = message.get("action")
    if not isinstance(action, str) or action not in ("status", "stats", "transcribe", "exit", *DESKTOP_ACTIONS):
        raise ValueError("Unsupported session action")
    if target != "app" and action in DESKTOP_ACTIONS:
        raise ValueError("This command requires the desktop target")
    allowed = {"request_id", "action"}
    if action == "transcribe":
        allowed.update(("audio", "language", "context", "hot_words"))
    elif action == "engine_set":
        allowed.add("engine")
    if message.keys() - allowed:
        raise ValueError("Request contains unsupported fields")
    fields = {}
    if action == "engine_set":
        engine = message.get("engine")
        if not isinstance(engine, str) or engine not in LOCAL_ENGINES:
            raise ValueError("engine_set requires a supported local engine")
        fields["engine"] = engine
    if action == "transcribe":
        audio = message.get("audio")
        if not isinstance(audio, str) or not audio:
            raise ValueError("transcribe requires a nonempty audio path")
        path = Path(audio).expanduser().resolve()
        if not path.is_file():
            raise ValueError(f"Audio file does not exist: {path}")
        options = {}
        for key in ("language", "context"):
            if key in message:
                if not isinstance(message[key], str):
                    raise ValueError(f"{key} must be a string")
                options[key] = message[key]
        fields = {"audio_path": str(path), "options": options}
        if "hot_words" in message:
            hot_words = message["hot_words"]
            if not isinstance(hot_words, list) or any(not isinstance(word, str) for word in hot_words):
                raise ValueError("hot_words must be a list of strings")
            fields["hot_words"] = hot_words
    return action, fields


def session_reply(response):
    print(json.dumps(response, ensure_ascii=True), flush=True)


def run_session(args):
    pending_id = None
    try:
        if hasattr(sys.stdin, "reconfigure"):
            sys.stdin.reconfigure(encoding="utf-8")
        with contextlib.ExitStack() as resources:
            client = None
            for line in sys.stdin:
                if not line.strip():
                    continue
                pending_id = None
                try:
                    message = json.loads(line)
                    if isinstance(message, dict) and type(message.get("request_id")) is int:
                        pending_id = message["request_id"]
                    action, fields = session_request(message, args.target)
                    if args.target == "app" and action != "exit":
                        validate_desktop_request(
                            action, fields, client.sequence + 1 if client is not None else 1,
                        )
                except ValueError as error:
                    session_reply({"success": False, "request_id": pending_id, "error": str(error)})
                    pending_id = None
                    continue
                if action == "exit":
                    resources.close()
                    session_reply({"success": True, "request_id": pending_id})
                    return 0
                if client is None:
                    client = resources.enter_context(request_client(args))
                response = client.request(action, **fields)
                session_reply({**response, "request_id": pending_id})
                pending_id = None
        return 0
    except KeyboardInterrupt:
        code, error = 130, "Interrupted"
    except Exception as failure:
        code, error = 1, str(failure)
    if pending_id is not None:
        session_reply({"success": False, "request_id": pending_id, "error": error})
    else:
        print(error, file=sys.stderr)
    return code


def main(argv=None):
    code = 0
    try:
        args = parser().parse_args(argv)
        args.target = resolve_target(args)
        args = resolve_engine_selection(args)
        action = "engine_" + args.engine_command if args.command == "engine" else args.command
        if args.target != "app" and action in DESKTOP_ACTIONS:
            raise ValueError("This command requires the desktop target")
        if args.command == "session":
            return run_session(args)
        if args.command == "doctor":
            response = doctor(args)
        else:
            fields = {}
            if action == "engine_set":
                fields["engine"] = args.value
            if args.command == "transcribe":
                audio = args.audio.expanduser().resolve()
                if not audio.is_file():
                    raise ValueError(f"Audio file does not exist: {audio}")
                fields = {"audio_path": str(audio), "options": {
                    key: value for key, value in (
                        ("language", args.language), ("context", args.context)
                    ) if value is not None
                }}
                if args.hot_word is not None:
                    fields["hot_words"] = args.hot_word
            if args.target == "app":
                validate_desktop_request(action, fields, 1)
            with request_client(args) as client:
                response = client.request(action, **fields)
            code = 0 if response["success"] else 1
    except ValueError as error:
        code, response = 2, {"success": False, "error": str(error)}
    except KeyboardInterrupt:
        code, response = 130, {"success": False, "error": "Interrupted"}
    except Exception as error:
        code, response = 1, {"success": False, "error": str(error)}
    print(json.dumps(response, ensure_ascii=True))
    return code


if __name__ == "__main__":
    sys.exit(main())
