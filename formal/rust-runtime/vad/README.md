# FireRedVAD frame boundaries

The Python reference is `src-tauri/resources/firered_vad.py` at SHA-256 `e2557c5208b522985b259f85f1170f43a627d52db8e400b7b7581c543b838ecf`. Qwen calls `speech_timestamps` on each full clip. R2T2 calls it on a window of accumulated PCM. Both consume sample indices at 16 kHz.

| Reference decision | Formal state or action | Observable regression |
| --- | --- | --- |
| Skip extraction and inference below 2,640 samples | `Admit`, `inferred`, `EligibleWork` | Fixed 15-frame ONNX model rejects an attempted 14-frame call. |
| Start on 15 consecutive flags and retain the first flag's index | `Search`, `candidate`, `speech` | A 14-frame voice burst is discarded and pre-roll begins twelve hops before the first qualifying flag. |
| Reset the quiet run on voice and finish on the thirtieth consecutive quiet flag | `Continue`, `Pause`, `quiet` | A 29-frame gap bridges and a 30-frame gap splits the controlled ONNX sequence. |
| Retain pending quiet at EOF and clamp padded intervals | `Finish`, `Region`, `ExactRegions` | The final controlled region ends at sample 28,720. |

The five fixed scenarios cover a 2,639-sample short input, the first admissible voiced input, a broken candidate, mixed gaps and silence. The model starts from flags after smoothing. The Rust tests separately exercise the causal smoother, inclusive 0.5 comparison, Kaldi features, model posteriors and PCM conversion. Default spacing between emitted segments is at least 30 hops. A 12-hop pad at either side cannot make two such segments overlap.

Run SANY on `VadBoundaries.tla`, then TLC with `-workers 1 -config VadBoundaries.cfg VadBoundaries.tla`. Positive checks include types, eligible work, exact regions and eventual completion under weak fairness of local CPU steps. Each other configuration enables the fault named by its file. `earlyWork` must violate `EligibleWork`. The five remaining controls must violate `ExactRegions`. These are finite synchronous call scenarios and exclude feature caching, numerical operations inside ONNX and concurrent callers.

The external corpus test uses `LW_QUALITY_ROOT` pointing to the checkout containing the six benchmark WAVs. Run `cargo test -p light-whisper-vad --test parity -- --include-ignored` to include it. The bundled waveform and the controlled ONNX fixture tests run without an external corpus or Python interpreter.
