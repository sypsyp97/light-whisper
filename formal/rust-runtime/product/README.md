# Engine session ownership model

`EngineLifecycle.tla` checks the portion of the JSON-lines server that owns an active stream and its accepted sample count. `Server::stream` in `rust-runtime/crates/engine/src/server.rs` implements Start, Feed and FinishOrCancel. Newer session IDs replace the previous owner. Feed validates the session and raw sample offset before updating the count. Finish and cancel require the active ID. The timeout branch of the server loop checks that no stream is active before suspending. `Server::init` resets the last ID when it successfully resumes. The model's Exit abstracts EOF and the acknowledged exit command through process teardown.

The reference paths are `R2T2StreamSession.start/feed/finish/cancel` in `src-tauri/resources/r2t2_stream.py`, `ASRStreamServerMixin._handle_stream_command` in `asr_stream_server.py` and the idle check in `server_common.py`. Both engines use the same wire-session guards. The real subprocess tests in `scripts/rust_runtime_tests/engine_protocol.py` exercise a stale ID, misaligned offset, superseding start, double finish, cancellation, model suspension and resumption.

The finite model has two increasing IDs, unit-size feeds and at most two samples per session. Guards abstract rejected wire requests as stuttering. Startup failure is terminal for admission in this model. Retry after missing resources, asynchronous preview publication, audio quality and network transfer are outside the checked state space. `OwnedRuntime`, `AlignedAudio` and `NoStaleCompletion` are safety properties. These configurations make no fairness or liveness claim.

Run the command below from this directory using an existing TLA+ tools JAR and an external scratch directory. Positive checking generated 42 states, 17 distinct, on TLA+ tools 1.7.4. Each broken guard must produce its designated counterexample.

```powershell
java -cp $env:TLA2TOOLS_JAR tla2sany.SANY EngineLifecycle.tla
if ($LASTEXITCODE) { throw "SANY failed" }
$expect = @{ none = $null; idle = 'OwnedRuntime'; id = 'AlignedAudio'; offset = 'AlignedAudio'; finish = 'NoStaleCompletion' }
foreach ($mode in 'none','idle','id','offset','finish') {
    $log = Join-Path $env:LW_MODEL_OUTPUT "$mode.log"
    java -XX:+UseParallelGC -Xmx256m -cp $env:TLA2TOOLS_JAR tlc2.TLC -workers 1 -metadir (Join-Path $env:LW_MODEL_OUTPUT $mode) -config "$mode.cfg" EngineLifecycle.tla *> $log
    if ($mode -eq 'none') {
        if ($LASTEXITCODE -ne 0 -or -not (Select-String -Path $log -Pattern '0 states left on queue' -Quiet)) { throw "positive TLC failed" }
    } elseif ($LASTEXITCODE -ne 12 -or -not (Select-String -Path $log -SimpleMatch -Pattern "Invariant $($expect[$mode]) is violated" -Quiet)) {
        throw "missing negative-control witness for $mode"
    }
}
```

Rust server source SHA256 at correspondence review was `cffd39918313349c0c58ad369882d2aacc16c1f3990dd84af46d8cea8cc6e71e`. Reinspect the state writers after touching the stream, start, shutdown or idle paths, then replay the subprocess and TLC checks.

`WakeBackend.tla` models one concrete recovery schedule. A loaded CUDA model becomes idle and the CUDA library becomes unavailable while CPU remains available. Under weak fairness of a submitted wake request, the positive model resumes on CPU. Configure TLC with `wake.cfg` and run against `WakeBackend.tla`. The positive run has four distinct states. `wake-no-fallback.cfg` models retrying the original backend only and violates `AvailableBackendUsed`. `scripts/rust_runtime_tests/wake_backend_fallback.py` replays that schedule in an isolated real process by renaming its own staged CUDA directory after unload and checking a real CPU transcription.
