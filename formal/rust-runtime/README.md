# Native runtime formal checks

From the repository root, run `python formal/rust-runtime/check_models.py --java <java> --jar <tla2tools-1.7.4.jar> --output <new external directory>`. CI already downloads and verifies this JAR. The runner checks its SHA-256, rejects unregistered models/configurations, checks SANY for each module and runs TLC with one worker. The output contains full checker logs, the expected verdict for each configuration, state counts and hashes of the source files that own the modeled decisions.

| Model | Production decisions | Scope of finite check |
| --- | --- | --- |
| NativeStream | `NativeAsr::start/feed/finish/reset` and the owned FFI event in native-asr | Two unit pushes, failure/release, coalesced and noncoalesced final context |
| NativeCliSession | CLI `session`, `dispatch`, `Client::ready/request/close` and wire validation | Two user records, lazy child ownership, failure and exit. Two explicit witnesses show a reused child and retired failed child. |
| VadBoundaries | VAD `probabilities`, `speech_timestamps` and segmentation after smoothing | Five finite flag sequences. Float inference and real sample boundaries are checked separately by the VAD integration tests. |
| EngineLifecycle | `Server::stream`, `Server::init`, idle suspension and process teardown | Two session IDs, offset admission, replacement and stale completion |
| WakeBackend | Engine idle suspension and `Recognition::resume`/server backend selection | One CUDA loss with CPU available. A fair wake must finish on CPU. |
| CliBootstrap | `standalone::discover`, archive extraction, cache publication and CLI configuration | Two competing clients, selection, privately staged files, failure and cleanup. The dedicated bootstrap runner owns the generated configurations. |

The runner currently checks six positive configurations, 29 fault controls and nine reachability witnesses. Safety controls require TLC exit 12 and the named invariant. Progress controls require exit 13 and the temporal counterexample. Positives require exit zero with no states left on the queue. These results concern the bounded models. Source changes require fresh correspondence review and replay of the relevant native tests.

Detailed property assumptions for VAD, product session ownership and standalone CLI cache operations are in their model directories.
