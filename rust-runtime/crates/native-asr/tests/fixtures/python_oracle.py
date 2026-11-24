"""Generate a Rust parity fixture using the shipped Python wrappers and the same native assets."""
import argparse
import hashlib
import json
from pathlib import Path
import sys
import wave

import numpy as np


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def record(path, fields):
    # NUL-delimited UTF-8 preserves punctuation, whitespace and empty language fields.
    path.write_bytes(b"".join(field.encode("utf-8") + b"\0" for field in fields))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("resources", "library", "qwen", "r2t2", "wav", "output"):
        parser.add_argument("--" + name, required=True, type=Path)
    args = parser.parse_args()
    sys.path.insert(0, str(args.resources))
    from qwen3_native import QwenNativeRuntime
    from r2t2_native import NativeRuntime

    with wave.open(str(args.wav)) as wav:
        assert (wav.getframerate(), wav.getnchannels(), wav.getsampwidth()) == (16000, 1, 2)
        pcm = np.frombuffer(wav.readframes(wav.getnframes()), dtype="<i2").astype("<f4") / np.float32(32768)
    assert pcm.size > 5120 and pcm.size % 5120, "use speech with a partial final packet"
    args.output.mkdir(parents=True, exist_ok=True)
    pcm.tofile(args.output / "input.f32le")
    qwen = QwenNativeRuntime(args.qwen, args.library, backend="cpu", threads=4)
    try:
        result = qwen.run(pcm)
        assert result.text and result.language
        record(args.output / "qwen.expected", (result.text, result.language))
    finally:
        qwen.close()
    r2t2 = NativeRuntime(args.r2t2, args.library, backend="cpu", threads=4, rolling=True)
    packets = []
    try:
        r2t2.start()
        for offset in range(0, len(pcm), 5120):
            text, language = r2t2.feed(pcm[offset:offset + 5120])
            packets.extend((text, r2t2.preview_text, language or ""))
        text, language = r2t2.finish()
        assert text and any(packets)
        record(args.output / "r2t2.expected", [*packets, text, language or ""])
    finally:
        r2t2.close()
    paths = {name: getattr(args, name).resolve() for name in ("library", "wav", "qwen", "r2t2")}
    paths.update({name: args.output / name for name in ("input.f32le", "qwen.expected", "r2t2.expected")})
    paths.update({name: args.resources / name for name in ("qwen3_native.py", "r2t2_native.py")})
    paths["oracle"] = Path(__file__)
    report = {name: {"path": str(path), "sha256": digest(path)} for name, path in paths.items()}
    (args.output / "oracle.json").write_text(json.dumps(report, indent=2), encoding="utf-8")


if __name__ == "__main__":
    main()
