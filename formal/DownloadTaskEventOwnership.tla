----------------------- MODULE DownloadTaskEventOwnership -------------------
EXTENDS Naturals, FiniteSets
CONSTANT Wrong
Ids == 1..2
ActivePhases == {"preparing", "progress", "waiting"}
Statuses == {"progress", "completed", "cancelled", "error"}
VARIABLE s
Init == s = [started |-> 0, slot |-> 0,
  phase |-> [i \in Ids |-> "unused"], terminal |-> [i \in Ids |-> FALSE],
  cancelRequested |-> {}, waiters |-> [i \in Ids |-> 0], acked |-> [i \in Ids |-> 0],
  uiAttempt |-> 0, owner |-> 0, busy |-> FALSE, promises |-> {},
  badEvent |-> FALSE, badPromise |-> FALSE, badCancel |-> FALSE,
  currentSuccessRestarted |-> FALSE]
Begin(i) ==
  /\ i = s.started + 1 /\ i \in Ids /\ s.slot = 0 /\ ~s.busy
  /\ s' = [s EXCEPT !.started = i, !.slot = i, !.phase[i] = "preparing",
    !.uiAttempt = i, !.owner = i, !.busy = TRUE, !.promises = @ \cup {i}]
Advance(i) ==
  /\ i = s.slot /\ i \notin s.cancelRequested
  /\ s.phase[i] \in {"preparing", "progress"}
  /\ s' = [s EXCEPT !.phase[i] = IF @ = "preparing" THEN "progress" ELSE "waiting"]
Cancel(i) ==
  /\ i <= s.started /\ s.waiters[i] < 2
  /\ LET target == IF s.slot = 0 THEN 0
         ELSE IF Wrong = "old-cancel" \/ s.slot = i THEN s.slot ELSE 0
     IN s' = [s EXCEPT !.waiters[i] = @ + 1,
       !.cancelRequested = IF target = 0 THEN @ ELSE @ \cup {target},
       !.owner = IF @ = i THEN 0 ELSE @,
       !.badCancel = @ \/ (target # 0 /\ target # i)]
\* Preparing includes waiting for any already-started preparation to settle;
\* StopForCancel abstracts that settlement, not instantaneous interruption.
\* Fairness assumes preparation, child kill, and reap finish; it gives no
\* wall-clock bound. Cancellation also covers stdout progress and EOF wait.
StopForCancel(i) ==
  /\ i \in s.cancelRequested /\ s.phase[i] \in ActivePhases
  /\ s' = [s EXCEPT !.phase[i] = "exited"]
NormalExit(i) ==
  /\ s.phase[i] = "waiting" /\ i \notin s.cancelRequested
  /\ s' = [s EXCEPT !.phase[i] = "exited"]
PublishTerminal(i) ==
  /\ s.phase[i] = "exited" /\ ~s.terminal[i]
  /\ s' = [s EXCEPT !.terminal[i] = TRUE]
Cleanup(i) ==
  /\ s.phase[i] = "exited" /\ s.terminal[i]
  /\ s' = [s EXCEPT !.slot = IF @ = i THEN 0 ELSE @, !.phase[i] = "done"]
Ack(i) ==
  /\ s.acked[i] < s.waiters[i]
  /\ Wrong = "early-ack" \/ (s.phase[i] = "done" /\ s.terminal[i] /\ s.slot # i)
  /\ s' = [s EXCEPT !.acked[i] = @ + 1,
    !.busy = IF s.uiAttempt = i /\ s.owner = 0 THEN FALSE ELSE @]
Event(i, status) ==
  /\ i \in 0..s.started /\ status \in Statuses /\ s.started > 0
  /\ LET owned == i # 0 /\ i = s.owner
         accept == owned \/ Wrong = "old-event"
         terminal == status # "progress"
     IN s' = [s EXCEPT !.badEvent = @ \/ (accept /\ ~owned),
       !.busy = IF accept THEN ~terminal ELSE @,
       !.owner = IF accept /\ terminal THEN 0 ELSE @,
       !.currentSuccessRestarted = @ \/ (accept /\ owned /\ status = "completed")]
PromiseFinish(i) ==
  /\ i \in s.promises /\ s.phase[i] = "done"
  /\ LET owned == i = s.owner
     IN s' = [s EXCEPT !.promises = @ \ {i},
       !.busy = IF owned THEN FALSE ELSE @,
       !.owner = IF owned THEN 0 ELSE @,
       !.badPromise = @ \/ (owned /\ i # s.uiAttempt)]
Next == (\E i \in Ids: Begin(i) \/ Advance(i) \/ Cancel(i)
  \/ StopForCancel(i) \/ NormalExit(i) \/ PublishTerminal(i) \/ Cleanup(i)
  \/ Ack(i) \/ PromiseFinish(i)) \/ (\E i \in 0..2, status \in Statuses: Event(i, status))
Spec == Init /\ [][Next]_s /\ \A i \in Ids:
  WF_s(StopForCancel(i)) /\ WF_s(PublishTerminal(i)) /\ WF_s(Cleanup(i))
OnlyCurrentRequestPublishes == ~s.badEvent /\ ~s.badPromise
CancellationTargetsCapturedRequest == ~s.badCancel
AckFollowsTerminalAndSlotRelease == \A i \in Ids:
  s.acked[i] > 0 => s.phase[i] = "done" /\ s.terminal[i] /\ s.slot # i
WorkerKeepsItsSlot == \A i \in Ids:
  s.phase[i] \in ActivePhases \cup {"exited"} => s.slot = i
TerminalFollowsWorkerExit == \A i \in Ids:
  s.terminal[i] => s.phase[i] \in {"exited", "done"}
CancellationRetiresPublication == s.owner = 0 \/ s.owner \notin s.cancelRequested
CancellationFinishes == \A i \in Ids: i \in s.cancelRequested ~> s.phase[i] = "done"
=============================================================================
