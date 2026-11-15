----------------------------- MODULE Workers -----------------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Mode, Target
VARIABLES stage, closing, late
vars == <<stage, closing, late>>
Requests == 1..5
Ingress == {r \in Requests : stage[r] = 1}
Dispatch == {r \in Requests : stage[r] = 2}
Egress == {r \in Requests : stage[r] = 3}
Retained == Ingress \cup Dispatch \cup Egress
Counted == IF Mode = "post-auth-capacity" THEN Dispatch \cup Egress
           ELSE IF Mode = "early-egress-release" THEN Ingress \cup Dispatch
           ELSE Retained
Init == /\ stage = [r \in Requests |-> 0] /\ closing = FALSE /\ late = FALSE
Reserve(r) == /\ stage[r] = 0 /\ (~closing \/ Mode = "late-reservation")
              /\ Cardinality(Counted) < 4
              /\ stage' = [stage EXCEPT ![r] = 1]
              /\ late' = (late \/ closing) /\ UNCHANGED closing
Authenticate(r) == /\ stage[r] = 1 /\ stage' = [stage EXCEPT ![r] = 2]
                   /\ UNCHANGED <<closing, late>>
RejectIngress(r) == /\ stage[r] = 1 /\ stage' = [stage EXCEPT ![r] = 4]
                    /\ UNCHANGED <<closing, late>>
CompleteDispatch(r) == /\ stage[r] = 2 /\ stage' = [stage EXCEPT ![r] = 3]
                       /\ UNCHANGED <<closing, late>>
CompleteEgress(r) == /\ stage[r] = 3 /\ Mode # "stalled-egress"
                     /\ stage' = [stage EXCEPT ![r] = 4]
                     /\ UNCHANGED <<closing, late>>
Close == /\ ~closing /\ closing' = TRUE /\ UNCHANGED <<stage, late>>
Idle == /\ ((closing /\ Retained = {}) \/ Mode = "stalled-egress")
        /\ UNCHANGED vars
Next == Close \/ Idle \/ (\E r \in Requests : Reserve(r) \/ Authenticate(r)
                         \/ RejectIngress(r) \/ CompleteDispatch(r) \/ CompleteEgress(r))
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
TypeOK == /\ stage \in [Requests -> 0..4] /\ closing \in BOOLEAN /\ late \in BOOLEAN
WorkerBound == Cardinality(Retained) <= 4
NoPostCloseReservation == ~late
ClosingDrains == closing ~> Retained = {}
WitnessNotReached == ~CASE Target = "preauth-full" -> Cardinality(Ingress) = 4 /\ Dispatch = {}
                       [] Target = "egress-full" -> Cardinality(Egress) = 4 /\ Dispatch = {}
                       [] OTHER -> FALSE
=============================================================================
