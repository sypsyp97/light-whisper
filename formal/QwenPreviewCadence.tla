---------------------- MODULE QwenPreviewCadence ----------------------
\* Adaptive preview cadence for the Qwen3-ASR runtime.
\*
\* A capture below LongSamples (4 s) previews every BaseThreshold (200 ms); a
\* longer capture previews every LongThreshold (320 ms); the very first preview
\* uses StartupThreshold (120 ms).  The threshold for the next preview is chosen
\* at the end of every _accept_pcm from the sample count observed there, and the
\* decision must observe that capture, not the one before it. A retired job
\* cannot publish, and finish() may reuse a preview only when it
\* covers the whole capture inside the preview window.
\*
\* Production anchors:
\*   src-tauri/resources/qwen3_native.py#def _accept_pcm(
\*     appends the samples and reads the threshold for the next preview.
\*   src-tauri/resources/qwen3_native.py#def feed(
\*     submits a preview job once the accumulated new samples reach it.
\*   src-tauri/resources/qwen3_native.py#def finish(
\*     reuses _preview_text only for a complete capture inside _window_samples,
\*     otherwise decodes every chunk; finally reset()s.
\*   src-tauri/resources/qwen3_native.py#def reset(
\*   src-tauri/resources/qwen3_native.py#def _submit(
\*     collects finished previews, discards the not yet started raw job and
\*     appends the new job to _jobs, then clears _new_samples.
\*   src-tauri/resources/qwen3_native.py#def _collect_previews(
\*     publishes completed jobs in submission order and never a cancelled one.
\*   src-tauri/resources/qwen3_native.py#def _prepare_loop(
\*   src-tauri/resources/qwen3_native.py#def _worker_loop(
\*
\* Covered here: the QwenPreviewRuntime cadence rule (threshold decision and the
\* capture it observes), one pending or running job, nonempty previews, reuse of
\* the last published preview by finish(), and reset().
\* Deliberately NOT modelled: QwenPipelineRuntime.finish()'s second reuse source,
\* an in-flight job whose count already equals _sample_count, and the backlog
\* suppression flag (preview=False). For exact in-flight reuse, see
\* test_qwen_pipeline.py#test_feed_acknowledges_during_native_and_only_latest_pending_runs.
\* The pipeline cadence tests in test_qwen_preview_cadence.py use suppression
\* and exercise finish() after publication.
\*
\* Abstract thresholds 1/2/3 and cutoff 4 preserve the ordering of production
\* counts 1920/3200/5120 and 64000. There is no fixed samples-per-unit ratio.
\*
\* Wrong selects a deliberately weakened branch for the negative controls:
\*   "none"                     the contract as implemented
\*   "publish-stale"            a replaced/cancelled job still publishes
\*   "finish-ignores-coverage"  finish() reuses a preview that misses the tail
\*   "finish-ignores-window"    finish() reuses a preview beyond the window
\*   "fixed-threshold"          the threshold never adapts to the sample count
\*   "gate-lagged"              the threshold decision observes the previous block
\*
\* Fairness: no WF_/SF_ conjunct is declared.  This model states safety
\* obligations only; a preview that never completes, or a stream that never
\* finishes, is a test failure rather than a specification violation, so no
\* progress obligation is invented here.
EXTENDS Naturals

CONSTANTS Wrong, StartupThreshold, BaseThreshold, LongThreshold, LongSamples,
          WindowSamples, MaxSamples
VARIABLE s
vars == <<s>>

Step == 2
JobStates == {"none", "queued", "prepared", "running", "done"}
NoJob == [upto |-> 0, st |-> "none", replaces |-> 0]

\* QwenPreviewRuntime._accept_pcm: the threshold that governs the next preview is
\* a function of the sample count after this capture and of whether a preview was
\* already published when that decision was taken.
ThresholdFor(captured, hasPreview) ==
  IF ~hasPreview THEN StartupThreshold
  ELSE IF captured < LongSamples THEN BaseThreshold ELSE LongThreshold

\* The last threshold decision: the sample count it observed, whether a preview
\* existed then, and the value it produced for the next preview.
NoGate == [at |-> 0, seen |-> FALSE, value |-> StartupThreshold]

Init ==
  s = [stage |-> "idle", captured |-> 0, since |-> 0, hasPreview |-> FALSE,
       gate |-> NoGate, job |-> NoJob, retired |-> 0,
       stalePublishes |-> 0, lastPub |-> 0,
       finishUsesPreview |-> FALSE, finishSamples |-> 0, finishCovered |-> 0]

\* QwenPreviewRuntime.start / reset: a fresh stream starts in the startup state.
Start ==
  /\ s.stage = "idle"
  /\ s' = [s EXCEPT !.stage = "active", !.captured = 0, !.since = 0,
       !.hasPreview = FALSE, !.gate = NoGate, !.job = NoJob,
       !.retired = 0, !.lastPub = 0]

\* QwenPreviewRuntime._accept_pcm: the decision at the end of the accept fixes the
\* threshold of the next preview from the count this accept produced.  The
\* weakened branch observes the count from before this block instead.
AcceptPcm(n) ==
  /\ s.stage = "active"
  /\ n \in 1..Step
  /\ s.captured + n <= MaxSamples
  /\ LET captured == s.captured + n
         seen == IF Wrong = "fixed-threshold" THEN FALSE ELSE s.hasPreview
         observed == IF Wrong = "gate-lagged" THEN s.captured ELSE captured
     IN s' = [s EXCEPT !.captured = captured, !.since = @ + n,
          !.gate = [at |-> observed, seen |-> s.hasPreview,
                    value |-> ThresholdFor(observed, seen)]]

\* Single-slot abstraction: _submit replaces a waiting _raw_pending snapshot;
\* _prepare_loop replaces _pending when a newer snapshot finishes preparation.
\* A running native job is outside both replaceable slots.
Launch ==
  /\ s.stage = "active"
  /\ s.since >= s.gate.value
  /\ s.job.st \in {"none", "queued", "prepared"}
  /\ s' = [s EXCEPT
       !.retired = IF s.job.st = "none" THEN s.retired ELSE s.job.upto,
       !.job = [upto |-> s.captured, st |-> "queued", replaces |-> s.job.upto],
       !.since = 0]

\* QwenPipelineRuntime._prepare_loop: the queued job becomes _pending.
Prepare ==
  /\ s.stage = "active"
  /\ s.job.st = "queued"
  /\ s' = [s EXCEPT !.job = [s.job EXCEPT !.st = "prepared"]]

\* QwenPipelineRuntime._worker_loop: the prepared job starts running.
Run ==
  /\ s.stage = "active"
  /\ s.job.st = "prepared"
  /\ s' = [s EXCEPT !.job = [s.job EXCEPT !.st = "running"]]

\* QwenPipelineRuntime._worker_loop: the native decode of that job returns.
Complete ==
  /\ s.stage = "active"
  /\ s.job.st = "running"
  /\ s' = [s EXCEPT !.job = [s.job EXCEPT !.st = "done"]]

\* QwenPipelineRuntime._collect_previews: only the completed job publishes.  The
\* weakened branch publishes the window of a job that was already replaced or
\* cancelled, which is the stale-publication fault.
Publish ==
  /\ s.stage = "active"
  /\ \/ s.job.st = "done" /\ Wrong # "publish-stale"
        /\ s' = [s EXCEPT !.lastPub = s.job.upto, !.hasPreview = TRUE,
             !.job = NoJob]
     \/ Wrong = "publish-stale" /\ s.retired > 0
        /\ s' = [s EXCEPT !.lastPub = s.retired, !.stalePublishes = IF @ < 1 THEN @ + 1 ELSE @,
             !.hasPreview = TRUE]

\* QwenPipelineRuntime._discard_pending / _discard_slot: a job that has not
\* started running is cancelled and must never publish.
Cancel ==
  /\ s.stage = "active"
  /\ s.job.st \in {"queued", "prepared"}
  /\ s' = [s EXCEPT !.retired = s.job.upto, !.job = NoJob]

\* QwenPreviewRuntime.finish: reuse the published preview only when it covers the
\* whole capture and the capture still fits _window_samples; otherwise decode all
\* chunks.  finish() always ends in reset().  This is the base-class rule; the
\* pipeline override's extra "in-flight job with count == _sample_count" reuse
\* source is tested separately (see the module header).
Finish ==
  /\ s.stage = "active"
  /\ s.captured > 0
  /\ LET covered == s.hasPreview /\ s.lastPub = s.captured
         reuse == CASE Wrong = "finish-ignores-coverage" ->
                        s.hasPreview /\ s.captured <= WindowSamples
                   [] Wrong = "finish-ignores-window" -> covered
                   [] OTHER -> covered /\ s.captured <= WindowSamples
     IN s' = [s EXCEPT
          !.finishUsesPreview = reuse,
          !.finishSamples = s.captured,
          !.finishCovered = s.lastPub,
          !.stage = "idle", !.captured = 0, !.since = 0,
          !.hasPreview = FALSE, !.gate = NoGate,
          !.job = NoJob, !.retired = 0, !.lastPub = 0]

\* QwenPreviewRuntime.reset called on its own (error path, stop without finish).
Reset ==
  /\ s.stage = "active"
  /\ s' = [s EXCEPT !.stage = "idle", !.captured = 0, !.since = 0,
       !.hasPreview = FALSE, !.gate = NoGate, !.job = NoJob,
       !.retired = 0, !.lastPub = 0]

Next ==
  \/ Start
  \/ \E n \in 1..Step: AcceptPcm(n)
  \/ Launch
  \/ Prepare
  \/ Run
  \/ Complete
  \/ Publish
  \/ Cancel
  \/ Finish
  \/ Reset

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ s.stage \in {"idle", "active"}
  /\ s.captured \in 0..MaxSamples
  /\ s.since \in 0..MaxSamples
  /\ s.hasPreview \in BOOLEAN
  /\ s.gate.at \in 0..MaxSamples
  /\ s.gate.seen \in BOOLEAN
  /\ s.gate.value \in {StartupThreshold, BaseThreshold, LongThreshold}
  /\ s.job.upto \in 0..MaxSamples
  /\ s.job.replaces \in 0..MaxSamples
  /\ s.job.st \in JobStates
  /\ s.retired \in 0..MaxSamples
  /\ s.lastPub \in 0..MaxSamples
  /\ s.finishUsesPreview \in BOOLEAN
  /\ s.finishSamples \in 0..MaxSamples
  /\ s.finishCovered \in 0..MaxSamples
  /\ s.stalePublishes \in 0..1

\* Stale publication is the fault the cadence change must not introduce: every
\* published preview belongs to the job that was current when it completed.
PublicationIsCurrent == s.stalePublishes = 0

\* Sample count and published text stay consistent: a published preview belongs
\* to a capture that already holds at least its samples.
PreviewTextMatchesSampleCount ==
  /\ s.lastPub <= s.captured
  /\ (s.lastPub > 0 => s.hasPreview)

\* finish() may reuse preview text only when that preview covered every captured
\* sample and the capture still fits the preview window.  Otherwise the full
\* capture is decoded, so no trailing audio is dropped and no truncated window is
\* promoted to the final result.
FinishReuseRequiresFullCoverage ==
  s.finishUsesPreview =>
    (s.finishCovered = s.finishSamples /\ s.finishSamples <= WindowSamples)

\* The threshold that governs the next preview is exactly the value produced from
\* the sample count the decision observed and from whether a preview existed then:
\* the startup threshold before the first preview, BaseThreshold below
\* LongSamples and LongThreshold from LongSamples on.  A threshold that stopped
\* tracking the capture is the cadence fault.  On its own this invariant relates
\* the recorded decision to itself, so it cannot see a decision that lags one
\* block behind; GateObservesLatestCapture pins the observation.
ThresholdTracksSamples ==
  s.stage = "active" =>
    /\ s.gate.at <= s.captured
    /\ s.gate.value = ThresholdFor(s.gate.at, s.gate.seen)

\* The threshold decision is taken at the end of the accept it belongs to, so the
\* count it observed is the count the stream holds now - never the count from the
\* previous block.  This is the clause that a one-block-lagged implementation
\* violates while still satisfying ThresholdTracksSamples.
GateObservesLatestCapture ==
  s.stage = "active" => s.gate.at = s.captured

\* reset() - explicit, on finish() and on start() - returns the startup state.
ResetRestoresStartup ==
  s.stage = "idle" =>
    /\ s.captured = 0 /\ s.since = 0 /\ s.job = NoJob /\ s.lastPub = 0
    /\ ~s.hasPreview /\ s.gate = NoGate

\* Only Launch records which previous job it displaced. Standalone Cancel
\* clears the slot, so a later first launch has replaces = 0.
PendingReplacementNotReached ==
  ~(s.job.st = "queued" /\ s.job.replaces > 0 /\ s.job.replaces = s.retired
    /\ s.retired < s.job.upto)

=============================================================================
