"""Drive engine download with a local HTTP fixture and an isolated cache."""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading

from engine_protocol import digest, match, require

MODELS = {
    "qwen3-asr-0.6b": ("audio-cpp/audio.cpp-gguf", "f861648c0c1f4da9f18e9a0ce5e217b0679bb2f5",
                       "Qwen3-ASR-0.6B-GGUF/qwen3-asr-0.6b-q8_0.gguf",1151272416,
                       "6c44ec2fb4cee513892d7863c1fcc3ea6b699ffa4d899b0ef4ab19956d9544f7"),
    "confucius4-r2t2": ("davidxifeng/Confucius4-R2T2-gguf","a8e6b385d7df7eae9519363e07034a209004797a",
                        "r2t2-q8_0.gguf",2477512064,"19f5ccd624484bcb5d44301437de41560b0ecc40c430e8850dfeefefbe82ccf5"),
}


def run(args,report):
    repo,revision,name,size,sha = MODELS[args.engine]
    pinned_url = f"/{repo}/resolve/{revision}/{name}"
    folder = Path("models--"+repo.replace("/","--"))
    snapshot = folder/"snapshots"/revision
    original = args.hub_cache/snapshot/name
    tail_size = 4096
    with tempfile.TemporaryDirectory(prefix="lw-engine-download-") as tmp:
        work = Path(tmp)
        cache = work/"hub"
        target = cache/snapshot/name
        partial = target.with_name(target.name+".incomplete")
        manifest = cache/snapshot/".light_whisper_complete.json"
        partial.parent.mkdir(parents=True)
        sentinel = partial.parent/"unrelated.gguf"
        sentinel.write_bytes(b"preserve other files")
        if args.case == "resume":
            require(original.stat().st_size == size and digest(original) == sha,"cached model SHA mismatch")
            with partial.open("wb") as dest, original.open("rb") as source:
                left = size-tail_size
                while left:
                    chunk = source.read(min(left,4*1024*1024))
                    dest.write(chunk)
                    left -= len(chunk)
                tail = source.read()
        else:
            partial.write_bytes(b"unfinished")
        paused=cache/folder/"blobs"/"other-client-transfer.incomplete"
        if args.case=="shared-partial":
            paused.parent.mkdir(parents=True)
            paused.write_bytes(b"independent paused transfer")
        calls = report["http"]
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                offset = self.headers.get("Range")
                calls.append([self.path,offset])
                if (args.case=="resume" and self.path==pinned_url and offset==f"bytes={size-tail_size}-"):
                    self.send_response(206)
                    self.send_header("Content-Length",str(tail_size))
                    self.send_header("Content-Range",f"bytes {size-tail_size}-{size-1}/{size}")
                    self.end_headers()
                    self.wfile.write(tail)
                else:
                    self.send_error(503)
            def log_message(self,*args):
                pass
        with ThreadingHTTPServer(("127.0.0.1",0),Handler) as http:
            worker = threading.Thread(target=http.serve_forever,daemon=True)
            worker.start()
            try:
                endpoint = f"http://127.0.0.1:{http.server_port}"
                env = dict(os.environ,HF_ENDPOINT=endpoint,HF_FALLBACK_ENDPOINT=endpoint,
                           HF_HUB_CACHE=str(cache),LIGHT_WHISPER_DATA_DIR=str(work/"data"),
                           PYTHONIOENCODING="utf-8",NO_PROXY="127.0.0.1",no_proxy="127.0.0.1",
                           HTTP_PROXY="http://127.0.0.1:1",HTTPS_PROXY="http://127.0.0.1:1")
                root = args.source_root.resolve()
                command = ([str(args.candidate.resolve(strict=True))] if args.candidate else
                           [str(root/".venv/Scripts/python.exe"),str(root/"src-tauri/resources/engine.py")])
                command += ["download","--engine",args.engine]
                def download():
                    result = subprocess.run(command,cwd=work,env=env,capture_output=True,text=True,
                                            encoding="utf-8",timeout=120,
                                            creationflags=getattr(subprocess,"CREATE_NO_WINDOW",0))
                    events = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
                    report["attempts"].append(dict(events=events,exit_code=result.returncode,stderr=result.stderr[-1500:]))
                    require(events,"downloader emitted no terminal JSON")
                    progress = events[:-1]
                    require(progress and all(row.get("model")=="asr" and row.get("total")==1
                                             and "overall_progress" in row for row in progress),
                            f"missing download progress records: {events}")
                    return events[-1],progress
                result,progress = download()
                if args.case in ("unavailable","shared-partial"):
                    match(result,success=False,failed_models=["asr"])
                    match(result["results"]["asr"],success=False,model="asr")
                    match(progress[-1],stage="error",completed=1)
                    require(calls==[[pinned_url,"bytes=10-"]],f"wrong pinned range request: {calls}")
                    require(not target.exists() and not manifest.exists(),"partial file published")
                    require(partial.read_bytes()==b"unfinished","lost resumable partial")
                    if args.case=="shared-partial":
                        require(paused.is_file() and paused.read_bytes()==b"independent paused transfer",
                                "downloader removed or modified another client's paused blob transfer")
                else:
                    match(result,success=True)
                    match(progress[-1],stage="completed",progress=100,completed=1)
                    require(calls==[[pinned_url,f"bytes={size-tail_size}-"]],f"wrong resume interaction: {calls}")
                    require(digest(target)==sha and not partial.exists(),"published incorrect model")
                    data = json.loads(manifest.read_text(encoding="utf-8"))
                    match(data,repo_id=repo,commit_hash=revision,files=[dict(path=name,size=size,sha256=sha)])
                    require((cache/folder/"refs/main").read_text().strip()==revision,"incorrect pinned ref")
                    result,progress = download()
                    match(result,success=True)
                    match(progress[-1],stage="completed",progress=100)
                    require(len(calls)==1,"verified cache accessed network")
                require(sentinel.read_bytes()==b"preserve other files","unrelated snapshot content changed")
            finally:
                http.shutdown()
                worker.join(timeout=2)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--source-root",type=Path,required=True)
    p.add_argument("--candidate",type=Path)
    p.add_argument("--engine",choices=MODELS,required=True)
    p.add_argument("--case",choices=("unavailable","shared-partial","resume"),required=True)
    p.add_argument("--hub-cache",type=Path,default=Path.home()/".cache/huggingface/hub")
    p.add_argument("--output",type=Path,required=True)
    a=p.parse_args()
    report=dict(engine=a.engine,case=a.case,attempts=[],http=[])
    try:
        run(a,report)
        report["passed"]=True
    except Exception as exc:
        report.update(passed=False,error=f"{type(exc).__name__}: {exc}")
    a.output.parent.mkdir(parents=True,exist_ok=True)
    a.output.write_text(json.dumps(report,ensure_ascii=False,indent=2),encoding="utf-8")
    print(json.dumps({key:report.get(key) for key in ("engine","case","passed","error")}))
    return int(not report["passed"])


if __name__=="__main__":
    raise SystemExit(main())
