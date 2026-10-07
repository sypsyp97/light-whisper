--------------------------- MODULE GpuIdleSettings ---------------------------
EXTENDS Naturals
VARIABLE s
Init == s = [readPending |-> TRUE, edited |-> FALSE, saving |-> FALSE,
  display |-> 0, saved |-> 0, pending |-> 0, writes |-> 0,
  timeout |-> 1, savedTimeout |-> 1, pendingTimeout |-> 1,
  lastTimeout |-> 1, restarts |-> 0]
BeginSave(value) ==
  /\ ~s.saving /\ value \in 0..2 /\ s.writes < 3
  /\ s' = [s EXCEPT !.edited = TRUE, !.saving = TRUE,
    !.pending = value, !.writes = @ + 1,
    !.pendingTimeout = IF value > 0 THEN value ELSE s.timeout]
FinishSave(success) ==
  /\ s.saving /\ success \in BOOLEAN
  /\ s' = [s EXCEPT !.saving = FALSE,
    !.saved = IF success THEN s.pending ELSE @,
    !.display = IF success THEN s.pending ELSE @,
    !.timeout = IF success THEN s.pendingTimeout ELSE @,
    !.savedTimeout = IF success THEN s.pendingTimeout ELSE @,
    !.lastTimeout = IF success /\ s.pending > 0 THEN s.pending ELSE @]
FinishRead ==
  /\ s.readPending
  /\ s' = [s EXCEPT !.readPending = FALSE,
    !.display = IF ~s.edited THEN 0 ELSE @]
Restart ==
  /\ ~s.saving /\ s.restarts < 1
  /\ s' = [s EXCEPT !.display = s.saved, !.timeout = s.savedTimeout,
    !.readPending = FALSE, !.restarts = @ + 1]
Next == FinishRead \/ Restart \/ (\E v \in 0..2: BeginSave(v))
  \/ \E ok \in BOOLEAN: FinishSave(ok)
Spec == Init /\ [][Next]_s /\ WF_s(FinishRead)
  /\ WF_s(\E ok \in BOOLEAN: FinishSave(ok))
DisplayMatchesLastSave == s.display = s.saved
TimeoutMatchesDisk == s.timeout = s.savedTimeout
DisabledKeepsTimeout == s.saved = 0 => s.savedTimeout = s.lastTimeout
SavesSettle == s.saving ~> ~s.saving
=============================================================================
