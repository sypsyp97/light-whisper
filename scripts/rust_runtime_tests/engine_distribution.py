"""Check the build's engine directory or exact distributed engine.tar.xz without extraction."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import tarfile

from engine_protocol import require


def inspect(payload,report):
    files = {}
    manifests = {}
    def record(name,source):
        name=PurePosixPath(name.replace("\\","/")).as_posix().lower()
        leaf=PurePosixPath(name).name
        forbidden=(leaf.endswith((".py",".pyc",".pyo",".pyd",".pyz")) or
                   re.fullmatch(r"python(?:w|\d+)?\.(?:dll|exe|zip)",leaf) or
                   leaf=="base_library.zip" or "__pycache__" in PurePosixPath(name).parts)
        require(not forbidden,f"CPython component in distribution: {name}")
        require(name not in files,f"duplicate distribution entry: {name}")
        if leaf=="runtime-manifest.json":
            data=source.read()
            manifests[name]=json.loads(data)
            size=len(data)
            sha=hashlib.sha256(data).hexdigest()
        else:
            h=hashlib.sha256()
            size=0
            for chunk in iter(lambda:source.read(2*1024*1024),b""):
                if size==0 and leaf.endswith((".exe",".dll")):
                    require(chunk.startswith(b"MZ"),f"invalid PE image {name}")
                h.update(chunk)
                size+=len(chunk)
            sha=h.hexdigest()
        files[name]={"size":size,"sha256":sha}
    if payload.is_dir():
        for path in sorted(payload.rglob("*")):
            if path.is_file():
                with path.open("rb") as source:
                    record(path.relative_to(payload).as_posix(),source)
    else:
        with tarfile.open(payload,"r|xz") as archive:
            for member in archive:
                if member.isfile():
                    with archive.extractfile(member) as source:
                        record(member.name,source)
    required=["engine.exe","onnxruntime.dll","kaldi-native-fbank-core.dll",
              "cublas64_12.dll","cublaslt64_12.dll","cudart64_12.dll",
              "_internal/fireredvad_cmvn.json","_internal/fireredvad_vad.onnx",
              "_internal/_soundfile_data/libsndfile_x64.dll"]
    for backend in ("cpu","cuda"):
        prefix=f"_internal/r2t2-native/{backend}/"
        required.extend(prefix+name for name in ("audiocpp.dll","reuse.dll","runtime-manifest.json"))
    for name in required:
        require(files.get(name,{}).get("size",0)>0,f"missing required packaged file: {name}")
    for backend in ("cpu","cuda"):
        prefix=f"_internal/r2t2-native/{backend}/"
        manifest=manifests[prefix+"runtime-manifest.json"]
        require(manifest.get("backend")==backend,f"wrong native backend in {prefix}")
        for section,directory in (("files",prefix),("shared_files","")):
            for filename,metadata in manifest.get(section,{}).items():
                key=filename.lower()
                actual=files.get(directory+key)
                if backend=="cuda" and key in required and actual is None:
                    actual=files.get(key)
                require(actual=={"size":metadata["size"],"sha256":metadata["sha256"]},
                        f"packaged bytes disagree with {backend} manifest: {filename}")
    report["files"]=files

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--payload",required=True,type=Path)
    p.add_argument("--output",required=True,type=Path)
    a=p.parse_args()
    report={"payload":str(a.payload.resolve(strict=True))}
    try:
        inspect(a.payload,report)
        report["passed"]=True
    except Exception as error:
        report.update(passed=False,error=f"{type(error).__name__}: {error}")
    a.output.parent.mkdir(parents=True,exist_ok=True)
    a.output.write_text(json.dumps(report,indent=2),encoding="utf-8")
    print(json.dumps({k:report.get(k) for k in ("passed","error")}))
    return int(not report["passed"])

if __name__=="__main__":
    raise SystemExit(main())
