"""Fresh installer payload -> native CLI -> windowless host -> extracted engine -> real model."""
import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import shutil
import statistics
import subprocess
import tempfile
import time

from engine_protocol import Wire, digest, match, require
from model_download import MODELS


@contextmanager
def process(command, env, work, report, label):
    with (work/(label+".stderr.log")).open("w",encoding="utf-8") as err:
        child=subprocess.Popen([str(x) for x in command],env=env,cwd=work,stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE,stderr=err,text=True,encoding="utf-8",
                               creationflags=getattr(subprocess,"CREATE_NO_WINDOW",0))
        wire=Wire(child,report)
        try:
            yield wire
        finally:
            child.stdin.close()
            try:
                child.wait(timeout=30)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(timeout=20)
            child.stdout.close()
            wire.reader.join(timeout=2)
            report.setdefault("exit_codes",{})[label]=child.returncode


def run(a,report):
    payload=a.payload.resolve()
    root=a.source_root.resolve()
    host=payload/"light-whisper.exe"
    cli=payload/"light-whisper-cli.exe"
    report["host_sha256"]=digest(host)
    report["cli_sha256"]=digest(cli)
    oracle=json.loads(a.oracle.read_text(encoding="utf-8"))
    match(oracle,passed=True,engine=a.engine,fixture=a.fixture)
    fixtures=json.loads((root/"tmp/asr-opt-20261025/natural-quality/fixtures.json").read_text(encoding="utf-8"))
    row=next(r for r in fixtures["fixtures"] if r["id"]==a.fixture)
    audio=root/row["path"]
    match(row["wav"],file_sha256=digest(audio))
    match(oracle,fixture_sha256=digest(audio))
    a.work_root=a.work_root.resolve()
    a.work_root.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="package-smoke-",dir=a.work_root) as tmp:
        work=Path(tmp)
        data=work/"app-data"
        data.mkdir()
        cache=work/"hub"
        env=dict(os.environ,LIGHT_WHISPER_DATA_DIR=str(data),HF_HUB_CACHE=str(cache),HF_HUB_OFFLINE="1",
                 PYTHONIOENCODING="utf-8",PYTHONUNBUFFERED="1",LIGHT_WHISPER_AUDIO_PROCESSING="0",
                 LIGHT_WHISPER_GPU_IDLE_SECONDS="0",LIGHT_WHISPER_NATIVE_THREADS="4",OMP_NUM_THREADS="2",
                 WEBVIEW2_USER_DATA_FOLDER=str(work/"webview2"),HTTP_PROXY="http://127.0.0.1:1",
                 HTTPS_PROXY="http://127.0.0.1:1",NO_PROXY="127.0.0.1,localhost")
        system32=Path(os.environ["SystemRoot"])/"System32"
        gpu_tool=shutil.which("nvidia-smi.exe")
        dirs=[system32]
        if gpu_tool and not any(Path(gpu_tool).parent.samefile(directory) for directory in dirs):
            dirs.append(Path(gpu_tool).resolve().parent)
        env["PATH"]=os.pathsep.join(map(str,dirs))
        report["child_path"]=env["PATH"]
        tools={}
        for tool in ("python.exe","python3.exe","py.exe","node.exe","cargo.exe","rustc.exe","nvidia-smi.exe"):
            check=subprocess.run([str(system32/"where.exe"),tool],env=env,cwd=work,capture_output=True,
                                 text=True,encoding="utf-8",errors="replace",timeout=10)
            require(check.returncode in (0,1),f"where.exe lookup failed for {tool}")
            tools[tool]=check.stdout.splitlines() if check.returncode==0 else []
        report["child_path_executables"]=tools
        require(not any(entries for tool,entries in tools.items() if tool!="nvidia-smi.exe"),
                f"development tools remain discoverable on product PATH: {tools}")
        def timed(label,fn):
            before=time.perf_counter()
            try:
                return fn()
            finally:
                report["timings_ms"].setdefault(label,[]).append(round((time.perf_counter()-before)*1000,3))
        def once(*args,using=None):
            result=subprocess.run([str(cli),*map(str,args)],env=env if using is None else using,cwd=work,text=True,encoding="utf-8",
                                  capture_output=True,timeout=180,creationflags=getattr(subprocess,"CREATE_NO_WINDOW",0))
            require(result.returncode==0,f"CLI {args}: {result.stdout!r}, {result.stderr[-400:]!r}")
            return json.loads(result.stdout)
        doctor=once("--target","app","--app-exe",host,"doctor")
        match(doctor,success=True,target="app",cli_runtime="rust",frozen=False,app_protocol_supported=True)
        require(Path(doctor["runtime"]).samefile(host),"doctor selected a different host")
        package_archive=payload/"resources/engine.tar.xz"
        require(package_archive.is_file(),"fresh extracted installer must contain resources/engine.tar.xz")
        repo,revision,name,size,sha=MODELS[a.engine]
        rel=Path("models--"+repo.replace("/","--"))/"snapshots"/revision/name
        weight=cache/rel
        weight.parent.mkdir(parents=True)
        shutil.copyfile(a.hub_cache/rel,weight)
        require(weight.stat().st_size==size and digest(weight)==sha,"private model copy differs from pinned baseline")
        (data/"engine.json").write_text(json.dumps({"engine":a.engine,"models_dir":str(cache)}),encoding="utf-8")
        def golden(reply):
            require(reply.get("success") is True,reply)
            require([reply["text"],reply["language"]]==oracle["transcripts"]["file"],
                    f"expected {oracle['transcripts']['file']!r}, received {reply!r}")
        # Bootstrap in a separate empty data root so the following host setup also starts fresh.
        cli_data=work/"standalone-data"
        cli_data.mkdir()
        shutil.copy2(data/"engine.json",cli_data/"engine.json")
        standalone_env=dict(env,LIGHT_WHISPER_DATA_DIR=str(cli_data))
        implicit=timed("cli_standalone_extract_and_status",lambda:once("--target","engine","status",using=standalone_env))
        match(implicit,success=True,engine=a.engine,model_loaded=True,initialized=True)
        implicit_result=timed("cli_standalone_transcribe",lambda:once("--target","engine","transcribe",audio,
                             "--language","zh","--hot-word","Python",using=standalone_env))
        golden(implicit_result)
        report["standalone_result"]=implicit_result
        resolved=once("--target","engine","doctor",using=standalone_env)
        match(resolved,success=True,cli_runtime="rust",runtime_present=True)
        selected=Path(resolved["runtime"]).resolve(strict=True)
        require(selected.is_relative_to(cli_data.resolve()),f"discovered sidecar outside private test root: {selected}")
        report["standalone_engine_sha256"]=digest(selected)
        args=["--target","app","--app-exe",host]
        with process([host,"--agent-host"],env,work,report,"host") as h:
            welcome=timed("host_ready",lambda:h.receive("host ready"))
            match(welcome,success=True,surface="desktop-host",window_count=0,pid=h.child.pid)
            require(not (data/"engine").exists(),"host smoke must start without cached sidecar")
            start=timed("cli_host_extract_and_start",lambda:once(*args,"engine","start"))
            match(start,success=True,configured_engine=a.engine)
            status=once(*args,"status")
            match(status,success=True,ready=True,running=True,model_loaded=True,runtime_observed=True,
                  runtime_engine=a.engine,configured_engine=a.engine)
            # Actual public transcribe command, including discovery and client process creation.
            result=timed("cli_host_transcribe",lambda:once(*args,"transcribe",audio,"--language","zh","--hot-word","Python"))
            golden(result)
            report["host_result"]=result
            stats=once(*args,"stats")
            match(stats["stats"],transcription_count=1)
            h.child.stdin.write('{"action":"exit"}\n')
            h.child.stdin.flush()
            require(h.child.wait(timeout=30)==0,"windowless host did not exit cleanly")
        engine=data/"engine/engine.exe"
        require(engine.is_file(),"host did not materialize engine.exe from the package")
        report["engine_sha256"]=digest(engine)
        require(report["engine_sha256"]==report["standalone_engine_sha256"],"host and CLI selected different payload bytes")
        direct_args=["--target","engine","--engine-exe",engine,"--engine",a.engine]
        direct=timed("cli_direct_cold_transcribe",lambda:once(*direct_args,"transcribe",audio,
                                                              "--language","zh","--hot-word","Python"))
        golden(direct)
        report["direct_result"]=direct
        if a.compare_source:
            # Alternate complete owned sessions. Keep a single model resident for every timed call.
            # Each session performs one untimed decode before its measured decode.
            def sample(kind,index):
                command=([root/".venv/Scripts/python.exe",root/"src-tauri/resources/engine.py"]
                         if kind=="source" else [engine])
                label=f"{kind}-{index}"
                with process(command+["serve","--engine",a.engine],env,work,report,label) as wire:
                    init=wire.receive(label+" init")
                    match(init,success=True,backend=oracle["backend"],model_loaded=True)
                    def query():
                        return wire.request("transcribe",audio_path=str(audio),options={"language":"zh","hot_words":["Python"]})
                    golden(query())
                    golden(timed("preloaded_"+kind,query))
                    wire.request("exit")
                    require(wire.child.wait(timeout=30)==0,"sidecar shutdown")
            for index in range(3):
                for kind in (("source","native") if index%2==0 else ("native","source")):
                    sample(kind,index)
            report["preloaded_median_ms"]={kind:statistics.median(report["timings_ms"]["preloaded_"+kind])
                                           for kind in ("source","native")}
        require(all(code==0 for code in report["exit_codes"].values()),"unclean child exit")


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--payload",type=Path,required=True)
    p.add_argument("--source-root",type=Path,required=True)
    p.add_argument("--oracle",type=Path,required=True)
    p.add_argument("--work-root",type=Path,required=True)
    p.add_argument("--hub-cache",type=Path,default=Path.home()/".cache/huggingface/hub")
    p.add_argument("--engine",choices=MODELS,default="qwen3-asr-0.6b")
    p.add_argument("--fixture",default="ascend-test-00081")
    p.add_argument("--compare-source",action="store_true")
    p.add_argument("--output",type=Path,required=True)
    a=p.parse_args()
    report=dict(engine=a.engine,responses=[],timings_ms={})
    try:
        run(a,report)
        report["passed"]=True
    except Exception as exc:
        report.update(passed=False,error=f"{type(exc).__name__}: {exc}")
    a.output.parent.mkdir(parents=True,exist_ok=True)
    a.output.write_text(json.dumps(report,indent=2,ensure_ascii=False),encoding="utf-8")
    print(json.dumps({k:report.get(k) for k in ("passed","error","timings_ms","preloaded_median_ms")}))
    return int(not report["passed"])

if __name__=="__main__":
    raise SystemExit(main())
