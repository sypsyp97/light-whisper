"""Additional isolated public engine scenarios. Shared process owner for dedicated suites."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import wave

from engine_protocol import Wire, digest, match, require


def run_scenario(label, scenario):
    parser = argparse.ArgumentParser(description=label)
    parser.add_argument("--source-root", required=True, type=Path)
    parser.add_argument("--reference-resources", type=Path, help="alternate isolated reference source for mutation checks")
    parser.add_argument("--candidate", type=Path)
    parser.add_argument("--engine", required=True, choices=["qwen3-asr-0.6b","confucius4-r2t2"])
    parser.add_argument("--backend", default="cpu", choices=["cpu","cuda"])
    parser.add_argument("--fixture", default="ascend-test-00081")
    parser.add_argument("--hub-cache", type=Path, default=Path.home()/".cache/huggingface/hub")
    parser.add_argument("--oracle", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    root = args.source_root.resolve()
    source = args.reference_resources or (root / "src-tauri/resources")
    row = next(item for item in json.loads((root/"tmp/asr-opt-20261025/natural-quality/fixtures.json").read_text(
        encoding="utf-8"))["fixtures"] if item["id"] == args.fixture)
    audio_path = root/row["path"]
    require(digest(audio_path) == row["wav"]["file_sha256"], "fixture identity")
    with wave.open(str(audio_path)) as audio:
        pcm = audio.readframes(audio.getnframes())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    report = dict(suite=label, engine=args.engine, backend=args.backend, fixture_sha256=digest(audio_path),
                  responses=[], transcripts={})
    try:
        with tempfile.TemporaryDirectory(prefix="lw-engine-scenario-") as dirname:
            work = Path(dirname)
            if args.candidate:
                command = [str(args.candidate.resolve(strict=True))]
                oracle = json.loads(args.oracle.read_text(encoding="utf-8"))
                match(oracle, passed=True, **{k:report[k] for k in ("suite","engine","backend","fixture_sha256")})
            else:
                stage = work/"reference"
                stage.mkdir()
                for path in source.glob("*.py"):
                    if not path.name.startswith("test_"):
                        shutil.copy2(path,stage)
                for name in ("fireredvad_cmvn.json","fireredvad_vad.onnx"):
                    shutil.copy2(source/name,stage)
                shutil.copytree(source/"r2t2-native"/args.backend,stage/"r2t2-native"/args.backend)
                report["reference_hashes"] = {p.relative_to(stage).as_posix():digest(p) for p in stage.rglob("*") if p.is_file()}
                command = [str(root/".venv/Scripts/python.exe"),str(stage/"engine.py")]
            command += ["serve","--engine",args.engine]
            env = dict(os.environ, HF_HUB_OFFLINE="1", HF_HUB_CACHE=str(args.hub_cache.resolve()),
                       LIGHT_WHISPER_AUDIO_PROCESSING="0", LIGHT_WHISPER_DATA_DIR=str(work/"data"),
                       LIGHT_WHISPER_GPU_IDLE_SECONDS="0", PYTHONIOENCODING="utf-8", OMP_NUM_THREADS="2",
                       LIGHT_WHISPER_NATIVE_THREADS="4", PYTHONUNBUFFERED="1",
                       HTTPS_PROXY="http://127.0.0.1:1", HTTP_PROXY="http://127.0.0.1:1", NO_PROXY="")
            with args.output.with_suffix(".stderr.log").open("w",encoding="utf-8") as stderr:
                process = subprocess.Popen(command,cwd=work,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,
                                           stderr=stderr,text=True,encoding="utf-8",
                                           creationflags=getattr(subprocess,"CREATE_NO_WINDOW",0))
                wire = Wire(process,report)
                try:
                    init = wire.receive("startup")
                    match(init,success=True,engine=args.engine,backend=args.backend,model_loaded=True)
                    scenario(wire,args,audio_path,pcm,report)
                    wire.request("exit")
                    require(process.wait(timeout=30) == 0,"unclean exit")
                finally:
                    process.stdin.close()
                    if process.poll() is None:
                        try:
                            process.wait(timeout=20)
                        except subprocess.TimeoutExpired:
                            process.kill()
                            process.wait(timeout=20)
                    process.stdout.close()
                    wire.reader.join(timeout=2)
                    report["exit_code"] = process.returncode
            if args.candidate:
                require(report["transcripts"] == oracle["transcripts"],
                        f"expected {oracle['transcripts']!r}, got {report['transcripts']!r}")
        report["passed"] = True
    except Exception as error:
        report.update(passed=False,error=f"{type(error).__name__}: {error}")
    args.output.write_text(json.dumps(report,indent=2,ensure_ascii=False),encoding="utf-8")
    print(json.dumps({k:report.get(k) for k in ("suite","passed","error","transcripts")}))
    return int(not report["passed"])
