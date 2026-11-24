"""Black-box parity against the installed-source baseline using offline cached weights."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import tempfile
import threading
import time
import wave


def require(predicate, message):
    if not predicate:
        raise AssertionError(message)


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def match(actual, **expected):
    for key, value in expected.items():
        require(type(actual.get(key)) is type(value) and actual[key] == value,
                f"{key}: expected {value!r}, response {actual!r}")


class Wire:
    def __init__(self, child, report):
        self.child = child
        self.report = report
        self.messages = queue.Queue()
        self.rid = 0
        self.reader = threading.Thread(target=self.read, daemon=True)
        self.reader.start()

    def read(self):
        try:
            for line in self.child.stdout:
                self.messages.put(line)
        finally:
            self.messages.put(None)

    def receive(self, label):
        line = self.messages.get(timeout=180)
        require(line is not None, f"missing {label} JSON response; engine closed stdout")
        response = json.loads(line)
        require(isinstance(response, dict), f"{label}: expected JSON object")
        self.report["responses"].append([label, response])
        return response

    def request(self, action, success=True, **kwargs):
        self.rid += 1
        self.child.stdin.write(json.dumps(dict(request_id=self.rid, action=action, **kwargs)) + "\n")
        self.child.stdin.flush()
        result = self.receive(action)
        match(result, request_id=self.rid, success=success)
        if not success:
            require(isinstance(result.get("error"), str) and result["error"], result)
        return result


def pcm64(data):
    return base64.b64encode(data).decode("ascii")


def stream_reply(result, engine, backend, session, count, final):
    match(result, session_id=session, engine=engine, backend=backend, sample_count=count, final=final)
    require(all(isinstance(result.get(k), str) for k in ("text", "tentative_text")), result)
    require("language" in result, result)
    if final:
        match(result, tentative_text="")


def contract(wire, args, audio_path, pcm, report):
    engine, backend = args.engine, args.backend
    init = wire.receive("startup")
    match(init, success=True, engine=engine, backend=backend, model_loaded=True)
    request = wire.request
    status = request("status")
    match(status, engine=engine, backend=backend, initialized=True, model_loaded=True, native_streaming=True)
    match(status["models"], asr=True, vad=True)
    if args.readiness:
        request("exit")
        require(wire.child.wait(timeout=30) == 0, "unclean protocol exit")
        return
    wire.child.stdin.write("{\n")
    wire.child.stdin.flush()
    match(wire.receive("malformed JSON"), success=False)
    request("nonexistent-action", False)
    start_stats = request("stats")["stats"]
    match(start_stats, transcription_count=0)
    n = len(pcm) // 2
    options = {"language": "zh", "context": "", "hot_words": ["Python", " python "]}
    def transcribe(label, mode, **fields):
        result = request("transcribe", options=options, **fields)
        match(result, engine=engine, input_mode=mode, backend=backend)
        require(result["raw_text"] == result["text"] and result["text"].strip(), result)
        require(isinstance(result["language"], str) and result["language"], result)
        require(abs(result["duration"] - n / 16000) < 0.001, result)
        report["transcripts"][label] = [result["text"], result["language"]]
        return result
    path_result = transcribe("file", "path", audio_path=str(audio_path))
    inline = dict(audio_base64=pcm64(pcm), audio_format="pcm_s16le", sample_rate=16000)
    memory_result = transcribe("inline", "memory", audio_path=str(audio_path.with_suffix(".missing")), **inline)
    match(memory_result, text=path_result["text"], language=path_result["language"])
    invalid = request("transcribe", False, audio_path=str(audio_path), audio_base64="!",
                      audio_format="pcm_s16le", sample_rate=16000)
    match(invalid, type="transcription_error", input_mode="memory")
    wav = dict(audio_base64=pcm64(audio_path.read_bytes()), audio_format="wav", sample_rate=16000)
    qwen = engine.startswith("qwen")
    if qwen:
        match(request("transcribe", False, **wav), type="transcription_error")
    else:
        match(transcribe("wav-memory", "memory", **wav), text=path_result["text"])
    for fields in ({"language": 123}, {"hot_words": "invalid"}, {"context": ["invalid"]}):
        match(request("stream_start", False, session_id=1, **fields), type="stream_error")
    start = request("stream_start", session_id=1, language=" zh ", hot_words=["Python", " python "])
    stream_reply(start, engine, backend, 1, 0, False)
    match(start, language="zh", text="", tentative_text="")
    match(request("transcribe", False, audio_path=str(audio_path)), type="stream_busy")
    match(request("stream_start", False, session_id=1), type="stream_error")
    base = dict(session_id=1, offset=0, sample_rate=16000, audio_format="pcm_s16le", audio_base64=pcm64(pcm[:1280]))
    for override in ({"session_id": 2}, {"session_id": True}, {"offset": 1}, {"offset": False},
                     {"sample_rate": 8000}, {"audio_format": "wav"}, {"audio_base64": "AA=="},
                     {"audio_base64": "!!"}, {"preview": 0}):
        match(request("stream_feed", False, **(base | override)), type="stream_error")
    if not qwen:
        match(request("stream_feed", False, **base, preview=False), type="stream_error")
    reply = request("stream_feed", **base)
    stream_reply(reply, engine, backend, 1, 640, False)
    if qwen:
        match(reply, tentative_text="")
    match(request("stream_cancel", False, session_id=2), type="stream_error")
    cancelled = request("stream_cancel", session_id=1)
    stream_reply(cancelled, engine, backend, 1, 640, True)
    match(cancelled, text="")
    request("stream_start", session_id=2)
    request("stream_start", session_id=3, language="zh")
    match(request("stream_finish", False, session_id=2), type="stream_error")
    previous = ""
    for offset in range(0, n, 2560):
        end = min(offset+2560, n)
        fields = dict(base, session_id=3, offset=offset, audio_base64=pcm64(pcm[offset*2:end*2]))
        if qwen:
            fields["preview"] = False
        reply = request("stream_feed", **fields)
        stream_reply(reply, engine, backend, 3, end, False)
        require(reply["text"].startswith(previous), reply)
        previous = reply["text"]
        if qwen:
            match(reply, text="", tentative_text="")
    final = request("stream_finish", session_id=3)
    stream_reply(final, engine, backend, 3, n, True)
    require(final["text"].strip() and final["text"].startswith(previous), final)
    report["transcripts"]["stream"] = [final["text"], final["language"]]
    match(request("stream_finish", False, session_id=3), type="stream_error")
    stats = request("stats")["stats"]
    count = 3 if qwen else 4
    match(stats, transcription_count=count)
    require(abs(stats["total_audio_duration"] - count * n / 16000) <= 0.011, stats)
    request("cleanup")
    idle = request("set_gpu_idle", seconds=1)
    match(idle, enabled=True, gpu_idle_seconds=1)
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        status = request("status")
        if status.get("gpu_suspended") is True:
            break
        time.sleep(0.2)
    match(status, initialized=False, model_loaded=False, gpu_suspended=True)
    match(status["models"], vad=True, asr=False)
    request("set_gpu_idle", seconds=0)
    resumed = transcribe("resumed", "path", audio_path=str(audio_path))
    match(resumed, text=path_result["text"], language=path_result["language"])
    match(request("status"), model_loaded=True, initialized=True)
    request("exit")
    require(wire.child.wait(timeout=30) == 0, "unclean protocol exit")


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--source-root", type=Path, required=True, help="installed-baseline checkout with fixtures + .venv")
    p.add_argument("--candidate", type=Path, help="staged candidate engine.exe; default stages baseline source")
    p.add_argument("--oracle", type=Path, help="same-engine/backend baseline JSON for exact transcript parity")
    p.add_argument("--hub-cache", type=Path, default=Path.home()/".cache/huggingface/hub")
    p.add_argument("--engine", choices=("qwen3-asr-0.6b","confucius4-r2t2"), required=True)
    p.add_argument("--backend", choices=("cpu","cuda"), default="cpu")
    p.add_argument("--readiness", action="store_true")
    p.add_argument("--fixture", default="ascend-test-00081")
    p.add_argument("--output", type=Path, required=True)
    args = p.parse_args()
    root = args.source_root.resolve()
    source = root/"src-tauri/resources"
    fixtures = json.loads((root/"tmp/asr-opt-20261025/natural-quality/fixtures.json").read_text(encoding="utf-8"))
    fixture = next(row for row in fixtures["fixtures"] if row["id"] == args.fixture)
    audio = root/fixture["path"]
    require(digest(audio) == fixture["wav"]["file_sha256"], "fixture SHA mismatch")
    with wave.open(str(audio)) as wav:
        require((wav.getnchannels(), wav.getsampwidth(), wav.getframerate()) == (1,2,16000), "fixture format")
        pcm = wav.readframes(wav.getnframes())
    args.output = args.output.resolve()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    report = dict(engine=args.engine, backend=args.backend, responses=[], transcripts={}, fixture=args.fixture, readiness=args.readiness,
                  fixture_sha256=digest(audio), driver_sha256=digest(Path(__file__)))
    try:
        with tempfile.TemporaryDirectory(prefix="lw-engine-parity-") as tmp:
            temp = Path(tmp)
            if args.candidate:
                require(args.oracle is not None, "a baseline receipt is required for candidate parity")
                command = [str(args.candidate.resolve(strict=True))]
                recorded = json.loads(args.oracle.read_text(encoding="utf-8"))
                match(recorded, passed=True, readiness=args.readiness, engine=args.engine, backend=args.backend, fixture_sha256=digest(audio))
            else:
                stage = temp/"reference"
                stage.mkdir()
                for path in source.glob("*.py"):
                    if not path.name.startswith("test_"):
                        shutil.copy2(path, stage)
                for name in ("fireredvad_vad.onnx","fireredvad_cmvn.json"):
                    shutil.copy2(source/name, stage/name)
                shutil.copytree(source/"r2t2-native"/args.backend, stage/"r2t2-native"/args.backend)
                command = [str(root/".venv/Scripts/python.exe"), str(stage/"engine.py")]
                report["reference_hashes"] = {p.relative_to(stage).as_posix():digest(p) for p in stage.rglob("*") if p.is_file()}
            command += ["serve","--engine",args.engine]
            report.update(command=command, executable_sha256=digest(Path(command[0])))
            env = dict(os.environ, HF_HUB_OFFLINE="1", HF_HUB_CACHE=str(args.hub_cache.resolve()),
                       LIGHT_WHISPER_AUDIO_PROCESSING="0", LIGHT_WHISPER_DATA_DIR=str(temp/"data"),
                       LIGHT_WHISPER_GPU_IDLE_SECONDS="0", PYTHONIOENCODING="utf-8", OMP_NUM_THREADS="2",
                       LIGHT_WHISPER_NATIVE_THREADS="4", PYTHONUNBUFFERED="1")
            with args.output.with_suffix(".stderr.log").open("w", encoding="utf-8") as stderr:
                child = subprocess.Popen(command, cwd=temp, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                         stderr=stderr, text=True, encoding="utf-8",
                                         creationflags=getattr(subprocess,"CREATE_NO_WINDOW",0))
                try:
                    wire = Wire(child,report)
                    contract(wire,args,audio,pcm,report)
                finally:
                    if child.poll() is None:
                        child.kill()
                        child.wait(timeout=20)
                    child.stdin.close()
                    child.stdout.close()
                    wire.reader.join(timeout=2)
                    report["exit_code"] = child.returncode
            if args.candidate:
                require(report["transcripts"] == recorded["transcripts"],
                        f"transcript/language parity: expected {recorded['transcripts']!r}, got {report['transcripts']!r}")
        report["passed"] = True
    except Exception as exc:
        report.update(passed=False, error=f"{type(exc).__name__}: {exc}")
    args.output.write_text(json.dumps(report,indent=2,ensure_ascii=False), encoding="utf-8")
    print(json.dumps({key: report.get(key) for key in ("engine","passed","exit_code","error","transcripts")}))
    return int(not report["passed"])


if __name__ == "__main__":
    raise SystemExit(main())
