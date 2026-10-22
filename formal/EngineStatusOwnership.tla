------------------------ MODULE EngineStatusOwnership ------------------------
EXTENDS Naturals, FiniteSets, Sequences
CONSTANTS Wrong, Scenario
VARIABLE s, lastEvent
vars == <<s, lastEvent>>
Ids == 1..6
Limit == IF Scenario = "progress" THEN 6 ELSE 3
Kinds == {"ready", "loading", "stopped", "missing", "failure"}
Text(id) == CASE id = 1 -> "1" [] id = 2 -> "2" [] id = 3 -> "3"
  [] id = 4 -> "4" [] id = 5 -> "5" [] OTHER -> "6"
InitialView == [stage |-> "checking", device |-> "none", gpu |-> "none",
  error |-> "none", polling |-> TRUE, failures |-> 0]
Snapshot(view, kind, id) ==
  [view EXCEPT
    !.stage = CASE kind = "ready" -> "ready" [] kind = "missing" -> "downloading"
      [] kind = "failure" -> "error" [] OTHER -> "loading",
    !.device = IF kind = "failure" THEN @ ELSE IF kind = "ready" THEN "cuda" ELSE "cpu",
    !.gpu = IF kind = "failure" THEN @ ELSE IF kind = "ready" THEN Text(id) ELSE "none",
    !.error = CASE kind = "failure" -> Text(id)
      [] kind \in {"ready", "missing"} -> "none" [] OTHER -> @,
    !.polling = IF kind \in {"ready", "failure"} THEN FALSE ELSE @,
    !.failures = IF kind = "ready" THEN 0 ELSE @]
StartResult(view, success) ==
  [view EXCEPT !.failures = IF success THEN 0 ELSE @ + 1,
    !.stage = IF ~success /\ view.failures = 2 THEN "error" ELSE @,
    !.error = IF ~success /\ view.failures = 2 THEN "start failed" ELSE @,
    !.polling = IF ~success /\ view.failures = 2 THEN FALSE ELSE @]
Visible(view) == <<view.stage, view.device, view.gpu, view.error, view.polling>>
Init ==
  /\ s = [issued |-> 0, owner |-> 0, intended |-> 0, pending |-> {},
    tokens |-> [id \in Ids |-> 0], epochs |-> [id \in Ids |-> 0],
    startOwner |-> 0, startIntent |-> 0,
    startTokens |-> [id \in Ids |-> 0], startEpochs |-> [id \in Ids |-> 0],
    startPending |-> {}, starts |-> <<>>, wantedStarts |-> <<>>, progressed |-> {},
    failuresDelivered |-> 0, loadingPollDuringStart |-> FALSE, lateLoadingDelivered |-> FALSE,
    downloads |-> <<>>, wantedDownloads |-> <<>>, downloading |-> FALSE,
    view |-> InitialView, wanted |-> InitialView, eventCount |-> 0,
    sawReady |-> FALSE, sawFailure |-> FALSE, recovered |-> FALSE]
  /\ lastEvent = "init"
Query ==
  /\ s.issued < Limit /\ s.view.polling /\ ~s.downloading
  /\ Scenario # "progress" \/ CASE s.issued = 0 -> TRUE
       [] s.issued % 2 = 1 -> s.issued \in s.progressed /\ s.issued \in s.startPending
       [] OTHER -> s.failuresDelivered = s.issued \div 2 /\ s.issued \notin s.pending
  /\ LET id == s.issued + 1
         token == s.owner + (IF Wrong = "query" THEN 0 ELSE 1)
     IN s' = [s EXCEPT !.issued = id, !.owner = token, !.intended = @ + 1,
       !.tokens[id] = token, !.epochs[id] = s.intended + 1,
       !.pending = @ \cup {id}]
  /\ lastEvent' = "engine-status:query:" \o Text(s.issued + 1)
StatusEvent(kind) ==
  /\ Scenario = "general" /\ s.issued > 0 /\ s.eventCount = 0
  /\ kind \in {"ready", "loading", "failure"}
  /\ LET next == [Snapshot(s.view, kind, 3) EXCEPT !.failures = s.view.failures,
           !.gpu = IF kind = "ready" THEN "event" ELSE @]
         wanted == [Snapshot(s.wanted, kind, 3) EXCEPT !.failures = s.wanted.failures,
           !.gpu = IF kind = "ready" THEN "event" ELSE @]
         terminal == kind # "loading"
     IN s' = [s EXCEPT !.owner = @ + (IF Wrong = "event" THEN 0 ELSE 1),
       !.intended = @ + 1, !.eventCount = 1, !.view = next, !.wanted = wanted,
       !.startOwner = @ + (IF terminal \/ Wrong = "progress" THEN 1 ELSE 0),
       !.startIntent = @ + (IF terminal THEN 1 ELSE 0),
       !.sawReady = @ \/ kind = "ready", !.sawFailure = @ \/ kind = "failure",
       !.recovered = @ \/ (s.sawFailure /\ kind = "ready")]
  /\ lastEvent' = "engine-status:event:" \o kind
Crash ==
  /\ Scenario = "general" /\ s.issued > 0 /\ s.issued < Limit
  /\ s.eventCount = 0 /\ ~s.downloading
  /\ LET id == s.issued + 1
         token == s.owner + (IF Wrong = "event" THEN 0 ELSE 1)
           + (IF Wrong = "query" THEN 0 ELSE 1)
         next == [s.view EXCEPT !.stage = "loading", !.error = "none",
           !.polling = TRUE, !.failures = 0]
         wanted == [s.wanted EXCEPT !.stage = "loading", !.error = "none",
           !.polling = TRUE, !.failures = 0]
     IN s' = [s EXCEPT !.issued = id, !.owner = token, !.intended = @ + 2,
       !.tokens[id] = token, !.epochs[id] = s.intended + 2,
       !.pending = @ \cup {id}, !.eventCount = 1, !.view = next, !.wanted = wanted,
       !.startOwner = @ + 1, !.startIntent = @ + 1]
  /\ lastEvent' = "engine-status:crashed:" \o Text(s.issued + 1)
Complete(id, kind) ==
  /\ id \in s.pending /\ kind \in Kinds
  /\ Scenario # "progress" \/ (kind = (IF id % 2 = 0 THEN "loading" ELSE "stopped")
       /\ (id # 6 \/ s.failuresDelivered = 3))
  /\ LET current == s.tokens[id] = s.owner
         intended == s.epochs[id] = s.intended
         beginStart == current /\ kind = "stopped"
         wantStart == intended /\ kind = "stopped"
         retireStart == current /\ kind \in {"ready", "failure", "missing", "stopped"}
         retireIntent == intended /\ kind \in {"ready", "failure", "missing", "stopped"}
         download == current /\ kind = "missing" /\ ~s.downloading
         wantDownload == intended /\ kind = "missing" /\ ~s.downloading
     IN s' = [s EXCEPT !.pending = @ \ {id},
       !.view = IF current THEN Snapshot(@, kind, id) ELSE @,
       !.wanted = IF intended THEN Snapshot(@, kind, id) ELSE @,
       !.starts = IF beginStart THEN Append(@, id) ELSE @,
       !.wantedStarts = IF wantStart THEN Append(@, id) ELSE @,
       !.startOwner = @ + (IF retireStart THEN 1 ELSE 0),
       !.startIntent = @ + (IF retireIntent THEN 1 ELSE 0),
       !.startTokens[id] = IF beginStart THEN s.startOwner + 1 ELSE @,
       !.startEpochs[id] = IF wantStart THEN s.startIntent + 1 ELSE @,
       !.startPending = IF beginStart THEN @ \cup {id} ELSE @,
       !.loadingPollDuringStart = @ \/ (intended /\ kind = "loading" /\
         \E start \in s.startPending: s.startEpochs[start] = s.startIntent),
       !.lateLoadingDelivered = @ \/ (Scenario = "progress" /\ id = 6),
       !.downloads = IF download THEN Append(@, id) ELSE @,
       !.wantedDownloads = IF wantDownload THEN Append(@, id) ELSE @,
       !.downloading = @ \/ download,
       !.sawReady = @ \/ (intended /\ kind = "ready"),
       !.sawFailure = @ \/ (intended /\ kind = "failure"),
       !.recovered = @ \/ (intended /\ kind = "ready" /\ s.sawFailure)]
  /\ lastEvent' = "engine-status:complete:" \o Text(id) \o ":" \o kind
StartProgress(id) ==
  /\ id \in s.startPending /\ id \notin s.progressed
  /\ s' = [s EXCEPT !.progressed = @ \cup {id},
    !.owner = @ + (IF Wrong = "event" THEN 0 ELSE 1), !.intended = @ + 1,
    !.startOwner = @ + (IF Wrong = "progress" THEN 1 ELSE 0),
    !.view.stage = "loading", !.wanted.stage = "loading"]
  /\ lastEvent' = "engine-status:progress:" \o Text(id)
CompleteStart(id, success) ==
  /\ id \in s.startPending
  /\ Scenario # "progress" \/ (~success /\ id \in s.progressed
       /\ (IF id = 5 THEN s.issued = 6 /\ 6 \in s.pending
           ELSE s.issued >= id + 1 /\ id + 1 \notin s.pending))
  /\ LET current == s.startTokens[id] = s.startOwner \/ (Wrong = "start" /\ ~success)
         intended == s.startEpochs[id] = s.startIntent
         terminal == current /\ ~success /\ s.view.failures = 2
         wantTerminal == intended /\ ~success /\ s.wanted.failures = 2
     IN s' = [s EXCEPT !.startPending = @ \ {id},
       !.failuresDelivered = @ + (IF success THEN 0 ELSE 1),
       !.view = IF current THEN StartResult(@, success) ELSE @,
       !.wanted = IF intended THEN StartResult(@, success) ELSE @,
       !.startOwner = @ + (IF terminal THEN 1 ELSE 0),
       !.startIntent = @ + (IF wantTerminal THEN 1 ELSE 0),
       !.owner = @ + (IF terminal THEN 1 ELSE 0),
       !.intended = @ + (IF wantTerminal THEN 1 ELSE 0)]
  /\ lastEvent' = "engine-status:start:" \o Text(id) \o ":" \o
       (IF success THEN "success" ELSE "failure")
Next == Query \/ Crash \/ (\E kind \in {"ready", "loading", "failure"}: StatusEvent(kind))
  \/ (\E id \in Ids, kind \in Kinds: Complete(id, kind))
  \/ (\E id \in Ids: StartProgress(id))
  \/ (\E id \in Ids, success \in BOOLEAN: CompleteStart(id, success))
Spec == Init /\ [][Next]_vars
StatusPublicationOwned == Visible(s.view) = Visible(s.wanted)
StartEffectsOwned == s.starts = s.wantedStarts
AutoDownloadOwned == s.downloads = s.wantedDownloads
PollingOwned == s.view.polling = s.wanted.polling
StartOutcomeOwned == <<s.view.stage, s.view.error, s.view.polling>>
  = <<s.wanted.stage, s.wanted.error, s.wanted.polling>>
ReadyFailureRecoveryNotReached == ~(s.sawReady /\ s.sawFailure /\ s.recovered)
ProgressFailureThresholdNotReached == ~(s.loadingPollDuringStart /\ {1, 3, 5} \subseteq s.progressed
  /\ s.wanted.failures = 3 /\ s.view.stage = "error" /\ ~s.view.polling /\ s.lateLoadingDelivered)
\* General bound: three queries, one external authoritative event, at most one
\* native loading progress event per start. Progress bound: six ordered queries
\* (stopped/loading pairs), three native progress events and three failed starts.
\* Each start spans a loading poll. The final poll completes after the third
\* failure, checking that terminal start error retires already pending snapshots.
\* Snapshot owner/intended and startOwner/startIntent are independent protocols.
\* Every query/event retires old snapshots. Loading events/polls preserve starts;
\* accepted terminal snapshots/events or a new actual invocation retire starts.
\* Pending obsolete completions stay enabled; issued native starts stay issued.
\* Faults remove query/event retirement or awaited-start catch guard, or wrongly
\* retire starts on loading progress. Ghost epochs never read implementation
\* tokens. Script constraints use delivered inputs, never ownership outcomes.
\* Missing status folds its first zero-delay auto-download dispatch; native
\* download stays pending. Intermediate need_download is abstracted.
\* Restart threshold ten, download generation/cancellation, unmount, other
\* model-state branches, native scheduler, unbounded requests, refinement and
\* liveness are excluded. Both separate witnesses assert reachability only.
=============================================================================
