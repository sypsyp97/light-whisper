-------------------------- MODULE EngineLifecycle --------------------------
EXTENDS Naturals
CONSTANT Fault
VARIABLES phase, owner, last, count, acceptedWrong, staleFinish
vars == <<phase, owner, last, count, acceptedWrong, staleFinish>>
Init == /\ phase = "boot" /\ owner = 0 /\ last = 0 /\ count = 0
        /\ acceptedWrong = FALSE /\ staleFinish = FALSE
Startup == /\ phase = "boot" /\ phase' \in {"ready", "failed"}
           /\ UNCHANGED <<owner, last, count, acceptedWrong, staleFinish>>
Start(i) == /\ phase = "ready" /\ i > last /\ owner' = i /\ last' = i
            /\ count' = 0 /\ UNCHANGED <<phase, acceptedWrong, staleFinish>>
Feed(i, offset) ==
    /\ phase = "ready" /\ owner # 0 /\ count < 2
    /\ (i = owner \/ Fault = "id") /\ (offset = count \/ Fault = "offset")
    /\ count' = count + 1 /\ acceptedWrong' = (i # owner \/ offset # count)
    /\ UNCHANGED <<phase, owner, last, staleFinish>>
FinishOrCancel(i) ==
    /\ phase = "ready" /\ owner # 0 /\ (i = owner \/ Fault = "finish")
    /\ staleFinish' = (i # owner) /\ owner' = 0 /\ count' = 0
    /\ UNCHANGED <<phase, last, acceptedWrong>>
Idle == /\ phase = "ready" /\ (owner = 0 \/ Fault = "idle")
        /\ phase' = "suspended" /\ UNCHANGED <<owner, last, count, acceptedWrong, staleFinish>>
Resume == /\ phase = "suspended" /\ phase' = "ready"
          /\ last' = 0 /\ UNCHANGED <<owner, count, acceptedWrong, staleFinish>>
Exit == /\ phase # "stopped" /\ phase' = "stopped" /\ owner' = 0 /\ count' = 0
        /\ UNCHANGED <<last, acceptedWrong, staleFinish>>
Next == Startup \/ Idle \/ Resume \/ Exit \/ (phase = "stopped" /\ UNCHANGED vars)
        \/ (\E i \in 1..2 : Start(i) \/ FinishOrCancel(i) \/ (\E k \in 0..2 : Feed(i,k)))
Spec == Init /\ [][Next]_vars
OwnedRuntime == owner = 0 \/ phase = "ready"
AlignedAudio == ~acceptedWrong
NoStaleCompletion == ~staleFinish
=============================================================================
