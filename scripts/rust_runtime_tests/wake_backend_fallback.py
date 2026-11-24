"""Wake a previously working CUDA runtime after its private CUDA library becomes unavailable."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

from engine_protocol import Wire, digest, match, require


def link_copy(source,dest):
    try:
        os.link(source,dest)
    except OSError:
        shutil.copy2(source,dest)


def run(a,report):
    source=a.source_root.resolve()
    baseline=source/"src-tauri/resources"
    fixture=json.loads((source/"tmp/asr-opt-20261025/natural-quality/fixtures.json").read_text(encoding="utf-8"))
    row=next(r for r in fixture["fixtures"] if r["id"]==a.fixture)
    wave=source/row["path"]
    require(digest(wave)==row["wav"]["file_sha256"],"fixture SHA mismatch")
    a.work_root.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="wake-fallback-",dir=a.work_root) as directory:
        stage=Path(directory)
        if a.candidate:
            original=a.candidate.resolve(strict=True)
            report["exe_sha256"]=digest(original)
            for file in original.parent.iterdir():
                if file.is_file() and file.suffix.lower() in (".exe",".dll"):
                    link_copy(file,stage/file.name)
            src=original.parent/"_internal"
            dest=stage/"_internal"
            dest.mkdir()
            # Keep all packaged shared non-backend DLLs in the original loader location.
            for file in src.iterdir():
                if file.is_file() and file.suffix.lower()==".dll":
                    link_copy(file,dest/file.name)
            command=[str(stage/original.name)]
        else:
            src=baseline
            dest=stage
            for path in src.glob("*.py"):
                if not path.name.startswith("test_"):
                    shutil.copy2(path,dest)
            command=[str(source/".venv/Scripts/python.exe"),str(dest/"engine.py")]
        for name in ("fireredvad_vad.onnx","fireredvad_cmvn.json"):
            link_copy(src/name,dest/name)
        for backend in ("cpu","cuda"):
            backend_src=src/"r2t2-native"/backend
            if not backend_src.is_dir():
                backend_src=baseline/"r2t2-native"/backend
            shutil.copytree(backend_src,dest/"r2t2-native"/backend,copy_function=link_copy)
        if (src/"_soundfile_data").is_dir():
            shutil.copytree(src/"_soundfile_data",dest/"_soundfile_data",copy_function=link_copy)
        command += ["serve","--engine",a.engine]
        env=dict(os.environ,HF_HUB_OFFLINE="1",HF_HUB_CACHE=str(a.hub_cache.resolve()),
                 LIGHT_WHISPER_DATA_DIR=str(stage/"data"),LIGHT_WHISPER_AUDIO_PROCESSING="0",
                 LIGHT_WHISPER_GPU_IDLE_SECONDS="0",OMP_NUM_THREADS="2",PYTHONIOENCODING="utf-8",
                 LIGHT_WHISPER_NATIVE_THREADS="4",PYTHONUNBUFFERED="1")
        with a.output.with_suffix(".stderr.log").open("w",encoding="utf-8") as stderr:
            child=subprocess.Popen(command,env=env,cwd=stage,stdin=subprocess.PIPE,stdout=subprocess.PIPE,
                                   stderr=stderr,text=True,encoding="utf-8",
                                   creationflags=getattr(subprocess,"CREATE_NO_WINDOW",0))
            wire=Wire(child,report)
            try:
                match(wire.receive("startup"),success=True,backend="cuda",model_loaded=True)
                request=wire.request
                # Inline PCM works independently of libsndfile, which is unrelated to wake selection.
                import wave as wav_reader
                from engine_protocol import pcm64
                with wav_reader.open(str(wave)) as wav:
                    inline=pcm64(wav.readframes(wav.getnframes()))
                fields=dict(audio_base64=inline,audio_format="pcm_s16le",sample_rate=16000,options={"language":"zh"})
                before=request("transcribe",**fields)
                match(before,backend="cuda",engine=a.engine)
                require(before["text"].strip(),before)
                match(request("set_gpu_idle",seconds=1),enabled=True)
                deadline=time.monotonic()+15
                while time.monotonic()<deadline:
                    status=request("status")
                    if status.get("gpu_suspended") is True:
                        break
                    time.sleep(.2)
                match(status,initialized=False,model_loaded=False,gpu_suspended=True)
                lib=dest/"r2t2-native/cuda/audiocpp.dll"
                lib.parent.rename(lib.parent.with_name("disabled-by-test-cuda"))
                require(not lib.exists() and (dest/"r2t2-native/cpu/audiocpp.dll").is_file(),"invalid fallback fixture")
                match(request("set_gpu_idle",seconds=0),enabled=False)
                resumed=request("transcribe",**fields)
                match(resumed,backend="cpu",engine=a.engine)
                require(resumed["text"].strip() and resumed["language"],resumed)
                match(request("status"),backend="cpu",initialized=True,model_loaded=True)
                request("exit")
                require(child.wait(timeout=30)==0,"unclean exit")
            finally:
                child.stdin.close()
                try:
                    child.wait(timeout=30)
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
    p.add_argument("--work-root",required=True,type=Path)
    p.add_argument("--engine",choices=("qwen3-asr-0.6b","confucius4-r2t2"),required=True)
    p.add_argument("--hub-cache",type=Path,default=Path.home()/".cache/huggingface/hub")
    p.add_argument("--fixture",default="ascend-test-00081")
    p.add_argument("--output",required=True,type=Path)
    a=p.parse_args()
    a.output.parent.mkdir(parents=True,exist_ok=True)
    report=dict(engine=a.engine,responses=[])
    try:
        run(a,report)
        report["passed"]=True
    except Exception as exc:
        report.update(passed=False,error=f"{type(exc).__name__}: {exc}")
    a.output.write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding="utf-8")
    print(json.dumps({k:report.get(k) for k in ("engine","passed","error","exit_code")}))
    return int(not report["passed"])

if __name__=="__main__":
    raise SystemExit(main())
