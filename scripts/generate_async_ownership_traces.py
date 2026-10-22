"""Generate replay inputs from real TLC negative counterexamples, never examples.

The negative transition order becomes an input schedule for the corrected
controller. Internal SQL/transport phases map to their owning atomic public call;
drivers must explicitly handle every event and assert the corrected outcome.
"""

import argparse
import hashlib
import json
import re
from pathlib import Path

from check_formal import ROOT, TLC_SHA256, tlc

CASES = {
    "online-asr-identity-initial": ("OnlineAsrIdentity", "OnlineAsrIdentityInitialNegative.cfg", "WritesRequireKnownIdentity"),
    "online-asr-identity-switch": ("OnlineAsrIdentity", "OnlineAsrIdentitySwitchNegative.cfg", "WritesRequireKnownIdentity"),
    "online-asr-identity-stale": ("OnlineAsrIdentity", "OnlineAsrIdentityStaleNegative.cfg", "EndpointPublicationOwned"),
    "online-asr-identity-key-failure": ("OnlineAsrIdentity", "OnlineAsrIdentityKeyFailureNegative.cfg", "KnownEndpointRetained"),
    "online-asr-identity-recovery": ("OnlineAsrIdentity", "OnlineAsrIdentityRecoveryTrace.cfg", "RecoveryNotReached"),
    "online-asr-identity-flush": ("OnlineAsrIdentity", "OnlineAsrIdentityFlushTrace.cfg", "FlushNotReached"),
    "online-asr-identity-retry-owner": ("OnlineAsrIdentity", "OnlineAsrIdentityRetryNegative.cfg", "KnownEndpointRetained"),
    "online-asr-identity-sibling": ("OnlineAsrIdentity", "OnlineAsrIdentitySiblingTrace.cfg", "SiblingNotReached"),
    "engine-status-event": ("EngineStatusOwnership", "EngineStatusOwnershipEventNegative.cfg", "StatusPublicationOwned"),
    "engine-status-query": ("EngineStatusOwnership", "EngineStatusOwnershipQueryNegative.cfg", "StatusPublicationOwned"),
    "engine-status-start": ("EngineStatusOwnership", "EngineStatusOwnershipStartNegative.cfg", "StartOutcomeOwned"),
    "engine-status-progress": ("EngineStatusOwnership", "EngineStatusOwnershipProgressNegative.cfg", "StartOutcomeOwned"),
    "history-stats-publication": ("HistoryStatsOwnership", "HistoryStatsOwnershipPublicationNegative.cfg", "AcceptedSnapshotSurvivesCurrentFailure"),
    "history-stats-error": ("HistoryStatsOwnership", "HistoryStatsOwnershipErrorNegative.cfg", "ErrorOwned"),
    "history-stats-cleanup": ("HistoryStatsOwnership", "HistoryStatsOwnershipCleanupNegative.cfg", "ErrorOwned"),
    "catalog-global-invalidation": ("CatalogDiscoveryOwnership", "CatalogDiscoveryOwnershipGlobalNegative.cfg", "LatestValidCompletionPublishes"),
    "catalog-polish-independent": ("CatalogDiscoveryOwnership", "CatalogDiscoveryOwnershipPolishNegative.cfg", "LatestValidCompletionPublishes"),
    "history-filter-late-retirement": ("HistoryFilterOwnership", "HistoryFilterOwnershipLateNegative.cfg", "ObsoleteCannotPublishOrEndReset"),
    "history-filter-pagination": ("HistoryFilterOwnership", "HistoryFilterOwnershipPaginationNegative.cfg", "PendingResetBlocksPagination"),
    "history-filter-failed-pagination": ("HistoryFilterOwnership", "HistoryFilterOwnershipFailedPaginationNegative.cfg", "PendingResetBlocksPagination"),
    "history-leases": ("HistoryLeaseInterleavings", "HistoryLeaseInterleavingsNegative.cfg", "LeaseCountMatchesHolders"),
    "assistant-key": ("AssistantCredentialOwnership", "AssistantCredentialOwnershipNegative.cfg", "CapturedProviderReceivesKey"),
    "assistant-read": ("AssistantCredentialOwnership", "AssistantCredentialReadNegative.cfg", "OnlyLatestReadPublishes"),
    "assistant-saved-read": ("AssistantCredentialOwnership", "AssistantCredentialSavedReadNegative.cfg", "OnlyLatestReadPublishes"),
    "microphone": ("MicrophoneAsyncOwnership", "MicrophoneAsyncOwnershipNegative.cfg", "SettledMonitorHonorsIntent"),
    "microphone-test-owner": ("MicrophoneAsyncOwnership", "MicrophoneTestOwnerNegative.cfg", "OlderTestCannotResume"),
    "microphone-effect-owner": ("MicrophoneEffectOwners", "MicrophoneEffectOwnersNegative.cfg", "RetiredEffectCannotStopReplacement"),
    "microphone-retired-test": ("MicrophoneEffectOwners", "MicrophoneRetiredTestNegative.cfg", "RetiredEffectCannotStopReplacement"),
    "output-drain": ("OutputDrainOwnership", "OutputDrainOwnershipNegative.cfg", "IdlePendingHasScheduledDrain"),
    "output-success": ("OutputDrainOwnership", "OutputSuccessDrainNegative.cfg", "IdlePendingHasScheduledDrain"),
    "output-start-error": ("OutputDrainOwnership", "OutputStartErrorNegative.cfg", "IdlePendingHasScheduledDrain"),
    "output-target": ("OutputDrainOwnership", "OutputTargetOwnershipNegative.cfg", "ChangedTargetNeverBlindPastes"),
    "ipc-deadline": ("IpcCommandDeadline", "IpcCommandDeadlineNegative.cfg", "OneTotalDeadline"),
    "ipc-write-deadline": ("IpcCommandDeadline", "IpcWriteDeadlineNegative.cfg", "OneTotalDeadline"),
    "ipc-cancel": ("IpcCommandDeadline", "IpcCancellationNegative.cfg", "InvalidFrameCannotBeReused"),
    "model-integrity": ("ModelIntegrityRepair", "ModelIntegrityRepairNegative.cfg", "CorruptAlwaysOffersRepair"),
    "model-repair": ("ModelIntegrityRepair", "ModelIntegrityRepairTrace.cfg", "RepairPathNotReached"),
}


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def counterexample_events(log: str) -> list[str]:
    states = re.findall(r"^State \d+:.*?\n(.*?)(?=^State \d+:|^\d+ states generated|^The depth|^Finished|\Z)", log, re.M | re.S)
    events = []
    for state in states:
        match = re.search(r'^/\\ lastEvent = "([^"\n]+)"', state, re.M)
        if not match:
            raise ValueError("TLC counterexample state has no lastEvent")
        if match[1] != "init":
            events.append(match[1])
    if not events:
        raise ValueError("TLC did not emit a counterexample schedule")
    return events


def verify_trace(path: Path) -> dict:
    trace = json.loads(path.read_text(encoding="utf-8"))
    model, config, invariant = CASES[path.stem]
    if (trace["model"] != f"formal/{model}.tla" or trace["config"] != f"formal/{config}"
            or trace["violated_invariant"] != invariant):
        raise ValueError(f"trace contract identity changed: {path}")
    if trace["generator"] != "TLC 1.7.4 counterexample" or trace["tlc_sha256"] != TLC_SHA256:
        raise ValueError(f"unrecognized trace provenance: {path}")
    for name in ["model", "config"]:
        if digest(ROOT / trace[name]) != trace[f"{name}_sha256"]:
            raise ValueError(f"stale {name} provenance: {path}")
    if not trace["events"] or not all(isinstance(event, str) and event for event in trace["events"]):
        raise ValueError(f"missing trace events: {path}")
    counterexample = ROOT / trace["counterexample"]
    if digest(counterexample) != trace["counterexample_sha256"]:
        raise ValueError(f"corrupt counterexample provenance: {path}")
    log = counterexample.read_text(encoding="utf-8")
    if f"Invariant {invariant} is violated" not in log or counterexample_events(log) != trace["events"]:
        raise ValueError(f"events differ from TLC counterexample: {path}")
    return trace


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tlc-jar", type=Path)
    parser.add_argument("--logs", type=Path, default=ROOT / ".codex/tmp/six-fixes/formal")
    parser.add_argument("--check", action="store_true", help="Reject stale checked-in model/config provenance")
    args = parser.parse_args()
    destination = ROOT / "formal/traces"
    if args.check:
        for case in CASES:
            trace = verify_trace(destination / f"{case}.json")
            print(f"PASS provenance {case}: {len(trace['events'])} TLC events")
        return
    if not args.tlc_jar or digest(args.tlc_jar) != TLC_SHA256:
        parser.error("--tlc-jar must name the pinned TLA+ 1.7.4 jar")
    destination.mkdir(exist_ok=True)
    logs = args.logs.resolve()
    for model in dict.fromkeys(case[0] for case in CASES.values()):
        tlc(args.tlc_jar.resolve(), model, ROOT / "formal" / f"{model}.cfg",
            ROOT / "formal" / f"{model}.tla", logs / model)
    for name, (model, config, invariant) in CASES.items():
        source = ROOT / "formal" / f"{model}.tla"
        cfg = ROOT / "formal" / config
        directory = logs / f"negative-{name}"
        tlc(args.tlc_jar.resolve(), model, cfg, source, directory, invariant)
        log = directory / "tlc.log"
        full_log = log.read_text(encoding="utf-8")
        # Preserve the actual invariant error and complete State blocks. Coverage
        # statistics stay in the run directory; no event is reconstructed by hand.
        start = full_log.index(f"Error: Invariant {invariant} is violated")
        end = full_log.find("The coverage statistics", start)
        counterexample = destination / f"{name}.tlc.txt"
        counterexample.write_text(full_log[start:end if end >= 0 else None], encoding="utf-8", newline="\n")
        trace = {
            "schema": 1, "generator": "TLC 1.7.4 counterexample", "tlc_sha256": TLC_SHA256,
            "model": source.relative_to(ROOT).as_posix(), "config": cfg.relative_to(ROOT).as_posix(),
            "model_sha256": digest(source), "config_sha256": digest(cfg),
            "counterexample": counterexample.relative_to(ROOT).as_posix(),
            "counterexample_sha256": digest(counterexample), "violated_invariant": invariant,
            "events": counterexample_events(full_log),
        }
        (destination / f"{name}.json").write_text(json.dumps(trace, indent=2) + "\n", encoding="utf-8", newline="\n")
        print(f"TRACE {name}: {len(trace['events'])} generated events")


if __name__ == "__main__":
    main()
