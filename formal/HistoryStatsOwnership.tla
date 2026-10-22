------------------------ MODULE HistoryStatsOwnership ------------------------
EXTENDS Naturals, FiniteSets, Sequences
CONSTANT Wrong
VARIABLE s, lastEvent
vars == <<s, lastEvent>>
Text(id) == CASE id = 1 -> "1" [] id = 2 -> "2" [] OTHER -> "3"
Init ==
  /\ s = [issued |-> 0, owner |-> 0, intended |-> 0, pending |-> {},
    mounted |-> TRUE, snapshot |-> 0, wantedSnapshot |-> 0,
    publications |-> <<>>, wantedPublications |-> <<>>,
    errors |-> <<>>, wantedErrors |-> <<>>, latestFailed |-> FALSE,
    failureSnapshot |-> 0, recovered |-> FALSE, disposedPending |-> FALSE]
  /\ lastEvent = "init"
Begin ==
  /\ s.mounted /\ s.issued < 3
  /\ LET id == s.issued + 1 IN
     s' = [s EXCEPT !.issued = id, !.owner = id, !.intended = id,
       !.pending = @ \cup {id}, !.latestFailed = FALSE]
  /\ lastEvent' = "history-stats:begin:" \o Text(s.issued + 1)
Complete(id, success) ==
  /\ id \in s.pending
  /\ LET publish == success /\ (s.owner = id \/ Wrong = "publication")
         logError == ~success /\ (s.owner = id \/ Wrong = "error")
         intended == s.mounted /\ s.intended = id
     IN s' = [s EXCEPT !.pending = @ \ {id},
       !.snapshot = IF publish THEN id ELSE @,
       !.wantedSnapshot = IF intended /\ success THEN id ELSE @,
       !.publications = IF publish THEN Append(@, id) ELSE @,
       !.wantedPublications = IF intended /\ success THEN Append(@, id) ELSE @,
       !.errors = IF logError THEN Append(@, id) ELSE @,
       !.wantedErrors = IF intended /\ ~success THEN Append(@, id) ELSE @,
       !.latestFailed = IF intended THEN ~success ELSE @,
       !.failureSnapshot = IF intended /\ ~success THEN s.wantedSnapshot ELSE @,
       !.recovered = @ \/ (intended /\ success /\ Len(s.wantedErrors) > 0)]
  /\ lastEvent' = "history-stats:" \o
       (IF success THEN "success:" ELSE "failure:") \o Text(id)
Dispose ==
  /\ s.mounted
  /\ s' = [s EXCEPT !.mounted = FALSE, !.intended = 0,
    !.owner = IF Wrong = "cleanup" THEN @ ELSE 0,
    !.disposedPending = Cardinality(s.pending) > 0]
  /\ lastEvent' = "history-stats:dispose"
Next == Begin \/ Dispose \/ (\E id \in 1..3, success \in BOOLEAN: Complete(id, success))
Spec == Init /\ [][Next]_vars
PublicationOwned == s.snapshot = s.wantedSnapshot /\ s.publications = s.wantedPublications
ErrorOwned == s.errors = s.wantedErrors
AcceptedSnapshotSurvivesCurrentFailure ==
  s.latestFailed /\ s.failureSnapshot # 0 => s.snapshot = s.failureSnapshot
RecoveryAndPendingDisposalNotReached == ~(s.recovered /\ s.disposedPending)
\* Three ordered request identities, each completing once with environmental
\* success/error input; starts abstract mount/history-updated/delete/reprocess.
\* owner is the implementation request token; intended and wanted ledgers are
\* independent specification history. Disposal retires intent even in faults.
\* Publication and logging are direct effects, including after unmount; they
\* cannot disappear merely because React drops an unmounted setState call.
\* Positive exploration permits every input in every fault mode. The three
\* faults only remove their corresponding publication/error/cleanup guard.
\* A separate reachability witness requires visible current failure, accepted
\* recovery success and pending disposal. No React/native scheduler refinement,
\* fairness, liveness or unbounded-request claim is made.
=============================================================================
