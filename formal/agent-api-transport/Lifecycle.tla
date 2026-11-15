--------------------------- MODULE Lifecycle ---------------------------
EXTENDS Naturals, FiniteSets
CONSTANTS Mode, Target
VARIABLE s
vars == <<s>>
Requests == 1..5
Init == /\ s \in {[phase |-> "new", lease |-> 0, hosts |-> {}, descriptor |-> previous,
             captured |-> 0, admitted |-> {}, active |-> {}, finished |-> {},
             disconnected |-> {}, rejected |-> {}, canceled |-> {},
             deletedForeign |-> FALSE, earlyCleanup |-> FALSE, late |-> FALSE,
             contenderTried |-> FALSE, rewritten |-> FALSE, lockGeneration |-> 1] : previous \in {0, 2}}
Start == /\ s.phase = "new" /\ s.lease = 0
         /\ s' = [s EXCEPT !.phase = "leased", !.lease = 1, !.hosts = {1}]
Bind == /\ s.phase = "leased" /\ s' = [s EXCEPT !.phase = "bound"]
Publish == /\ s.phase = "bound"
           /\ s' = [s EXCEPT !.phase = "serving", !.descriptor = 1]
StartFailure == /\ s.phase \in {"leased", "bound"}
                /\ s' = [s EXCEPT !.phase = "draining"]
AcquireFailure == /\ s.phase = "new" /\ s' = [s EXCEPT !.phase = "released"]
UnleasedPublish == /\ s.phase = "new" /\ Mode = "unleased-publish"
                   /\ s' = [s EXCEPT !.phase = "serving", !.descriptor = 1, !.hosts = {1}]
Contend == /\ s.phase \in {"leased", "bound", "serving", "draining", "checked", "cleaned"} /\ ~s.contenderTried
           /\ s' = [s EXCEPT !.contenderTried = TRUE,
                        !.hosts = IF Mode = "lease" THEN {1, 2} ELSE @]
Admit(r) == /\ r \notin s.admitted \cup s.rejected
            /\ (s.phase = "serving" \/ (s.phase = "draining" /\ Mode = "late-admit"))
            /\ (Cardinality(s.active) < 4 \/ Mode = "capacity")
            /\ s' = [s EXCEPT !.admitted = @ \cup {r}, !.active = @ \cup {r},
                         !.late = @ \/ s.phase # "serving"]
Reject(r) == /\ s.phase = "serving" /\ Cardinality(s.active) = 4
             /\ r \notin s.admitted \cup s.rejected
             /\ s' = [s EXCEPT !.rejected = @ \cup {r}]
Disconnect(r) == /\ r \in s.active \ s.disconnected
                 /\ s' = [s EXCEPT !.disconnected = @ \cup {r},
                              !.active = IF Mode = "cancel" THEN @ \ {r} ELSE @,
                              !.canceled = IF Mode = "cancel" THEN @ \cup {r} ELSE @]
Finish(r) == /\ r \in s.active /\ Mode # "stalled-dispatch"
             /\ s' = [s EXCEPT !.active = @ \ {r}, !.finished = @ \cup {r}]
Close == /\ s.phase = "serving" /\ s' = [s EXCEPT !.phase = "draining"]
Rewrite == /\ s.phase \in {"serving", "draining"} /\ ~s.rewritten
           /\ s' = [s EXCEPT !.rewritten = TRUE, !.descriptor = 2]
ReplaceLock == /\ Mode = "replace-lock" /\ s.lease = 1 /\ s.lockGeneration = 1
               /\ s' = [s EXCEPT !.lockGeneration = 2]
ObserveCleanup == /\ s.phase = "draining" /\ (s.active = {} \/ Mode = "early-cleanup")
                  /\ Mode # "stalled-cleanup"
                  /\ s' = [s EXCEPT !.phase = "checked", !.captured = s.descriptor,
                               !.earlyCleanup = s.active # {}]
Unlink == /\ s.phase = "checked"
          /\ s' = [s EXCEPT !.phase = "cleaned",
                       !.deletedForeign = s.descriptor = 2 /\ Mode = "foreign-delete",
                       !.descriptor = IF s.captured = 1 \/ Mode = "foreign-delete" THEN 0 ELSE @]
Release == /\ (s.phase = "cleaned" \/ (s.phase = "draining" /\ Mode = "early-release"))
           /\ s' = [s EXCEPT !.phase = "released", !.lease = 0, !.hosts = {}]
Successor == /\ s.phase = "released" /\ s.lease = 0 /\ s.active = {}
             /\ s' = [s EXCEPT !.phase = "successor", !.lease = 2, !.hosts = {2}, !.descriptor = 2]
Idle == /\ (s.phase = "successor" \/ Mode \in {"stalled-dispatch", "stalled-cleanup"})
        /\ UNCHANGED vars
Next == Start \/ Bind \/ Publish \/ StartFailure \/ AcquireFailure \/ UnleasedPublish
        \/ Contend \/ Close \/ Rewrite \/ ReplaceLock \/ ObserveCleanup \/ Unlink \/ Release \/ Successor \/ Idle
        \/ (\E r \in Requests : Admit(r) \/ Reject(r) \/ Disconnect(r) \/ Finish(r))
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
TypeOK == /\ s.phase \in {"new", "leased", "bound", "serving", "draining", "checked", "cleaned", "released", "successor"}
          /\ s.lease \in 0..2 /\ s.hosts \subseteq {1, 2} /\ s.descriptor \in 0..2 /\ s.captured \in 0..2
          /\ s.admitted \subseteq Requests /\ s.active \subseteq s.admitted /\ s.finished \subseteq s.admitted
          /\ s.disconnected \subseteq s.admitted /\ s.rejected \subseteq Requests /\ s.canceled \subseteq s.admitted
          /\ s.deletedForeign \in BOOLEAN /\ s.earlyCleanup \in BOOLEAN /\ s.late \in BOOLEAN
          /\ s.contenderTried \in BOOLEAN /\ s.rewritten \in BOOLEAN /\ s.lockGeneration \in 1..2
SingleHost == Cardinality(s.hosts) <= 1
PublishedUnderLease == s.phase = "serving" => s.lease = 1
WorkerBound == Cardinality(s.active) <= 4
NoCanceledDispatch == s.canceled = {}
NoPostCloseAdmission == ~s.late
DrainedCleanup == ~s.earlyCleanup
PreserveForeignDescriptor == ~s.deletedForeign
ReleaseAfterCleanup == s.phase = "released" => s.active = {} /\ s.descriptor # 1
StableLeasePath == s.lease = 1 => s.lockGeneration = 1
AdmittedSettles == \A r \in Requests : (r \in s.admitted ~> r \in s.finished)
DropCompletes == s.phase = "draining" ~> s.phase \in {"released", "successor"}
WitnessNotReached == ~CASE Target = "capacity" -> Cardinality(s.active) = 4 /\ s.rejected # {}
                       [] Target = "disconnect" -> s.disconnected # {} /\ s.active # {}
                       [] Target = "preserved" -> s.phase = "released" /\ s.descriptor = 2
                       [] Target = "successor" -> s.phase = "successor" /\ s.contenderTried
                       [] Target = "drained" -> s.phase = "released" /\ s.finished # {}
                       [] OTHER -> FALSE
=============================================================================
