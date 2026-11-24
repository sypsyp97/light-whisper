"""Missing offline weights or staged CPU assets must produce a recoverable startup failure."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from engine_protocol import Wire, match, require


def check(args,report):
    root=args.source_root.resolve()
    with tempfile.TemporaryDirectory(prefix="lw-missing-engine-asset-") as temp:
        work=Path(temp)
        resource=root/"src-tauri/resources"
        if args.candidate:
            native=args.candidate.resolve(strict=True)
            stage=work/"engine"
            stage.mkdir()
            src=native.parent
            for item in src.iterdir():
                if item.is_file() and item.name.lower().endswith((".exe",".dll")) and not item.name.lower().startswith(("cublas","cudart")):
                    shutil.copy2(item,stage/item.name)
            src=src/"_internal"
            dest=stage/"_internal"
            dest.mkdir()
            command=[str(stage/native.name)]
        else:
            dest=work/"reference"
            dest.mkdir()
            src=resource
            for file in src.glob("*.py"):
                if not file.name.startswith("test_"):
                    shutil.copy2(file,dest)
            command=[str(root/".venv/Scripts/python.exe"),str(dest/"engine.py")]
        shutil.copytree(src/"r2t2-native/cpu",dest/"r2t2-native/cpu")
        for name in ("fireredvad_vad.onnx","fireredvad_cmvn.json"):
            shutil.copy2(src/name,dest/name)
        # Omit only the target asset after staging known-good files.
        if args.case=="vad":
            (dest/"fireredvad_vad.onnx").unlink()
        elif args.case=="backend":
            (dest/"r2t2-native/cpu/audiocpp.dll").unlink()
        cache=work/"empty-cache" if args.case=="weights" else args.hub_cache.resolve()
        if args.case=="weights":
            cache.mkdir()
        env=dict(os.environ,HF_HUB_CACHE=str(cache),HF_HUB_OFFLINE="1",
                 LIGHT_WHISPER_DATA_DIR=str(work/"data"),LIGHT_WHISPER_AUDIO_PROCESSING="0",
                 HTTP_PROXY="http://127.0.0.1:1",HTTPS_PROXY="http://127.0.0.1:1",NO_PROXY="",
                 PYTHONIOENCODING="utf-8",LIGHT_WHISPER_GPU_IDLE_SECONDS="0",OMP_NUM_THREADS="2")
        with args.output.with_suffix(".stderr.log").open("w",encoding="utf-8") as stderr:
            child=subprocess.Popen(command+["serve","--engine",args.engine],cwd=work,env=env,stdin=subprocess.PIPE,
                                   stdout=subprocess.PIPE,stderr=stderr,text=True,encoding="utf-8",
                                   creationflags=getattr(subprocess,"CREATE_NO_WINDOW",0))
            wire=Wire(child,report)
            try:
                init=wire.receive("startup")
                error_type="models_not_downloaded" if args.case=="weights" else "init_error"
                match(init,success=False,engine=args.engine,type=error_type)
                status=wire.request("status")
                match(status,engine=args.engine,model_loaded=False,initialized=False)
                match(wire.request("stream_start",False,session_id=1),type=error_type)
                match(wire.request("stats")["stats"],initialized=False,transcription_count=0)
                wire.request("exit")
                require(child.wait(timeout=30)==0,"failure exit/cleanup")
            finally:
                child.stdin.close()
                try:
                    child.wait(timeout=20)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait(timeout=20)
                child.stdout.close()
                wire.reader.join(timeout=2)
                report["exit_code"]=child.returncode

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--source-root",required=True,type=Path)
    p.add_argument("--candidate",type=Path)
    p.add_argument("--engine",required=True,choices=("qwen3-asr-0.6b","confucius4-r2t2"))
    p.add_argument("--case",required=True,choices=("weights","vad","backend"))
    p.add_argument("--hub-cache",type=Path,default=Path.home()/".cache/huggingface/hub")
    p.add_argument("--output",required=True,type=Path)
    args=p.parse_args()
    args.output.parent.mkdir(parents=True,exist_ok=True)
    report=dict(engine=args.engine,case=args.case,responses=[])
    try:
        check(args,report)
        report["passed"]=True
    except Exception as exc:
        report.update(passed=False,error=f"{type(exc).__name__}: {exc}")
    args.output.write_text(json.dumps(report,indent=2,ensure_ascii=False),encoding="utf-8")
    print(json.dumps({k:report.get(k) for k in ("passed","error","exit_code")}))
    return int(not report["passed"])

if __name__=="__main__":
    raise SystemExit(main())
