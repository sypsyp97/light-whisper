---------------------------- MODULE Deadlines ----------------------------
EXTENDS Naturals
CONSTANTS Mode, Target, Kind
VARIABLES phase, elapsed, budget, chunks, completed, expired, dispatchDuration
vars == <<phase, elapsed, budget, chunks, completed, expired, dispatchDuration>>
Init == /\ phase = IF Kind = "egress" THEN "dispatch" ELSE "read"
        /\ elapsed = 0 /\ budget = 0 /\ chunks = 0
        /\ completed = FALSE /\ expired = FALSE /\ dispatchDuration = 0
DispatchTick == /\ phase = "dispatch" /\ dispatchDuration < 3
                /\ dispatchDuration' = dispatchDuration + 1
                /\ UNCHANGED <<phase, elapsed, budget, chunks, completed, expired>>
DispatchDone == /\ phase = "dispatch" /\ phase' = "read"
                /\ budget' = IF Mode = "egress-from-accept" THEN dispatchDuration ELSE 0
                /\ UNCHANGED <<elapsed, chunks, completed, expired, dispatchDuration>>
Tick == /\ phase = "read" /\ elapsed < 3
        /\ elapsed' = elapsed + 1 /\ budget' = budget + 1
        /\ UNCHANGED <<phase, chunks, completed, expired, dispatchDuration>>
Partial == /\ phase = "read" /\ chunks < 2
           /\ chunks' = chunks + 1
           /\ budget' = IF Mode = "reset-on-progress" THEN 0 ELSE budget
           /\ UNCHANGED <<phase, elapsed, completed, expired, dispatchDuration>>
Complete == /\ phase = "read" /\ chunks = 2 /\ budget < 2
            /\ phase' = "terminal" /\ completed' = TRUE
            /\ UNCHANGED <<elapsed, budget, chunks, expired, dispatchDuration>>
Expire == /\ phase = "read" /\ budget >= 2 /\ Mode # "stalled-expiry"
          /\ phase' = "terminal" /\ expired' = TRUE
          /\ UNCHANGED <<elapsed, budget, chunks, completed, dispatchDuration>>
Idle == /\ phase = "terminal" \/ Mode = "stalled-expiry"
        /\ UNCHANGED vars
Next == DispatchTick \/ DispatchDone \/ Tick \/ Partial \/ Complete \/ Expire \/ Idle
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
TypeOK == /\ Kind \in {"ingress", "client", "egress"}
          /\ phase \in {"dispatch", "read", "terminal"}
          /\ elapsed \in 0..3 /\ budget \in 0..6 /\ chunks \in 0..2
          /\ dispatchDuration \in 0..3 /\ completed \in BOOLEAN /\ expired \in BOOLEAN
AbsoluteDeadline == completed => elapsed < 2
EgressStartsAfterDispatch == expired => elapsed >= 2
EventuallySettled == <> (completed \/ expired)
WitnessNotReached == ~CASE Target = "expired" -> expired /\ elapsed = 2
                       [] Target = "dispatch-outlives" -> completed /\ dispatchDuration = 3
                       [] Target = "partial" -> completed /\ chunks = 2 /\ elapsed = 1
                       [] OTHER -> FALSE
=============================================================================
