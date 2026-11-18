---------------------------- MODULE PageNavigation ----------------------------
EXTENDS Naturals, TLC

CONSTANTS MaxRequests, Mode

VARIABLES view, mounted, serial, owner, requests, publications, audit

Vars == <<view, mounted, serial, owner, requests, publications, audit>>
Pages == {"main", "settings", "history"}
Ids == 1..MaxRequests
Missing == [target |-> "main", state |-> "unused"]
RequestOf(target, state) == [target |-> target, state |-> state]
Audit(kind, expected) == [
  kind |-> kind, expected |-> expected, view |-> view,
  owner |-> owner, publications |-> publications
]

Init ==
  /\ view = "main"
  /\ mounted = TRUE
  /\ serial = 0
  /\ owner = 0
  /\ requests = [id \in Ids |-> Missing]
  /\ publications = 0
  /\ audit = [kind |-> "init", expected |-> "main", view |-> "main",
              owner |-> 0, publications |-> 0]

Choose(target) ==
  /\ mounted
  /\ serial < MaxRequests
  /\ target # view \/ owner # 0
  /\ LET id == serial + 1
         immediate == target = "main" \/ target = view
         dropped == owner # 0 /\ Mode = "legacy-lock"
     IN /\ serial' = id
        /\ requests' = [requests EXCEPT ![id] =
              RequestOf(target, IF dropped \/ immediate THEN "done" ELSE "waiting")]
        /\ owner' = IF dropped THEN owner ELSE IF immediate THEN 0 ELSE id
        /\ view' = IF ~dropped /\ immediate THEN target ELSE view
        /\ audit' = Audit(IF dropped THEN "ignored" ELSE IF immediate THEN "immediate" ELSE "waiting", target)
  /\ UNCHANGED <<mounted, publications>>

Settle(id, ok) ==
  /\ requests[id].state = "waiting"
  /\ LET target == requests[id].target
         isCurrent == mounted /\ owner = id
         delayed == isCurrent /\ ok /\ Mode = "legacy-delay"
         publish == isCurrent \/ (~mounted /\ owner = id /\ Mode = "closed-owner")
                    \/ (ok /\ Mode = "stale-commit") \/ (~ok /\ Mode = "stale-toast")
     IN /\ requests' = [requests EXCEPT ![id] = RequestOf(target, "done")]
        /\ owner' = IF publish /\ ~delayed /\ (ok \/ Mode # "failure-lock") THEN 0 ELSE owner
        /\ view' = IF publish /\ ok /\ ~delayed THEN target ELSE view
        /\ publications' = IF publish /\ ~ok THEN publications + 1 ELSE publications
        /\ audit' = Audit(IF ~isCurrent THEN "stale" ELSE IF ok THEN "ready" ELSE "failed", target)
  /\ UNCHANGED <<mounted, serial>>

Unmount ==
  /\ mounted
  /\ mounted' = FALSE
  /\ owner' = IF Mode = "closed-owner" THEN owner ELSE 0
  /\ audit' = Audit("unmount", view)
  /\ UNCHANGED <<view, serial, requests, publications>>

Quiescent ==
  /\ ~mounted
  /\ \A id \in Ids : requests[id].state # "waiting"
  /\ UNCHANGED Vars

Next ==
  \/ \E target \in Pages : Choose(target)
  \/ \E id \in Ids, ok \in BOOLEAN : Settle(id, ok)
  \/ Unmount
  \/ Quiescent

Spec ==
  /\ Init
  /\ [][Next]_Vars
  /\ \A id \in Ids : WF_Vars(\E ok \in BOOLEAN : Settle(id, ok))

TypeOK ==
  /\ view \in Pages
  /\ mounted \in BOOLEAN
  /\ serial \in 0..MaxRequests
  /\ owner \in 0..serial
  /\ requests \in [Ids -> [target : Pages, state : {"unused", "waiting", "done"}]]
  /\ publications \in 0..MaxRequests
  /\ audit \in [kind : {"init", "ignored", "immediate", "waiting", "ready", "failed", "stale", "unmount"},
                expected : Pages, view : Pages, owner : 0..MaxRequests, publications : 0..MaxRequests]

NoStalePublication ==
  audit.kind = "stale" => <<view, owner, publications>> = <<audit.view, audit.owner, audit.publications>>

OnlyLatestChoice ==
  /\ audit.kind # "ignored"
  /\ audit.kind = "waiting" => view = audit.view /\ owner = serial

ReadyPublishesImmediately ==
  audit.kind \in {"ready", "immediate"} => view = audit.expected /\ owner = 0

FailureSettles ==
  audit.kind = "failed" => view = audit.view /\ publications = audit.publications + 1 /\ owner = 0

UnmountReleases ==
  ~mounted => owner = 0

EventuallySettled ==
  \A id \in Ids : (owner = id /\ requests[id].state = "waiting") ~> (owner # id)

NoSupersededCompletionWitness ==
  ~(mounted /\ audit.kind = "stale" /\ view = "history" /\ audit.expected = "settings")

NoPostUnmountCompletionWitness ==
  ~(~mounted /\ audit.kind = "stale")

=============================================================================
