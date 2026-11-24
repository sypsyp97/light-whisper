"""Generate an isolated exact-sample oracle with the reference realtime RE-USE backend."""
import argparse
import hashlib
import json
from pathlib import Path
import sys
import wave

import numpy as np


def digest(file):
    with file.open("rb") as src:
        return hashlib.file_digest(src,"sha256").hexdigest()


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--source-root",type=Path,required=True)
    p.add_argument("--model",type=Path,required=True)
    p.add_argument("--output",type=Path,required=True)
    a=p.parse_args()
    root=a.source_root.resolve()
    resources=root/"src-tauri/resources"
    sys.path.insert(0,str(resources))
    from reuse_enhancement import RealtimeReuseEnhancer

    fixture="ascend-test-00081"
    row=next(r for r in json.loads((root/"tmp/asr-opt-20261025/natural-quality/fixtures.json").read_text(
        encoding="utf-8"))["fixtures"] if r["id"]==fixture)
    audio=root/row["path"]
    if digest(audio)!=row["wav"]["file_sha256"]:
        raise ValueError("reference fixture SHA mismatch")
    with wave.open(str(audio)) as wav:
        pcm=np.frombuffer(wav.readframes(wav.getnframes()),"<i2").astype(np.float32)/32768
    a.output.mkdir(parents=True,exist_ok=True)
    backend=RealtimeReuseEnhancer(backend="cpu",threads=2,model_path=a.model)
    trace=[]
    identities={}
    def step(action,samples=None):
        number=len(trace)
        source="-"
        target="-"
        if samples is not None:
            source=f"{number}-input.f32"
            samples.astype("<f4").tofile(a.output/source)
        result=getattr(backend,action)(samples) if samples is not None else getattr(backend,action)()
        if result is not None:
            if not np.all(np.isfinite(result)):
                raise AssertionError("nonfinite reference output")
            target=f"{number}-output.f32"
            result.astype("<f4").tofile(a.output/target)
        trace.append(f"{action} {source} {target}")
        return result
    try:
        enhanced=step("enhance",pcm)
        if len(enhanced)!=len(pcm) or np.max(np.abs(enhanced-pcm))<0.01 or np.max(np.abs(enhanced))<0.01:
            raise AssertionError("reference must distinguish actual enhancement from identity/silence")
        chunks=[]
        start=0
        for size in [0,1,318,1,319,1,1,639,641,1919,640,320,320]:
            chunks.append(step("feed",pcm[start:start+size]))
            start+=size
        chunks.append(step("feed",pcm[start:]))
        chunks.append(step("flush"))
        np.testing.assert_array_equal(np.concatenate(chunks),enhanced)
        step("flush")
        step("feed",pcm[:1600])
        step("reset")
        np.testing.assert_array_equal(step("enhance",pcm),enhanced)
        for size in (0,1,319,320,639,640,641):
            step("enhance",pcm[5000:5000+size])
        step("enhance",np.zeros(1600,np.float32))
    finally:
        backend.close()
    (a.output/"trace.txt").write_text("\n".join(trace)+"\n",encoding="ascii")
    for path in [audio,resources/"reuse_enhancement.py",resources/"r2t2-native/cpu/reuse.dll",a.model]:
        identities[str(path)]=digest(path)
    report=dict(fixture=fixture,samples=len(pcm),steps=len(trace),source_file_hashes=identities,
                trace_sha256=digest(a.output/"trace.txt"),
                vector_hashes={p.name:digest(p) for p in sorted(a.output.glob("*.f32"))})
    (a.output/"identity.json").write_text(json.dumps(report,indent=2),encoding="utf-8")
    print(f"{len(trace)} steps; {len(pcm)} natural input samples; feed+flush and reset exact")
if __name__=="__main__":
    main()
