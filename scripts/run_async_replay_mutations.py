"""Check replay sensitivities in isolated local snapshots; never edit live sources."""

import argparse
import hashlib
import json
import shutil
import subprocess
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CASES = {
    "online-asr-identity-initial": {
        "source": "src/hooks/useAsrEngineSettings.ts",
        "changes": [("    if (transitioning.current || !onlineAsrKeyReady) return;",
                     "    if (transitioning.current) return;")],
        "test": "src/hooks/__tests__/useAsrEngineSettingsFormalReplay.test.tsx",
        "pattern": "replays online-asr-identity-initial through the real hook",
        "failure": "to deeply equal []",
    },
    "online-asr-identity-switch": {
        "source": "src/hooks/useAsrEngineSettings.ts",
        "changes": [("    if (transitioning.current || !onlineAsrKeyReady) return;",
                     "    if (transitioning.current) return;")],
        "test": "src/hooks/__tests__/useAsrEngineSettingsFormalReplay.test.tsx",
        "pattern": "replays online-asr-identity-switch through the real hook",
        "failure": "to deeply equal []",
    },
    "online-asr-identity-stale": {
        "source": "src/hooks/useAsrEngineSettings.ts",
        "changes": [(
            "      parts.endpoint && getOnlineAsrEndpoint().then((endpoint) => {\n        if (!current()) return;",
            "      parts.endpoint && getOnlineAsrEndpoint().then((endpoint) => {",
        )],
        "test": "src/hooks/__tests__/useAsrEngineSettingsFormalReplay.test.tsx",
        "pattern": "replays online-asr-identity-stale through the real hook",
        "failure": "international.example.test",
    },
    "online-asr-identity-key-failure": {
        "source": "src/hooks/useAsrEngineSettings.ts",
        "changes": [(
            "        setConfigReadErrors((previous) => ({ ...previous, key: true }));",
            "        setEndpointReady(false);\n        setConfigReadErrors((previous) => ({ ...previous, key: true }));",
        )],
        "test": "src/hooks/__tests__/useAsrEngineSettingsFormalReplay.test.tsx",
        "pattern": "replays online-asr-identity-key-failure through the real hook",
        "failure": "expected false to be true",
    },
    "online-asr-identity-recovery": {
        "source": "src/hooks/useAsrEngineSettings.ts",
        "changes": [("      await readAsrConfig(configReadErrors);",
                     "      setEndpointReady(false);\n      await readAsrConfig(configReadErrors);")],
        "test": "src/hooks/__tests__/useAsrEngineSettingsFormalReplay.test.tsx",
        "pattern": "replays online-asr-identity-recovery through the real hook",
        "failure": "expected false to be true",
    },
    "online-asr-identity-retry-owner": {
        "source": "src/hooks/useAsrEngineSettings.ts",
        "changes": [(
            "    const version = configVersion.current;\n    if (transitioning.current || configRetryVersion.current === version) return;",
            "    const version = ++configVersion.current;\n    if (transitioning.current || configRetryVersion.current === version) return;",
        )],
        "test": "src/hooks/__tests__/useAsrEngineSettingsFormalReplay.test.tsx",
        "pattern": "replays online-asr-identity-retry-owner through the real hook",
        "failure": "to be 'domestic'",
    },
    "online-asr-identity-sibling": {
        "source": "src/hooks/useAsrEngineSettings.ts",
        "changes": [("    return Promise.all([\n      parts.engine",
                     "    return Promise.allSettled([\n      parts.engine")],
        "test": "src/hooks/__tests__/useAsrEngineSettingsFormalReplay.test.tsx",
        "pattern": "replays online-asr-identity-sibling through the real hook",
        "failure": "expected true to be false",
    },
    "engine-status-event": {
        "source": "src/hooks/useModelStatus.ts",
        "changes": [(
            "          statusRequestIdRef.current += 1;\n          const { status, message } = event.payload;",
            "          const { status, message } = event.payload;",
        )],
        "test": "src/hooks/__tests__/useModelStatusFormalReplay.test.tsx",
        "pattern": "replays engine-status-event through the real hook",
        "failure": "to deeply equal",
    },
    "engine-status-query": {
        "source": "src/hooks/useModelStatus.ts",
        "changes": [("    const requestId = ++statusRequestIdRef.current;",
                     "    const requestId = statusRequestIdRef.current;")],
        "test": "src/hooks/__tests__/useModelStatusFormalReplay.test.tsx",
        "pattern": "replays engine-status-query through the real hook",
        "failure": "to deeply equal",
    },
    "engine-status-start": {
        "source": "src/hooks/useModelStatus.ts",
        "changes": [("      } catch (startErr) {\n        if (!currentStart()) return;",
                     "      } catch (startErr) {")],
        "test": "src/hooks/__tests__/useModelStatusFormalReplay.test.tsx",
        "pattern": "replays engine-status-start through the real hook",
        "failure": "to deeply equal",
    },
    "engine-status-progress": {
        "source": "src/hooks/useModelStatus.ts",
        "changes": [(
            '          } else if (status === "loading") {\n            setStage("loading");',
            '          } else if (status === "loading") {\n            startRequestIdRef.current += 1;\n            setStage("loading");',
        )],
        "test": "src/hooks/__tests__/useModelStatusFormalReplay.test.tsx",
        "pattern": "replays engine-status-progress through the real hook",
        "failure": "to match object",
    },
    "engine-status-terminal-query": {
        "source": "src/hooks/useModelStatus.ts",
        "changes": [(
            "  const enterErrorState = useCallback((message: string) => {\n    statusRequestIdRef.current += 1;",
            "  const enterErrorState = useCallback((message: string) => {",
        )],
        "test": "src/hooks/__tests__/useModelStatusFormalReplay.test.tsx",
        "pattern": "replays engine-status-progress through the real hook",
        "failure": "to deeply equal",
    },
    "history-stats-publication": {
        "source": "src/pages/HistoryPage.tsx",
        "changes": [("if (currentRequest === statsRequestId.current) setStats(nextStats);",
                     "setStats(nextStats);")],
        "test": "src/pages/__tests__/HistoryStatsFormalReplay.test.tsx",
        "pattern": "replays history-stats-publication through the actual page",
        "failure": "historyPage.total:202",
    },
    "history-stats-error": {
        "source": "src/pages/HistoryPage.tsx",
        "changes": [(
            '      if (currentRequest === statsRequestId.current) {\n        console.error("Failed to load history stats:", error);\n      }',
            '      console.error("Failed to load history stats:", error);',
        )],
        "test": "src/pages/__tests__/HistoryStatsFormalReplay.test.tsx",
        "pattern": "replays history-stats-error through the actual page",
        "failure": "to deeply equal []",
    },
    "history-stats-cleanup": {
        "source": "src/pages/HistoryPage.tsx",
        "changes": [("      statsRequestId.current += 1;\n", "")],
        "test": "src/pages/__tests__/HistoryStatsFormalReplay.test.tsx",
        "pattern": "replays history-stats-cleanup through the actual page",
        "failure": "to deeply equal []",
    },
    "assistant-saved-read": {
        "source": "src/pages/SettingsPage.tsx",
        "changes": [("&& provider === assistantProviderRef.current && !draft && !assistantKeyDraftsRef.current[provider];",
                     "&& provider === assistantProviderRef.current && !assistantKeyDraftsRef.current[provider];")],
        "test": "src/pages/__tests__/SettingsPageProviderDiscovery.test.tsx",
        "pattern": "replays a read captured while dirty after the matching save clears its draft",
        "failure": "to have value",
    },
    "assistant-read": {
        "source": "src/pages/SettingsPage.tsx",
        "changes": [("if (current()) setAssistantApiKeyState(\"\");", "setAssistantApiKeyState(\"\");")],
        "test": "src/pages/__tests__/SettingsPageProviderDiscovery.test.tsx",
        "pattern": "replays a stale provider read outcome",
        "failure": "to have value",
    },
    "microphone-retired-test": {
        "source": "src/hooks/useMicrophoneSettings.ts",
        "changes": [
            ("    if (monitorOwner !== ownerRef.current) return Promise.resolve();\n", ""),
            ("if (monitorOwner !== ownerRef.current || generation !== monitorGenerationRef.current) return;",
             "if (generation !== monitorGenerationRef.current) return;"),
        ],
        "test": "src/hooks/__tests__/useMicrophoneSettings.test.tsx",
        "pattern": "replays microphone-retired-test through two hook instances",
        "failure": "replacement",
    },
    "microphone-effect-owner": {
        "source": "src/hooks/useMicrophoneSettings.ts",
        "changes": [
            ("  const ownerRef = useRef(Symbol(\"microphone-settings\"));",
             "  const ownerRef = useRef(Symbol(\"microphone-settings\"));\n  const localQueue = useRef(Promise.resolve());"),
            ("const operation = monitorQueue.then(", "const operation = localQueue.current.then("),
            ("    monitorQueue = operation;", "    localQueue.current = operation;"),
        ],
        "test": "src/hooks/__tests__/useMicrophoneSettings.test.tsx",
        "pattern": "replays microphone-effect-owner through two hook instances",
        "failure": "replacement",
    },
    "catalog-global-invalidation": {
        "source": "src/hooks/useModelDiscovery.ts",
        "changes": [(
            "const invalidateModels = useCallback(({ clearPolish, clearAssistant }: ModelInvalidationOptions) => {",
            "const invalidateModels = useCallback(({ clearPolish, clearAssistant }: ModelInvalidationOptions) => {\n    aiModelsRequestIdRef.current += 1;\n    assistantModelsRequestIdRef.current += 1;",
        )],
        "test": "src/hooks/__tests__/useModelDiscoveryFormalReplay.test.tsx",
        "pattern": "replays catalog-global-invalidation through the real hook",
        "failure": "offline",
    },
    "catalog-polish-independent": {
        "source": "src/hooks/useModelDiscovery.ts",
        "changes": [(
            "}, [assistantHasAuth, assistantModelsContext, assistantModelsFetch, assistantUseSeparateModel, sharedAssistantModels]);",
            "}, [aiModels, assistantHasAuth, assistantModelsContext, assistantModelsFetch, assistantUseSeparateModel, sharedAssistantModels]);",
        )],
        "test": "src/hooks/__tests__/useModelDiscoveryFormalReplay.test.tsx",
        "pattern": "replays catalog-polish-independent through the real hook",
        "failure": "offline",
    },
    "history-filter-late-retirement": {
        "source": "src/pages/HistoryPage.tsx",
        "changes": [("      requestId.current += 1;\n", "")],
        "test": "src/pages/__tests__/HistoryFilterFormalReplay.test.tsx",
        "pattern": "replays history-filter-late-retirement through the actual page",
        "failure": "obsolete request failed",
    },
    "history-filter-failed-pagination": {
        "source": "src/pages/HistoryPage.tsx",
        "changes": [("{hasMore && !loading && !loadError && (", "{hasMore && !loading && (")],
        "test": "src/pages/__tests__/HistoryFilterFormalReplay.test.tsx",
        "pattern": "replays history-filter-failed-pagination through the actual page",
        "failure": "historyPage.loadMore",
    },
}


def run(snapshot: Path, case: dict, label: str) -> tuple[int, dict]:
    report = snapshot / f"{label}.json"
    result = subprocess.run([
        "node", str(ROOT / "node_modules/vitest/vitest.mjs"), "run", case["test"],
        "-t", case["pattern"], "--reporter=json", f"--outputFile={report}",
    ], cwd=snapshot, capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=120)
    (snapshot / f"{label}.txt").write_text(result.stdout + result.stderr, encoding="utf-8")
    if not report.exists():
        raise ValueError(f"test collection failed without JSON report: {snapshot / f'{label}.txt'}")
    return result.returncode, json.loads(report.read_text(encoding="utf-8"))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace", type=Path, required=True, help="Explicit local scratch parent")
    parser.add_argument("--case", choices=CASES)
    args = parser.parse_args()
    workspace = args.workspace.resolve()
    workspace.mkdir(parents=True, exist_ok=True)
    records = []
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%f")
    for name in [args.case] if args.case else CASES:
        case = CASES[name]
        snapshot = workspace / f"mutation-{name}-{stamp}"
        if snapshot.resolve().parent != workspace:
            raise ValueError("snapshot escaped explicit scratch parent")
        snapshot.mkdir()
        for folder in ["src", "formal"]:
            shutil.copytree(ROOT / folder, snapshot / folder)
        for filename in ["package.json", "tsconfig.json", "vitest.config.ts"]:
            shutil.copyfile(ROOT / filename, snapshot / filename)
        config = snapshot / "vitest.config.ts"
        config.write_text(config.read_text(encoding="utf-8").replace(
            "  plugins:", '  cacheDir: path.resolve(import.meta.dirname, ".vite-cache"),\n  plugins:'), encoding="utf-8")
        dependencies = snapshot / "node_modules"
        if __import__("os").name == "nt":
            subprocess.run(["cmd", "/c", "mklink", "/J", str(dependencies), str(ROOT / "node_modules")],
                           check=True, capture_output=True, text=True)
        else:
            dependencies.symlink_to(ROOT / "node_modules", target_is_directory=True)
        baseline_code, baseline = run(snapshot, case, "baseline")
        if baseline_code or baseline["numPassedTests"] != 1 or baseline["numFailedTests"]:
            raise ValueError(f"baseline must collect and pass exactly one replay: {snapshot}")
        path = snapshot / case["source"]
        source = path.read_text(encoding="utf-8")
        original_hash = hashlib.sha256(path.read_bytes()).hexdigest()
        for old, new in case["changes"]:
            if source.count(old) != 1:
                raise ValueError(f"mutation anchor drift in {case['source']}: {old}")
            source = source.replace(old, new)
        path.write_text(source, encoding="utf-8")
        wrong_code, wrong = run(snapshot, case, "wrong")
        messages = [message for suite in wrong["testResults"] for test in suite["assertionResults"]
                    if test["status"] == "failed" for message in test["failureMessages"]]
        if wrong_code != 1 or wrong["numFailedTests"] != 1 or not any(case["failure"] in message for message in messages):
            raise ValueError(f"mutation must fail semantically in the selected replay: {snapshot}")
        record = {"case": name, "snapshot": str(snapshot), "source": case["source"],
                  "baseline_sha256": original_hash, "wrong_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                  "baseline_exit": baseline_code, "wrong_exit": wrong_code,
                  "test": case["pattern"], "failed_assertions": wrong["numFailedTests"]}
        records.append(record)
        print(f"PASS sensitivity {name}: baseline 1 GREEN, wrong source 1 semantic RED")
    (workspace / "async-replay-mutations.json").write_text(json.dumps(records, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
