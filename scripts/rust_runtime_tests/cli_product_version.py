"""Source and release builds must report the release script's package.json version."""
import argparse
import importlib.util
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

from engine_protocol import match, require


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--source-root",type=Path,required=True)
    p.add_argument("--target-dir",type=Path,required=True,help="owned external Cargo output")
    p.add_argument("--case",choices=("fingerprint","debug","release"),required=True)
    p.add_argument("--output",type=Path,required=True)
    a=p.parse_args()
    root=a.source_root.resolve()
    report=dict(case=a.case)
    try:
        with tempfile.TemporaryDirectory(prefix="lw-product-version-") as tmp:
            repo=Path(tmp)
            ws=repo/"rust-runtime"
            for name in ("cli","engine","native-asr","reuse","vad"):
                crate=root/"rust-runtime/crates"/name
                dest=ws/"crates"/name
                shutil.copytree(crate,dest,ignore=shutil.ignore_patterns("target",".git","tests"))
            for name in ("Cargo.toml","Cargo.lock"):
                shutil.copy2(root/"rust-runtime"/name,ws/name)
            for name in ("scripts/native_runtime_build.py","scripts/build_cli.py","scripts/build_engine.py",
                         "src-tauri/windows-app-manifest.xml","package.json"):
                dest=repo/name
                dest.parent.mkdir(parents=True,exist_ok=True)
                shutil.copy2(root/name,dest)
            spec=importlib.util.spec_from_file_location("isolated_packager",repo/"scripts/native_runtime_build.py")
            helper=importlib.util.module_from_spec(spec)
            spec.loader.exec_module(helper)
            package=repo/"package.json"
            old=json.loads(package.read_bytes())
            initial={component:helper.source_fingerprint(component) for component in ("cli","engine")}
            version="19.37.41-test.1"
            require(version!=old["version"],"version test needs differing release input")
            pattern=rb'("version"\s*:\s*)'+re.escape(json.dumps(old["version"]).encode())
            new_bytes,changed=re.subn(pattern,lambda m:m[1]+json.dumps(version).encode(),package.read_bytes(),count=1)
            require(changed==1 and json.loads(new_bytes)=={**old,"version":version},"version-only edit")
            package.write_bytes(new_bytes)
            if a.case=="fingerprint":
                new={component:helper.source_fingerprint(component) for component in ("cli","engine")}
                report.update(before=initial,after=new)
                require(initial["cli"]!=new["cli"],"CLI provenance ignored effective version change")
                require(initial["engine"]==new["engine"],"application version alone invalidated reusable engine")
            else:
                os.environ.update(CARGO_NET_OFFLINE="true",CARGO_TARGET_DIR=str(a.target_dir.resolve()),
                                  LIGHT_WHISPER_DATA_DIR=str(repo/"private-data"))
                if a.case=="debug":
                    subprocess.run(["cargo","build","--offline","--locked","-p","light-whisper-cli",
                                    "--manifest-path",str(ws/"Cargo.toml")],cwd=repo,check=True,timeout=240)
                    binary=a.target_dir.resolve()/"debug/light-whisper-cli.exe"
                else:
                    binary=helper.build_binary("cli",repo)
                completed=subprocess.run([str(binary),"doctor"],cwd=repo,capture_output=True,text=True,
                                         encoding="utf-8",check=True,timeout=20)
                doctor=json.loads(completed.stdout)
                report["observed_version"]=doctor.get("version")
                report["expected_version"]=version
                match(doctor,success=True,version=version,cli_runtime="rust")
        report["passed"]=True
    except Exception as exc:
        report.update(passed=False,error=f"{type(exc).__name__}: {exc}")
    a.output.parent.mkdir(parents=True,exist_ok=True)
    a.output.write_text(json.dumps(report,indent=2),encoding="utf-8")
    print(json.dumps(report))
    return int(not report["passed"])


if __name__=="__main__":
    raise SystemExit(main())
