---- MODULE NativeStream ----
EXTENDS Naturals
CONSTANT Fault
VARIABLES phase, supplied, nextOffset, event, pad, submitted, coalesced
vars == <<phase, supplied, nextOffset, event, pad, submitted, coalesced>>
Init == /\ phase = "idle" /\ supplied = 0 /\ nextOffset = 0 /\ event = FALSE
        /\ pad = 0 /\ submitted = 0 /\ coalesced \in BOOLEAN
Start == /\ phase = "idle" /\ phase' = "active"
         /\ supplied' = 0 /\ nextOffset' = 0 /\ submitted' = 0 /\ pad' = 0
         /\ UNCHANGED <<event, coalesced>>
Push == /\ phase = "active" /\ supplied < 2 /\ ~event
        /\ supplied' = supplied + 1 /\ submitted' = submitted + 1
        /\ nextOffset' = IF Fault = "Offset" THEN nextOffset ELSE nextOffset + 1
        /\ event' = TRUE /\ phase' = "read" /\ UNCHANGED <<pad, coalesced>>
Release(ok) == /\ phase = "read" /\ phase' = IF ok THEN "active" ELSE "error"
               /\ event' = IF ~ok /\ Fault = "Leak" THEN TRUE ELSE FALSE
               /\ UNCHANGED <<supplied, nextOffset, pad, submitted, coalesced>>
BeginFinish == /\ phase = "active" /\ phase' = "finishing"
               /\ pad' = IF supplied > 0 /\ (~coalesced \/ Fault = "DoublePad") THEN 1 ELSE 0
               /\ UNCHANGED <<supplied, nextOffset, event, submitted, coalesced>>
Finish == /\ phase = "finishing" /\ phase' = "done"
          /\ pad' = pad + IF supplied > 0 /\ coalesced THEN 1 ELSE 0
          /\ UNCHANGED <<supplied, nextOffset, event, submitted, coalesced>>
Reset == /\ phase \in {"active","error","done"} /\ phase' = "idle"
         /\ nextOffset' = 0 /\ supplied' = 0 /\ pad' = 0 /\ submitted' = 0
         /\ UNCHANGED <<event, coalesced>>
Next == Start \/ Push \/ Release(TRUE) \/ Release(FALSE) \/ BeginFinish \/ Finish \/ Reset
Spec == Init /\ [][Next]_vars
Contiguous == nextOffset = submitted
NoLeakedEvent == event = (phase = "read")
FinalContextOnce == phase = "done" => pad = (IF supplied > 0 THEN 1 ELSE 0)
====
