"""Exercise missing-sidecar discovery through the windowless desktop host and its client."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from engine_protocol import Wire, digest, match, require


def check(host_exe, root, report, case, probe, marker):
    with tempfile.TemporaryDirectory(prefix="lw-host-fallback-") as folder:
        tmp=Path(folder)
        host=tmp/"light-whisper.exe"
        shutil.copy2(host_exe,host)
        resources=tmp/"resources"
        resources.mkdir()
        # A usable interpreter and the original development scripts are intentionally available.
        # An isolated empty model cache prevents model inference on the legacy fallback.
        for path in (root/"src-tauri/resources").glob("*.py"):
            if case=="missing" and not path.name.startswith("test_"):
                shutil.copy2(path,resources)
        data=tmp/"data"
        data.mkdir()
        if case!="missing":
            require(probe and marker,"a known test sidecar and its observed startup error are required")
            locations={"legacy-loose":resources/"python-dist/engine","legacy-cache":data/"engine",
                       "native-loose":resources/"engine"}
            location=locations[case]
            location.mkdir(parents=True)
            shutil.copy2(probe,location/"engine.exe")
            if case=="legacy-cache":
                version=json.loads((root/"package.json").read_text(encoding="utf-8"))["version"]
                (location/".version").write_text(version,encoding="ascii")
        cache=tmp/"empty-model-cache"
        cache.mkdir()
        (data/"engine.json").write_text(json.dumps({"engine":"qwen3-asr-0.6b","models_dir":str(cache)}),
                                        encoding="utf-8")
        env=dict(os.environ,LIGHT_WHISPER_DATA_DIR=str(data),HF_HUB_OFFLINE="1",HF_HUB_CACHE=str(cache),
                 PATH=str(root/".venv/Scripts")+os.pathsep+os.environ["PATH"],PYTHONIOENCODING="utf-8",
                 HTTP_PROXY="http://127.0.0.1:1",HTTPS_PROXY="http://127.0.0.1:1",NO_PROXY="",
                 WEBVIEW2_USER_DATA_FOLDER=str(tmp/"webview2"))
        options=dict(cwd=tmp,env=env,text=True,encoding="utf-8",
                     creationflags=getattr(subprocess,"CREATE_NO_WINDOW",0))
        with (tmp/"host-stderr.log").open("w",encoding="utf-8") as stderr:
            child=subprocess.Popen([str(host),"--agent-host"],stdin=subprocess.PIPE,stdout=subprocess.PIPE,
                                   stderr=stderr,**options)
            wire=Wire(child,report)
            try:
                welcome=wire.receive("host readiness")
                match(welcome,success=True,surface="desktop-host",window_count=0,pid=child.pid)
                commands=[{"action":"engine_get","request_id":10},{"action":"engine_start","request_id":11},
                          {"action":"exit","request_id":12}]
                result=subprocess.run([str(host),"--agent-client"],capture_output=True,
                                      input="".join(json.dumps(row)+"\n" for row in commands),timeout=60,**options)
                replies=[json.loads(row) for row in result.stdout.splitlines()]
                report["client_replies"]=replies
                require(result.returncode==0,f"agent-client failed with {result.returncode}")
                require(len(replies)==4,f"expected three client responses after readiness: {replies}")
                match(replies[0],success=True,surface="desktop-client")
                match(replies[1],success=True,request_id=10)
                match(replies[2],success=False,request_id=11)
                match(replies[3],success=True,request_id=12)
                logs=sorted(path.relative_to(data).as_posix() for path in data.rglob("*.log"))
                report["data_log_paths"]=logs
                if case!="missing":
                    selected=marker in replies[2].get("error","")
                    require(selected==(case=="native-loose"),
                            f"{case}: observed probe selection={selected}, error={replies[2].get('error')!r}")
                require(not (data/"logs/qwen3_asr_server.log").exists(),
                        "missing native sidecar launched the Python ASR server")
            finally:
                if child.poll() is None:
                    child.stdin.write('{"action":"exit"}\n')
                    child.stdin.flush()
                    child.stdin.close()
                    try:
                        child.wait(timeout=20)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait(timeout=20)
                child.stdout.close()
                wire.reader.join(timeout=2)
                report["host_exit_code"]=child.returncode
        require(child.returncode==0,"unclean host shutdown")


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--case",choices=("missing","legacy-loose","legacy-cache","native-loose"),default="missing")
    p.add_argument("--probe",type=Path,help="compiled test sidecar returning a distinctive init_error")
    p.add_argument("--marker",help="known startup error substring from test sidecar")
    p.add_argument("--host",required=True,type=Path)
    p.add_argument("--source-root",required=True,type=Path)
    p.add_argument("--output",required=True,type=Path)
    a=p.parse_args()
    report=dict(host_sha256=digest(a.host),case=a.case,probe_sha256=digest(a.probe) if a.probe else None,responses=[])
    try:
        check(a.host,a.source_root.resolve(),report,a.case,a.probe,a.marker)
        report["passed"]=True
    except Exception as exc:
        report.update(passed=False,error=f"{type(exc).__name__}: {exc}")
    a.output.parent.mkdir(parents=True,exist_ok=True)
    a.output.write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding="utf-8")
    print(json.dumps({k:report.get(k) for k in ("passed","error","host_exit_code","data_log_paths")}))
    return int(not report["passed"])

if __name__=="__main__":
    raise SystemExit(main())
