--------------------- MODULE R2T2FinalAcousticWindow ----------------------
EXTENDS Naturals

VARIABLE s
vars == <<s>>

\* One unit is 80 ms. Input represents one callback already admitted by
\* rolling VAD. "finish" uses the CUDA coalesced policy. "preview" and
\* "plain" exercise an ordinary live decode and a native uncoalesced final
\* decode with buffered input. Empty input models an empty stream finish.
Context == 4
Cap(n) == IF n < Context THEN n ELSE Context
Init ==
  \E real \in (0..8 \cup 199..205), trim \in {0, 96, 98, 99, 100}, base \in {1, 2, 3, 4, 8},
     mode \in {"finish", "preview", "plain"}:
    \* Above the 16 s window, an available commit at or before the 8 s
    \* target is assumed. Representative boundaries cover different residues.
    /\ (trim > 0) = (real + (IF mode = "finish" /\ real > 0 THEN Context ELSE 0) > 200)
    /\ s = [phase |-> "capture", real |-> real, trim |-> trim, base |-> base,
            mode |-> mode, public |-> real, logical |-> real, model |-> 0]

\* finalize() transfers the remaining accepted samples and existing context
\* to audio_accum_. No new push crosses the capture boundary.
ContextLength == IF s.mode = "finish" /\ s.real > 0 THEN Context ELSE 0
Drain ==
  /\ s.phase = "capture"
  /\ s' = [s EXCEPT !.phase = "drained", !.logical = @ + ContextLength]

\* decode_rolling_stream_chunk trims at an existing commit, copies the
\* retained interleaved samples, then pads only the copy passed to extract.
Prepare ==
  /\ s.phase = "drained"
  /\ LET window == s.logical - s.trim
         block == Cap(s.base)
         extra == IF s.mode = "finish" THEN (block - (window % block)) % block ELSE 0
     IN s' = [s EXCEPT !.phase = "done", !.model = window + extra]

Done == s.phase = "done" /\ UNCHANGED vars
Next == Drain \/ Prepare \/ Done
Spec == Init /\ [][Next]_vars

CountConservation ==
  /\ s.public = s.real
  /\ s.logical = s.real + (IF s.phase = "capture" THEN 0 ELSE ContextLength)

\* Independent oracle: result is an exact multiple within less than one
\* block of the retained window. Those constraints identify the unique
\* minimal completion, including aligned and zero-length inputs.
MinimalFrontendWindow ==
  s.phase = "done" =>
    LET window == s.real + ContextLength - s.trim
        block == Cap(s.base)
    IN IF s.mode = "finish" THEN
         /\ s.model >= window /\ s.model < window + block /\ s.model % block = 0
       ELSE s.model = window
===========================================================================
