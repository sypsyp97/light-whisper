# CapturePipeline formal evidence

Machine-specific paths below are anonymized; `/path/to/...`, `%TEMP%`, and
`%USERPROFILE%` are placeholders for the original local directories.

Baseline: `d29cd45db8876bd7bcfa915ae642af3b0fcc4bbe` (`d29cd45`). This slice
adds only `formal/CapturePipeline.tla`, `formal/CapturePipeline.cfg`, and this
evidence file. No production files were changed by this slice.

Content hashes:

- `formal/CapturePipeline.tla`: `FC1019CB14ED5E27C9E2D01E1C6E0E1C515A5331A7436CAB88A34B7E3D43BF23`
- `formal/CapturePipeline.cfg`: `E6E5226A1D2F5716BFD4B1FD8DDCEE845920499B3A158B9751CBBF3DC23BC25B`

## Requirement covered

`CapturePipeline` is a bounded concurrent state model for the native Windows
capture and CPAL fallback boundary. It covers:

- native startup result, the explicit acceptance handshake, the timeout circuit
  breaker, and a detached worker that returns late;
- one priming transfer, priming-before-live ordering, rejection without writing
  into the fallback buffer, and the sample hard cap;
- CPAL startup success and startup failure after native fallback, including the
  selected-device snapshot;
- microphone-monitor startup, level publication, stop, and the same preferred
  device/default-device resolution rule.

The main safety invariants are `NativeSamplesRequireAcceptedWorker`,
`HandoffIsAcceptedOnce`, `PrimingPrecedesLiveNative`,
`SampleCapNeverExceeded`, `FallbackOwnsBuffer`, `NativeDeviceIsSnapshotted`,
`CPALDeviceIsResolved`, `MonitorDeviceIsResolved`,
`RecordingOwnsMonitor`, `LateWorkersCannotBeAccepted`, and
`TimeoutDetachesNative`.

## Source mapping

The model is mapped to the following implementation seams in the baseline:

| Model behavior | Source evidence |
| --- | --- |
| Hard cap and Windows startup watchdog | `src-tauri/src/services/audio_service/capture.rs:13-24`, `:102-203` |
| Acceptance gate and exactly-once priming transfer | `capture.rs:84-100`, `:121-141`; the existing focused tests are `capture.rs:623-771` |
| CPAL device resolution and stream startup | `capture.rs:208-230`, `:442-587`; the mono callbacks cap writes at `capture.rs:502-535` |
| Native WASAPI priming, live reads, cap, and preferred/default device choice | `src-tauri/src/services/audio_service/windows_capture.rs:150-229`, `:240-265` |
| Recording startup snapshots the selected device and stops the monitor first | `src-tauri/src/commands/audio.rs:89-177`, especially `:94-167` |
| Monitor stream, ready timeout, selected device, level emission, and stop | `src-tauri/src/services/audio_service/monitor.rs:68-176` |
| Downstream native ASR consumes the shared samples after capture startup | `src-tauri/src/services/audio_service/native_recording.rs:113-178`; its polling/control loop is `native_capture.rs:63-162` |

`native_recording.rs` and `native_capture.rs` are mapped as downstream
consumers only. This model ends at capture-buffer ownership and does not claim
to prove the ASR protocol, resampling, captions, network behavior, or result
finalization.

## Trusted environment assumptions and bounds

- `NativeReady`, `NativeUnavailable`, `NativeDisconnected`, and
  `NativeTimedOut` abstract WASAPI/COM/driver scheduling and the configured
  native startup watchdog. TLC does not measure the two-second deadline.
- A native or CPAL worker snapshots the selected name at recording start.
  `"Preferred"` resolves to the exact preferred input when present; `"Missing"`
  and `"None"` resolve to the default input, matching the source fallback
  branches. The model assumes the named/default device can be represented by
  one of those two abstract identities.
- `MaxSessions=2`, `MaxSamples=4`, and `PrimeSamples=2` are finite exploration
  bounds. `MaxSamples` is an abstract cap; it is not the production
  `MAX_RECORD_SAMPLES` value.
- Monitor levels use representative values `{0, 500, 1000}`. Actual audio
  buffers, peak conversion, timing, device-driver errors, and hardware effects
  are outside TLC.
- `CPALFailure` represents device/config/stream/play failure and the outer
  capture-start timeout as one terminal start-error branch. No liveness claim
  is made that any startup or stop eventually completes.
- The model is a safety model, not an implementation refinement proof. Rust
  and Windows hardware tests remain necessary, including the ignored native
  microphone test in `windows_capture.rs:359-383`.

No unresolved test interface was introduced (`INTERFACE_PENDING`: none).

## Positive TLC run

Exact command (WSL2, supplied TLC jar, unique metadir):

```text
wsl.exe bash -lc 'set -o pipefail; java -XX:+UseParallelGC -jar /path/to/temp/light-whisper-tla2tools-1.8.0.jar -noGenerateSpecTE -metadir /tmp/light-whisper-capture-final-nocoverage-restart-$(date +%s%N) -config /path/to/light-whisper/formal/CapturePipeline.cfg /path/to/light-whisper/formal/CapturePipeline.tla'
```

Result:

```text
Model checking completed. No error has been found.
104185 states generated, 14490 distinct states found, 0 states left on queue.
The depth of the complete state graph search is 19.
```

A coverage run with `-coverage 1` also reached every capture and monitor
action, including `NativeReady`, `NativeUnavailable`, `NativeDisconnected`,
`NativeTimedOut`, `NativeAcceptanceFailed`, `AcceptNative`, `StartCPAL`,
`CPALReady`, `CPALFailure`, `NativeSamples`, `CPALSamples`,
`LateNativePrime`, `LateNativeReturn`, `BeginMonitor`, `RestartMonitor`,
`MonitorReady`, and `StopMonitor`, with no invariant error.

Exact coverage command:

```text
wsl.exe bash -lc 'set -o pipefail; java -XX:+UseParallelGC -jar /path/to/temp/light-whisper-tla2tools-1.8.0.jar -noGenerateSpecTE -coverage 1 -metadir /tmp/light-whisper-capture-coverage-20260928-0827-restart -config /path/to/light-whisper/formal/CapturePipeline.cfg /path/to/light-whisper/formal/CapturePipeline.tla'
```

## Negative mutation and exact RED evidence

The mutation was made only in this temporary copy outside the repository:

`%TEMP%\light-whisper-capture-negative-cedb634e8c5e434f9b1a2d4e909f15aa`

`LateNativeReturn` was deliberately changed to append
`AppendSamples(samples, "native", 1)` while the detached worker was returning.
The mutation removes `samples` from that action's `UNCHANGED` tuple. It models
the forbidden late-native write and leaves the repository files unchanged.

Exact command:

```text
wsl.exe bash -lc 'set -o pipefail; java -XX:+UseParallelGC -jar /path/to/temp/light-whisper-tla2tools-1.8.0.jar -noGenerateSpecTE -metadir /tmp/light-whisper-capture-negative-final-$(date +%s%N) -config /path/to/temp/light-whisper-capture-negative-cedb634e8c5e434f9b1a2d4e909f15aa/CapturePipeline.cfg /path/to/temp/light-whisper-capture-negative-cedb634e8c5e434f9b1a2d4e909f15aa/CapturePipeline.tla'
```

The run failed semantically, not during parsing or type checking:

```text
Error: Invariant NativeSamplesRequireAcceptedWorker is violated.
State 2: <BeginRecording ...>
State 3: <NativeTimedOut ...>
  nativeDisabled = TRUE
  lateWorkers = {1}
  acceptedSession = 0
State 4: <LateNativeReturn(1) ...>
  samples = <<"native">>
  acceptedSession = 0
88 states generated, 55 distinct states found, 39 states left on queue.
The depth of the complete state graph search is 4.
```

This is the smallest counterexample observed for the intended late-worker
boundary: timeout closes the acceptance path, then a late native append would
be visible in the current shared buffer without an accepted native session.

## Missing coverage / blockers

There is no blocker for this formal slice. Remaining behavior is intentionally
outside the model: concrete WASAPI/CPAL API failures and timing, audio signal
quality and sample conversion, waveform rendering, native ASR ownership and
resampling, caption events, network/provider behavior, UI layout, and all
liveness/eventual-completion properties. Existing Rust tests and hardware
tests must continue to validate those paths.


## Delivery correspondence update

RecordingOwnsMonitor begins after the synchronous stop-monitor stage, not at
recording-slot reservation. The original model omitted pending monitor startup;
root source review found that gap and added MicrophoneMonitor plus real generation
reservation/invalidation/publication guards. Driver startup may still take time;
stop signals and joins published workers, and a late worker cannot publish a
current monitor. The disposed frontend effect regression also prevents restart
after unmount. RequestProtocols separately checks native stream acknowledgements
and finality; waveform pixels, sample conversion and driver internals remain
implementation-test/environment boundaries. Final model hashes/counts are in
results.json and supersede the initial slice hashes above.
