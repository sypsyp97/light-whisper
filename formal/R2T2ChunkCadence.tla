------------------------- MODULE R2T2ChunkCadence -------------------------
EXTENDS Naturals

CONSTANTS Wrong, ChunkSamples, MaxSamples
VARIABLE s
vars == <<s>>

\* One unit is 80 ms. Both a short Python feed and a final 80 ms tail are
\* reachable. Speech is already admitted by VAD, with no batched pre-roll.
\* caps denotes the CUDA ABI 0.6+ path, legacy the fixed CUDA 160 ms path.
Modes == {"caps", "legacy", "cpu"}
Input(mode) == IF mode = "cpu" THEN ChunkSamples ELSE ChunkSamples \div 2
Expected == IF s.mode = "caps" /\ s.seen THEN ChunkSamples ELSE Input(s.mode)
Target == IF s.steady THEN ChunkSamples ELSE s.grain

SessionState ==
  [phase |-> "new", mode |-> "caps", grain |-> Input("caps"),
   open |-> FALSE, seen |-> FALSE, steady |-> FALSE, fixed |-> FALSE, visible |-> FALSE,
   accepted |-> 0, decoded |-> 0, discarded |-> 0, py |-> 0, dll |-> 0,
   tail |-> 0, finishPending |-> 0, finishCalls |-> 0, blockOK |-> TRUE]

Init == s = SessionState

\* R2T2ASRServer._load_runtime / NativeRuntime.__init__ map backend and
\* negotiated ABI to a fixed Python input grain, separate from DLL grouping.
Load(mode) ==
  /\ s.phase = "new" /\ mode \in Modes
  /\ s' = [s EXCEPT !.phase = "loaded", !.mode = mode,
           !.grain = IF Wrong = "cuda-320" /\ mode = "caps"
                     THEN ChunkSamples ELSE Input(mode)]

\* A new native stream starts without preview history. Finish and start are
\* separate actions; an abandoned half block is handled by Reset instead.
Start ==
  /\ s.phase \in {"loaded", "finished"}
  /\ s' = [SessionState EXCEPT !.phase = "active", !.mode = s.mode,
           !.grain = s.grain,
           !.steady = Wrong = "restart-leak" /\ s.steady]

\* SegmentedR2T2.feed retains its incomplete input; native process_audio_chunk
\* retains a second, independent remainder. At most one input grain crosses
\* the ABI per step, so a decode can occur at most once per step.
\* A complete native decode may publish speculative text, commit text, or
\* withdraw a speculative preview. A push without a decode emits no text.
Feed(n, preview, commit) ==
  /\ s.phase = "active" /\ n \in 1..s.grain
  /\ s.accepted + n <= MaxSamples
  /\ LET pyTotal == s.py + n
         delivered == (pyTotal \div s.grain) * s.grain
         avail == s.dll + delivered
         full == avail >= Target
         early == Wrong = "early-block" /\ delivered > 0 /\ avail < Target
         count == IF full THEN Target ELSE IF early THEN avail ELSE 0
         runs == count > 0
         fixed == s.fixed \/ (runs /\ commit)
         visible == IF runs THEN
                      IF Wrong = "preview-regress" THEN preview ELSE preview \/ fixed
                    ELSE s.visible
         seen == s.seen \/ (runs /\ visible)
     IN s' = [s EXCEPT !.py = pyTotal % s.grain, !.dll = avail - count,
              !.accepted = @ + n, !.decoded = @ + count, !.open = @ \/ delivered > 0,
              !.blockOK = @ /\ (~runs \/ count = Expected),
              !.fixed = fixed, !.visible = visible, !.seen = seen,
              !.steady = s.mode = "caps" /\
                  (IF Wrong = "preview-unlatch" THEN visible
                   ELSE IF Wrong = "early-steady" THEN runs \/ s.steady
                   ELSE IF Wrong = "never-steady" THEN FALSE ELSE seen)]

\* _finish_segment -> native.finish; next admitted speech -> native.start.
\* Coalesced final acoustic zero context is synthetic, excluded from counts.
Pause ==
  /\ s.phase = "active" /\ s.py = 0 /\ s.open
  /\ s' = [s EXCEPT !.tail = @ + s.dll, !.dll = 0,
           !.open = FALSE, !.fixed = FALSE, !.visible = FALSE, !.seen = FALSE,
           !.steady = Wrong = "restart-leak" /\ s.steady]

\* Explicit cancellation discards queued PCM and the following start opens a
\* fresh stream. The abandoned samples remain accounted for only in discarded.
Reset ==
  /\ s.phase = "active"
  /\ s' = [s EXCEPT !.discarded = @ + s.py + s.dll, !.py = 0, !.dll = 0,
           !.open = FALSE, !.fixed = FALSE, !.visible = FALSE, !.seen = FALSE,
           !.steady = Wrong = "reset-leak" /\ s.steady]

\* SegmentedR2T2.finish flushes the Python remainder; native.finalize then
\* decodes the DLL remainder plus synthesis-only context in one final pass.
Finish ==
  /\ s.phase = "active"
  /\ LET pending == s.py + s.dll
     IN s' = [s EXCEPT !.phase = "finished", !.finishCalls = @ + 1,
              !.finishPending = pending, !.py = 0, !.dll = 0,
              !.tail = @ + (IF Wrong = "finish-drops-pending" THEN 0 ELSE pending)]

Next ==
  \/ \E mode \in Modes: Load(mode)
  \/ Start
  \/ \E n \in 1..ChunkSamples, preview \in BOOLEAN, commit \in BOOLEAN: Feed(n, preview, commit)
  \/ Pause \/ Reset \/ Finish

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ s.phase \in {"new", "loaded", "active", "finished"} /\ s.mode \in Modes
  /\ s.grain \in 1..ChunkSamples
  /\ s.py \in 0..(s.grain - 1) /\ s.dll \in 0..(ChunkSamples - 1)
  /\ s.accepted \in 0..MaxSamples /\ s.decoded \in 0..MaxSamples
  /\ s.tail \in 0..MaxSamples /\ s.discarded \in 0..MaxSamples
  /\ s.finishCalls \in 0..1 /\ s.finishPending \in 0..MaxSamples
  /\ {s.open, s.seen, s.steady, s.fixed, s.visible, s.blockOK} \subseteq BOOLEAN

CadenceMatchesPreviewHistory ==
  s.phase # "new" => s.grain = Input(s.mode) /\ Target = Expected
SampleConservation == s.accepted = s.decoded + s.tail + s.discarded + s.py + s.dll
OnlyFullBlocksProcessed == s.blockOK
FinishFlushesPendingOnce ==
  s.phase = "finished" => s.py + s.dll = 0 /\ s.finishCalls = 1
PreviewExtendsCommitted == s.fixed => s.visible
FinishWithoutPendingNotReached == ~(s.phase = "finished" /\ s.seen /\ s.mode = "caps" /\ s.finishPending = 3)
=============================================================================
