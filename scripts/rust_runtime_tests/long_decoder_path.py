"""Use the actual Windows sidecar to decode natural WAV across the legacy MAX_PATH boundary."""
import argparse
import json
import os
from pathlib import Path
import shutil
import tempfile

from engine_protocol import digest, match, require
from installed_roundtrip import process
from wake_backend_fallback import link_copy


def extended(path):
    value=str(path.absolute())
    if value.startswith("\\\\?\\"):
        return path
    if value.startswith("\\\\"):
        return Path("\\\\?\\UNC\\"+value[2:])
    return Path("\\\\?\\"+value)


def units(path):
    return len(str(path).encode("utf-16-le"))//2


def pad_directory(parent, size):
    folder=parent
    while units(folder)<size:
        left=size-units(folder)-1
        require(left>0,"workspace path cannot fit the requested boundary")
        piece=min(90,left)
        if left-piece==1:
            piece-=1
        folder=folder/("p"*piece)
    extended(folder).mkdir(parents=True,exist_ok=True)
    require(units(folder)==size,"wrong controlled directory length")
    return folder


def run(a,report):
    native=extended(a.candidate.resolve(strict=True))
    source=a.source_root.resolve()
    oracle=json.loads(a.oracle.read_text(encoding="utf-8"))
    match(oracle,passed=True,backend="cpu",fixture=a.fixture,engine=a.engine)
    data=json.loads((source/"tmp/asr-opt-20261025/natural-quality/fixtures.json").read_text(encoding="utf-8"))
    reference=source/next(r for r in data["fixtures"] if r["id"]==a.fixture)["path"]
    match(oracle,fixture_sha256=digest(reference))
    a.work_root=a.work_root.resolve()
    a.work_root.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="path-",dir=extended(a.work_root)) as tmp:
        work=a.work_root/Path(tmp).name
        leaf=Path("_internal/_soundfile_data/libsndfile_x64.dll")
        stage=pad_directory(work,263-units(leaf)-1) if a.case=="dll" else work/"engine"
        stage.mkdir(exist_ok=True)
        if a.case=="audio":
            input_dir=pad_directory(work,263-units(Path("input.wav"))-1)
        else:
            input_dir=work
        wav=input_dir/"input.wav"
        shutil.copyfile(reference,extended(wav))
        for item in native.parent.iterdir():
            if item.is_file() and item.suffix.lower() in (".exe",".dll") and not item.name.lower().startswith(("cublas","cudart")):
                link_copy(item,extended(stage/item.name))
        src=native.parent/"_internal"
        dest=extended(stage/"_internal")
        dest.mkdir()
        for name in ("fireredvad_vad.onnx","fireredvad_cmvn.json"):
            link_copy(src/name,dest/name)
        shutil.copytree(src/"r2t2-native/cpu",dest/"r2t2-native/cpu",copy_function=link_copy)
        shutil.copytree(src/"_soundfile_data",dest/"_soundfile_data",copy_function=link_copy)
        dll=stage/leaf
        report.update(audio_utf16_units=units(wav),dll_utf16_units=units(dll),exe_sha256=digest(native))
        require((units(dll)>259)==(a.case=="dll") and (units(wav)>259)==(a.case=="audio"),
                "the test must isolate a single long path")
        env=dict(os.environ,HF_HUB_CACHE=str(a.hub_cache.resolve()),HF_HUB_OFFLINE="1",
                 LIGHT_WHISPER_DATA_DIR=str(work/"data"),LIGHT_WHISPER_GPU_IDLE_SECONDS="0",
                 LIGHT_WHISPER_AUDIO_PROCESSING="0",LIGHT_WHISPER_NATIVE_THREADS="4")
        with process([stage/native.name,"serve","--engine",a.engine],env,work,report,"engine") as wire:
            match(wire.receive("startup"),success=True,backend="cpu",model_loaded=True)
            result=wire.request("transcribe",audio_path=str(wav),options={"language":"zh","hot_words":["Python"]})
            require([result["text"],result["language"]]==oracle["transcripts"]["file"],result)
            wire.request("exit")
            require(wire.child.wait(timeout=30)==0,"unclean decoder exit")


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--candidate",type=Path,required=True)
    p.add_argument("--source-root",type=Path,required=True)
    p.add_argument("--work-root",type=Path,required=True)
    p.add_argument("--oracle",type=Path,required=True)
    p.add_argument("--output",type=Path,required=True)
    p.add_argument("--case",choices=("dll","audio"),required=True)
    p.add_argument("--hub-cache",type=Path,default=Path.home()/".cache/huggingface/hub")
    p.add_argument("--fixture",default="ascend-test-00081")
    p.add_argument("--engine",default="qwen3-asr-0.6b")
    a=p.parse_args()
    report=dict(case=a.case,responses=[])
    try:
        run(a,report)
        report["passed"]=True
    except Exception as exc:
        report.update(passed=False,error=f"{type(exc).__name__}: {exc}")
    a.output.parent.mkdir(parents=True,exist_ok=True)
    a.output.write_text(json.dumps(report,indent=2,ensure_ascii=False),encoding="utf-8")
    print(json.dumps({k:report.get(k) for k in ("passed","error","audio_utf16_units","dll_utf16_units")}))
    return int(not report["passed"])

if __name__=="__main__":
    raise SystemExit(main())
