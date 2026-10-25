"""Compare shipping R2T2 dense-mask Explicit SDPA with isolated window graphs.

Test-only synthetic post-projection Q/K/V, no model execution or runtime option.
Read only the first MiB of --gguf for the embedded config, then link cached native
libraries. Run CPU smoke first; --backend cuda uses an actual CUDA graph/backend.
Optional --include-flash uses the existing Flash lowering with 0/-inf F16 masks;
unsupported Flash graphs are reported. Differences do not fail a quality gate.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile

from build_r2t2_runtime import ROOT, build_environment


TEST = ROOT / "scripts/native_tests/encoder_attention_experiment.cpp"
LIBRARIES = ("engine_runtime.lib", "ggml/src/ggml.lib", "ggml/src/ggml-cpu.lib",
             "ggml/src/ggml-base.lib", "external/sentencepiece/src/sentencepiece.lib",
             "cjson_vendor.lib", "yaml_vendor.lib")
SOURCE_INPUTS = (
    "src/community_models/confucius4_r2t2/audio_encoder.cpp",
    "src/framework/modules/attention/scaled_dot_product_attention.cpp",
    "src/framework/modules/structural_modules.cpp", "src/framework/core/module.cpp",
    "src/framework/core/backend.cpp",
)


def sha256(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def model_geometry(path):
    """Extract embedded config.json via GGUF names/offsets; never read weights."""
    with path.open("rb") as file:
        prefix = file.read(1024 * 1024)
    if prefix[:4] != b"GGUF" or struct.unpack_from("<I", prefix, 4)[0] not in (2, 3):
        raise ValueError("expected GGUF v2/v3")
    position = 24
    formats = {0: "B", 1: "b", 2: "H", 3: "h", 4: "I", 5: "i", 6: "f",
               7: "?", 10: "Q", 11: "q", 12: "d"}

    def unpack(format_):
        nonlocal position
        value = struct.unpack_from("<" + format_, prefix, position)
        position += struct.calcsize("<" + format_)
        return value[0] if len(value) == 1 else value

    def string():
        nonlocal position
        length = unpack("Q")
        if position + length > len(prefix):
            raise ValueError("GGUF config metadata exceeds the first MiB")
        value = prefix[position:position + length].decode("utf-8")
        position += length
        return value

    def value(kind):
        if kind == 8:
            return string()
        if kind == 9:
            item, count = unpack("IQ")
            if count > len(prefix):
                raise ValueError("unexpected large GGUF metadata before embedded config")
            return [value(item) for _ in range(count)]
        return unpack(formats[kind])

    metadata = {}
    for _ in range(struct.unpack_from("<Q", prefix, 16)[0]):
        key, kind = string(), unpack("I")
        if key == "audiocpp.embedded_files.data":
            item, count = unpack("IQ")
            if item != 0:
                raise ValueError("expected embedded-files byte array")
            index = metadata["audiocpp.embedded_files.names"].index("config.json")
            offsets = metadata["audiocpp.embedded_files.offsets"]
            start, end = offsets[index:index + 2]
            if not 0 <= start < end <= count or position + end > len(prefix):
                raise ValueError("embedded config.json is not within the first MiB")
            raw_config = prefix[position + start:position + end]
            config = json.loads(raw_config)
            audio = config.get("thinker_config", config)["audio_config"]
            geometry = {name: audio[name] for name in
                        ("d_model", "encoder_attention_heads", "n_window", "n_window_infer")}
            if any(not isinstance(number, int) or number <= 0 for number in geometry.values()):
                raise ValueError("invalid positive encoder geometry")
            if geometry["d_model"] % geometry["encoder_attention_heads"]:
                raise ValueError("d_model is not divisible by encoder_attention_heads")
            return geometry, {"path": str(path), "file_bytes": path.stat().st_size,
                              "read_bytes": len(prefix), "prefix_sha256": hashlib.sha256(prefix).hexdigest(),
                              "embedded_config_sha256": hashlib.sha256(raw_config).hexdigest(),
                              "embedded_config_file_offset": position + start,
                              "embedded_config_bytes": end - start}
        metadata[key] = value(kind)
    raise ValueError("GGUF lacks the expected embedded config.json")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--backend", choices=("cpu", "cuda"), default="cpu")
    parser.add_argument("--gguf", required=True, type=Path, help="Read-only config provenance; no model execution")
    parser.add_argument("--source", type=Path, default=ROOT / "build/r2t2/shipping-source")
    parser.add_argument("--libraries", type=Path, help="Cached static-library build; defaults to build/r2t2/{backend}")
    parser.add_argument("--build-dir", type=Path, help="Defaults to tmp/asr-opt-20261025/attention/{backend}")
    parser.add_argument("--threads", type=int, choices=(1, 2), default=2)
    parser.add_argument("--warmup", type=int, default=3)
    parser.add_argument("--rounds", type=int, default=9)
    parser.add_argument("--seed", type=int, default=20261025)
    parser.add_argument("--include-flash", action="store_true")
    parser.add_argument("--compile-only", action="store_true")
    parser.add_argument("--no-build", action="store_true")
    args = parser.parse_args()
    if args.warmup <= 0 or args.rounds <= 0 or not 0 <= args.seed <= 2**32 - 32:
        parser.error("positive warmup/rounds and seed in 0..2**32-33 required")
    source, libraries, build, gguf = (path.resolve() for path in (
        args.source, args.libraries or ROOT / f"build/r2t2/{args.backend}",
        args.build_dir or ROOT / f"tmp/asr-opt-20261025/attention/{args.backend}", args.gguf))
    geometry, model_provenance = model_geometry(gguf)
    build.mkdir(parents=True, exist_ok=True)
    env, _ = build_environment()
    env = {key.upper(): value for key, value in env.items()}
    lib_paths = [libraries / name for name in LIBRARIES]
    if args.backend == "cuda":
        cache = {}
        for line in (libraries / "CMakeCache.txt").read_text(encoding="utf-8").splitlines():
            if "=" in line and ":" in line.split("=", 1)[0] and not line.startswith(("#", "//")):
                key, value = line.split("=", 1)
                cache[key.split(":", 1)[0]] = value
        cuda = Path(cache["CUDAToolkit_BIN_DIR"]).parent
        lib_paths += [libraries / "ggml/src/ggml-cuda/ggml-cuda.lib"]
        lib_paths += [cuda / f"lib/x64/{name}.lib" for name in
                      ("cublas", "cublasLt", "cuda", "cudart", "cufft", "cudadevrt", "cudart_static")]
        env["PATH"] = str(cuda / "bin") + os.pathsep + env["PATH"]
    compiler = shutil.which("cl.exe", path=env["PATH"])
    linker = shutil.which("link.exe", path=env["PATH"])
    if not compiler or not linker:
        parser.error("Visual Studio cl.exe/link.exe unavailable after vcvars")
    paths = [TEST, Path(__file__).resolve(), ROOT / "scripts/build_r2t2_runtime.py",
             libraries / "CMakeCache.txt", *lib_paths, *[source / name for name in SOURCE_INPUTS],
             *sorted((source / "include").rglob("*.h")),
             *sorted((source / "external/ggml/include").glob("*.h"))]
    for path in paths:
        if not path.is_file():
            parser.error(f"required experiment input missing: {path}")
    fingerprint = {str(path): sha256(path) for path in paths}
    commands = []

    def execute(command, log_name):
        command = list(map(str, command))
        commands.append(command)
        # Sequential single-TU compile and at most two backend threads; low priority.
        result = subprocess.run(command, cwd=build, env=env, capture_output=True,
                                text=True, errors="replace", creationflags=subprocess.BELOW_NORMAL_PRIORITY_CLASS)
        with (build / log_name).open("a", encoding="utf-8") as log:
            log.write(subprocess.list2cmdline(command) + "\n" + result.stdout + result.stderr)
        return result

    executable = build / "encoder_attention_experiment.exe"
    manifest = build / "compiled-source.json"
    if not args.no_build:
        (build / "build.log").write_text("", encoding="utf-8")
        compile_command = [compiler, "/nologo", "/c", "/std:c++17", "/EHsc", "/O2", "/Gy", "/MD",
                           "/DNDEBUG", "/DNOMINMAX", "/D_CRT_SECURE_NO_WARNINGS",
                           f"/I{source / 'include'}", f"/I{source / 'external/ggml/include'}",
                           f"/Fo{build / 'experiment.obj'}", TEST]
        link_command = [linker, "/nologo", "/OPT:REF", "/INCREMENTAL:NO", f"/OUT:{executable}",
                        build / "experiment.obj", *lib_paths, "kernel32.lib", "advapi32.lib"]
        for command in (compile_command, link_command):
            result = execute(command, "build.log")
            if result.returncode:
                print(result.stdout + result.stderr, end="")
                raise RuntimeError(f"native experiment compilation/link failed; see {build / 'build.log'}")
        manifest.write_text(json.dumps({"inputs": fingerprint, "executable_sha256": sha256(executable),
                                        "commands": commands}, indent=2) + "\n", encoding="utf-8")
    else:
        previous = json.loads(manifest.read_text(encoding="utf-8"))
        if previous["inputs"] != fingerprint or previous["executable_sha256"] != sha256(executable):
            parser.error("compiled experiment inputs changed; rerun without --no-build")
    evidence = {
        "synthetic_post_projection_qkv_only": True, "model_weights_executed": False,
        "backend": args.backend, "geometry": geometry, "model_config_provenance": model_provenance,
        "inputs": fingerprint, "executable_sha256": sha256(executable), "commands": commands,
        "warmup": args.warmup, "rounds": args.rounds, "threads": args.threads, "seed": args.seed,
        "timing": "Whole native graph compute plus backend synchronize; excludes construction, upload, download, artifact writes. QKV/masks restored and synchronized before every warmup/measured compute because gallocr may reuse leaf storage after its last consumer. Rotating measured variant order.",
        "memory": "Actual ggml gallocr buffer reservation, including graph inputs/masks/output. Mask bytes also reported separately. Excludes backend temporary pools, library workspace, driver allocations and host metadata arena.",
        "numeric_metrics": "Exact float32 bytes; max abs; max relative = abs(a-b)/max(abs(a),abs(b)) with 0/0=0; L2 relative uses reference L2, null only for nonzero error against zero reference norm.",
        "raw_layout": "q/k/v/output little-endian float32 [1, capacity_tokens, heads, head_dim]; masks concatenate square tensors in mask_square_lengths order, with reported dtype.",
        "fixture": "Window = max chunk token count * integer(n_window_infer/(2*n_window)); exact source inline token-count function. One/two windows, tails at W+1/2W+1, then source four-chunk bucket with repeated shrink/grow and zero/nonzero padded QKV. Padded rows attend only themselves.",
        "limitations": "Standalone attention arithmetic and graph timing only. No projections, full encoder, model inference, recognition-quality claim or production speedup claim; no numerical tolerance gate.",
        "compile_only": args.compile_only,
    }
    returncode = 0
    if not args.compile_only:
        raw = Path(tempfile.mkdtemp(prefix="raw-", dir=build))
        evidence["raw_directory"] = str(raw.relative_to(build))
        command = [executable, "--backend", args.backend, "--output-dir", raw,
                   "--heads", geometry["encoder_attention_heads"], "--head-dim",
                   geometry["d_model"] // geometry["encoder_attention_heads"],
                   "--n-window", geometry["n_window"], "--n-window-infer", geometry["n_window_infer"],
                   "--warmup", args.warmup, "--rounds", args.rounds, "--threads", args.threads, "--seed", args.seed]
        if args.include_flash:
            command.append("--include-flash")
        (build / "native.log").write_text("", encoding="utf-8")
        result = execute(command, "native.log")
        print(result.stdout, end="")
        print(result.stderr, end="")
        returncode = result.returncode
        report = raw / "results.jsonl"
        evidence["results"] = [json.loads(line) for line in report.read_text(encoding="utf-8").splitlines()] if report.exists() else []
        evidence["artifacts"] = {str(path.relative_to(build)): {"bytes": path.stat().st_size, "sha256": sha256(path)}
                                 for path in sorted(raw.iterdir()) if path.is_file()}
    evidence["returncode"] = returncode
    encoded = json.dumps(evidence, indent=2) + "\n"
    (build / "experiment.json").write_text(encoded, encoding="utf-8")
    if not args.compile_only:
        (raw / "experiment.json").write_text(encoded, encoding="utf-8")
    print(f"Evidence: {build / 'experiment.json'}")
    return returncode


if __name__ == "__main__":
    raise SystemExit(main())
