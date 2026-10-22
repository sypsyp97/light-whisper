"""Real Qwen product-wrapper compatibility check; one DLL per fresh process.

Capture: --model MODEL --library BASELINE_DLL --dependencies DLL_DIR
         --zh-wav PUBLIC_ZH --en-wav PUBLIC_EN --output baseline.json
Check:   same inputs, CANDIDATE_DLL, --reference baseline.json --output candidate.json
Collect: --zh-wav PUBLIC_ZH --en-wav PUBLIC_EN --list-cases (no DLL/model load).
This checks exact observed text/language/sample traces, not semantic ASR quality
or performance. Twelve real decodes share one resident native session.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import sys

import numpy as np


sys.dont_write_bytecode = True
ROOT = next(path for path in Path(__file__).resolve().parents
            if (path / "src-tauri/resources/qwen3_native.py").is_file())
RESOURCES = ROOT / "src-tauri/resources"
sys.path.insert(0, str(ROOT / "scripts"))
# Reuse the established PCM16 conversion/validation and file hashing helper.
from test_r2t2_native_cache import read_wav, require, sha256


SCHEMA = "qwen-product-wrapper-regression-v1"
CONFIG = {"backend": "cuda", "threads": 1, "sample_rate": 16000,
          "preview_samples": 3200, "window_samples": 192000,
          "session_policy": "one resident QwenNativeRuntime", "decode_count": 12}
CONTEXT = "中文语音识别。"


def pcm_info(pcm):
    return {"samples": int(pcm.size), "dtype": str(pcm.dtype),
            "sha256": hashlib.sha256(pcm.tobytes()).hexdigest()}


def inputs(zh_wav, en_wav):
    clips = {"zh-full": read_wav(zh_wav), "en-full": read_wav(en_wav)}
    for name, pcm in clips.items():
        require(pcm.size <= CONFIG["window_samples"], f"{name}: fixture must fit the 12-second preview window")
        require(np.isfinite(pcm).all() and np.any(pcm), f"{name}: fixture must contain finite public speech samples")
    zh = clips["zh-full"]
    clips.update({"zh-short": zh[:32000], "zh-startup": zh[:1920],
                  "zh-startup-padded": np.pad(zh[:1920], (0, 8000 - 1920)),
                  "zh-growth": zh[32000:-1], "zh-last-sample": zh[-1:],
                  "zh-before-tail": zh[:-1]})
    fixtures = {language: {"path": str(path.resolve()), "sha256": sha256(path),
                           "pcm": pcm_info(clips[f"{language}-full"])}
                for language, path in (("zh", zh_wav), ("en", en_wav))}

    def action(name, op, pcm=None, decode=None, context="", language=None):
        return {"name": name, "op": op, "pcm": pcm, "decode": decode,
                "context": context, "language": language}

    actions = [
        action("zh-short", "run", "zh-short", "zh-short"),
        action("zh-full", "run", "zh-full", "zh-full"),
        action("en-full", "run", "en-full", "en-full"),
        action("zh-forced-context", "run", "zh-full", "zh-full", CONTEXT, "Chinese"),
        action("en-after-clear", "run", "en-full", "en-full"),
        action("zh-resident-repeat", "run", "zh-full", "zh-full"),
        action("reset-prime-start", "start", context=CONTEXT, language="Chinese"),
        action("reset-prime-preview", "feed", "zh-startup", "zh-startup-padded", CONTEXT, "Chinese"),
        action("explicit-reset", "reset"),
        action("after-reset-start", "start"),
        action("en-complete-preview", "feed", "en-full", "en-full"),
        action("en-reused-final", "finish"),
        action("restart-prime-start", "start", context=CONTEXT, language="Chinese"),
        action("restart-prime-preview", "feed", "zh-startup", "zh-startup-padded", CONTEXT, "Chinese"),
        action("restart-clears-active", "start"),
        action("zh-prefix-preview", "feed", "zh-short", "zh-short"),
        action("zh-grown-preview", "feed", "zh-growth", "zh-before-tail"),
        action("zh-one-sample-tail", "feed", "zh-last-sample"),
        action("zh-complete-final", "finish", decode="zh-full"),
    ]
    plan = [{**item, "input_pcm": pcm_info(clips[item["pcm"]]) if item["pcm"] else None,
             "decode_pcm": pcm_info(clips[item["decode"]]) if item["decode"] else None}
            for item in actions]
    require(sum(item["decode"] is not None for item in plan) == CONFIG["decode_count"], "decode budget changed")
    return clips, fixtures, plan


def dependency_info(library, directories):
    manifest, locations = {}, []
    for directory in dict.fromkeys([library.parent.resolve(), *(path.resolve() for path in directories)]):
        require(directory.is_dir(), f"dependency directory missing: {directory}")
        locations.append(str(directory))
        for path in sorted(directory.glob("*.dll")):
            if path.name.casefold() == library.name.casefold():
                continue
            name, digest = path.name.casefold(), sha256(path)
            require(name not in manifest or manifest[name] == digest, f"ambiguous dependency DLL: {name}")
            manifest[name] = digest
    return {"directories": locations, "dll_sha256": manifest}


def check_reference(reference, report):
    require(reference["status"] == "pass", "reference did not pass")
    for key in ("schema", "config", "plan"):
        require(reference[key] == report[key], f"reference {key} input changed")
    for key in ("model_sha256", "qwen_wrapper_sha256", "native_wrapper_sha256",
                "fixture_helper_sha256", "script_sha256"):
        require(reference["provenance"][key] == report["provenance"][key], f"reference {key} input changed")
    for language in ("zh", "en"):
        for key in ("sha256", "pcm"):
            require(reference["fixtures"][language][key] == report["fixtures"][language][key],
                    f"reference {language} {key} input changed")
    require(reference["provenance"]["dependencies"]["dll_sha256"] ==
            report["provenance"]["dependencies"]["dll_sha256"], "reference dependency DLLs changed")
    # A modified/stale trace must not masquerade as the declared input recipe.
    expected = [{"action": item["name"], "pcm": item["decode_pcm"],
                 "context": item["context"], "language": item["language"]}
                for item in report["plan"] if item["decode"]]
    require([item["input"] for item in reference["decodes"]] == expected, "reference decode trace input mismatch")
    require([item["input"] for item in reference["events"]] == report["plan"], "reference event trace input mismatch")
    require(all(item["status"] == "pass" for item in reference["decodes"] + reference["events"]),
            "reference contains an unfinished or failed trace")
    require(reference["native_closed"] is True, "reference native cleanup incomplete")


def execute(native, clips, report, save):
    sys.path.insert(0, str(RESOURCES))
    from qwen3_native import QwenPreviewRuntime

    session_handle = native.session.value
    active_action = None

    def decode(pcm, context="", language=None):
        item = active_action
        require(item["decode"] is not None, f"{item['name']}: unexpected decode")
        observed = {"action": item["name"], "pcm": pcm_info(pcm), "context": context, "language": language}
        expected = {"action": item["name"], "pcm": item["decode_pcm"],
                    "context": item["context"], "language": item["language"]}
        require(observed == expected, f"{item['name']}: decode PCM/context/language changed")
        require(native.session.value == session_handle and session_handle, "resident session changed")
        record = {"input": observed, "status": "running"}
        report["decodes"].append(record)
        report["stage"] = {"operation": "native.run", "action": item["name"]}
        save()  # Native aborts leave a running operation, never a false PASS.
        try:
            result = native.run(pcm, context=context, language=language, timestamps="none")
            record.update(status="pass", output={"text": result.text, "language": result.language})
            return result
        except BaseException as error:
            record.update(status="failed", error=f"{type(error).__name__}: {error}")
            raise
        finally:
            save()

    preview = QwenPreviewRuntime(decode, sample_rate=CONFIG["sample_rate"],
                                 preview_samples=CONFIG["preview_samples"], window_samples=CONFIG["window_samples"])
    try:
        for active_action in report["plan"]:
            item = active_action
            event = {"input": item, "status": "running"}
            report["events"].append(event)
            report["stage"] = {"operation": item["op"], "action": item["name"]}
            before = len(report["decodes"])
            save()
            try:
                if item["op"] == "run":
                    result = decode(clips[item["pcm"]], context=item["context"], language=item["language"])
                    event["output"] = {"text": result.text, "language": result.language}
                elif item["op"] == "start":
                    preview.start(context=item["context"], language=item["language"])
                elif item["op"] == "reset":
                    preview.reset()
                else:
                    text, language = (preview.feed(clips[item["pcm"]]) if item["op"] == "feed" else preview.finish())
                    event["output"] = {"text": text, "language": language}
                event["decode_count"] = len(report["decodes"]) - before
                event["preview_text"] = preview.preview_text
                require(event["decode_count"] == int(item["decode"] is not None), f"{item['name']}: decode/reuse count changed")
                if item["op"] in ("start", "reset", "finish"):
                    require(preview.preview_text == "", f"{item['name']}: stale preview")
                event["status"] = "pass"
            except BaseException as error:
                event.update(status="failed", error=f"{type(error).__name__}: {error}")
                raise
            finally:
                save()

        require(len(report["decodes"]) == CONFIG["decode_count"], "decode budget changed")
        outputs = {item["input"]["name"]: item.get("output") for item in report["events"]}
        for name in ("zh-full", "en-full"):
            require(bool(outputs[name]["text"].strip()), f"{name}: complete speech returned no text")
        for first, repeated in (("zh-full", "zh-resident-repeat"), ("en-full", "en-after-clear"),
                                ("en-full", "en-reused-final"), ("zh-full", "zh-complete-final")):
            require(outputs[first] == outputs[repeated], f"{first}/{repeated}: resident/reset/reuse output changed")
    finally:
        preview.reset()


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    for name in ("model", "library", "zh-wav", "en-wav", "output", "reference"):
        parser.add_argument("--" + name, type=Path)
    parser.add_argument("--dependencies", type=Path, action="append", default=[])
    parser.add_argument("--list-cases", action="store_true")
    args = parser.parse_args()
    required = ("zh_wav", "en_wav") if args.list_cases else ("model", "library", "dependencies", "zh_wav", "en_wav", "output")
    for name in required:
        if not getattr(args, name):
            parser.error(f"--{name.replace('_', '-')} is required")
    if args.list_cases:
        _, fixtures, plan = inputs(args.zh_wav, args.en_wav)
        print(json.dumps({"config": CONFIG, "fixtures": fixtures, "plan": plan}, ensure_ascii=False, indent=2))
        return 0
    if args.output.exists():
        parser.error("--output exists; choose a fresh path")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    # Exclusive creation prevents accidental replacement of a frozen report.
    with args.output.open("x", encoding="utf-8") as output:
        report = {"schema": SCHEMA, "mode": "check" if args.reference else "capture",
                  "created_utc": datetime.now(timezone.utc).isoformat(), "pid": os.getpid(),
                  "status": "running", "stage": {"operation": "inputs"}, "config": CONFIG,
                  "decodes": [], "events": [], "native_closed": None}

        def save():
            output.seek(0)
            output.write(json.dumps(report, ensure_ascii=False, allow_nan=False, indent=2) + "\n")
            output.truncate()
            output.flush()
            os.fsync(output.fileno())

        native = None
        save()
        try:
            clips, report["fixtures"], report["plan"] = inputs(args.zh_wav, args.en_wav)
            report["provenance"] = {
                "model": str(args.model.resolve()), "model_sha256": sha256(args.model),
                "library": str(args.library.resolve()), "library_sha256": sha256(args.library),
                "dependencies": dependency_info(args.library, args.dependencies),
                "qwen_wrapper_sha256": sha256(RESOURCES / "qwen3_native.py"),
                "native_wrapper_sha256": sha256(RESOURCES / "r2t2_native.py"),
                "fixture_helper_sha256": sha256(ROOT / "scripts/test_r2t2_native_cache.py"),
                "script_sha256": sha256(Path(__file__)),
            }
            reference = None
            if args.reference:
                data = args.reference.read_bytes()
                report["reference"] = {"path": str(args.reference.resolve()), "sha256": hashlib.sha256(data).hexdigest()}
                reference = json.loads(data)
                check_reference(reference, report)
            sys.path.insert(0, str(RESOURCES))
            from qwen3_native import QwenNativeRuntime
            report["stage"] = {"operation": "native.load"}
            save()
            native = QwenNativeRuntime(args.model, args.library, backend=CONFIG["backend"],
                                       dll_directories=args.dependencies, threads=CONFIG["threads"])
            report["native_closed"] = False
            report["abi_version"] = native.functions["abi_version"]()
            execute(native, clips, report, save)
            report["stage"] = {"operation": "compare"}
            if reference is not None:
                for key in ("abi_version", "decodes", "events"):
                    require(reference[key] == report[key], f"baseline/candidate {key} mismatch; inspect both reports")
            report["status"] = "pass"
        except BaseException as error:
            report.update(status="failed", error=f"{type(error).__name__}: {error}")
        finally:
            if native is not None:
                try:
                    native.close()
                    require(not (native.session or native.model or native.registry or native._dll_paths), "native handles remain after close")
                    report["native_closed"] = True
                except BaseException as error:
                    report.update(status="failed", cleanup_error=f"{type(error).__name__}: {error}")
            if report["status"] == "pass":
                report["stage"] = {"operation": "complete"}
            save()
    print(f"{report['status'].upper()} mode={report['mode']} decodes={len(report['decodes'])} output={args.output}", flush=True)
    if report["status"] != "pass":
        print(report.get("error", report.get("cleanup_error")), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    raise SystemExit(main())
