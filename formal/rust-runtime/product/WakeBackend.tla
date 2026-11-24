-------------------------- MODULE WakeBackend --------------------------
EXTENDS TLC
CONSTANT RetryCpu
VARIABLE phase
vars == <<phase>>
Init == phase = "cuda-loaded"
Suspend == /\ phase = "cuda-loaded" /\ phase' = "idle"
LoseCuda == /\ phase = "idle" /\ phase' = "only-cpu-available"
Wake == /\ phase = "only-cpu-available"
        /\ phase' = IF RetryCpu THEN "cpu-loaded" ELSE "error"
Next == Suspend \/ LoseCuda \/ Wake \/ (phase \in {"cpu-loaded","error"} /\ UNCHANGED phase)
Spec == Init /\ [][Next]_vars /\ WF_vars(Wake)
AvailableBackendUsed == phase # "error"
WakeCompletes == (phase = "only-cpu-available") ~> (phase = "cpu-loaded")
=======================================================================
