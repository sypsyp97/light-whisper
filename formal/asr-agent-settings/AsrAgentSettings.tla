------------------------- MODULE AsrAgentSettings -------------------------
EXTENDS Integers, TLC
CONSTANT Wrong
VARIABLE s
vars == <<s>>
Ids == {1, 2}
Owners == {1, 2}

Init == s = [active |-> TRUE, mount |-> 1, epoch |-> 0, events |-> 0,
  backend |-> 0, displayed |-> 0, endpoint |-> 0, key |-> 0, keyVersion |-> 0,
  engineKnown |-> TRUE, endpointKnown |-> TRUE, engineLoading |-> FALSE, loadingOwner |-> 0,
  edited |-> FALSE, editEpoch |-> -1, ui |-> "unused", uiEpoch |-> -1,
  uiKind |-> "none", uiMount |-> 0, uiFinalized |-> FALSE, badPersistence |-> FALSE,
  subscriptions |-> [m \in Owners |-> IF m = 1 THEN "pending" ELSE "unused"],
  warning |-> FALSE, readError |-> FALSE, badPublication |-> FALSE,
  foreignCallback |-> FALSE, readMount |-> [i \in Ids |-> 0],
  readEpoch |-> [i \in Ids |-> -1], readKeyVersion |-> [i \in Ids |-> 0],
  engineDone |-> [i \in Ids |-> FALSE], endpointDone |-> [i \in Ids |-> FALSE],
  keyDone |-> [i \in Ids |-> FALSE],
  requested |-> [i \in Ids |-> FALSE], failed |-> [i \in Ids |-> FALSE],
  refreshed |-> [i \in Ids |-> FALSE]]

ListenerOwns(m) == s.active /\ m = s.mount
ReadOwns(i) == s.active /\ s.readMount[i] = s.mount /\ s.readEpoch[i] = s.epoch
ReadMayPublish(i) ==
  IF Wrong = "read-epoch" THEN s.active /\ s.readMount[i] = s.mount
  ELSE IF Wrong = "mounted" THEN s.readMount[i] = s.mount /\ s.readEpoch[i] = s.epoch
  ELSE ReadOwns(i)
UIOwns == s.active /\ s.uiMount = s.mount /\ s.uiEpoch = s.epoch
UIMayPublish ==
  IF Wrong = "ui-epoch" THEN s.active /\ s.uiMount = s.mount
  ELSE IF Wrong = "mounted" THEN s.uiMount = s.mount /\ s.uiEpoch = s.epoch
  ELSE UIOwns
FlushMayPersist == IF Wrong = "flush" THEN TRUE ELSE UIOwns
UIReadMayPublish == IF Wrong = "ui-read" THEN s.active /\ s.uiMount = s.mount ELSE UIMayPublish
FinallyMayClear == Wrong = "finally" \/ UIOwns
Alibaba(value) == value \in {0, 2}
KeyReady == s.engineKnown /\ (~Alibaba(s.displayed) \/ s.endpointKnown)
EndpointReady == s.engineKnown /\ s.endpointKnown

ResolveSubscription(m, ok) ==
  /\ s.subscriptions[m] = "pending"
  /\ s' = [s EXCEPT !.subscriptions[m] =
       IF ~ok THEN "failed"
       ELSE IF ListenerOwns(m) \/ Wrong = "cleanup" THEN "installed" ELSE "disposed",
     !.warning = IF ~ok /\ ListenerOwns(m) /\ Wrong # "diagnostic" THEN TRUE ELSE @]

Unmount ==
  /\ s.active
  /\ s' = [s EXCEPT !.active = FALSE,
     !.subscriptions[s.mount] = IF @ = "installed" /\ Wrong # "cleanup" THEN "disposed" ELSE @]

Remount ==
  /\ ~s.active /\ s.mount = 1 /\ s.ui = "unused" /\ s.events = 0
  /\ s' = [s EXCEPT !.active = TRUE, !.mount = 2, !.epoch = @ + 1,
     !.subscriptions[2] = "pending", !.warning = FALSE, !.readError = FALSE,
     !.edited = FALSE, !.engineKnown = TRUE, !.endpointKnown = TRUE,
     !.engineLoading = FALSE, !.loadingOwner = 0]

Event(i, m) ==
  /\ i = s.events + 1 /\ i \in Ids /\ m <= s.mount
  /\ s.subscriptions[m] \in {"pending", "installed", "disposed"}
  /\ LET accept == ListenerOwns(m) \/ (Wrong = "listener-owner" /\ s.active)
     IN s' = [s EXCEPT !.events = i, !.backend = i,
       !.epoch = IF accept THEN @ + 1 ELSE @,
       !.requested[i] = accept,
       !.readMount[i] = IF accept THEN s.mount ELSE @,
       !.readEpoch[i] = IF accept THEN s.epoch + 1 ELSE @,
       !.readKeyVersion[i] = s.keyVersion,
       !.engineLoading = IF accept THEN TRUE ELSE @,
       !.loadingOwner = IF accept THEN i ELSE @,
       !.engineKnown = IF accept /\ Wrong # "readiness" THEN FALSE ELSE @,
       !.endpointKnown = IF accept /\ Wrong # "readiness" THEN FALSE ELSE @,
       !.edited = IF accept THEN FALSE ELSE @,
       !.readError = IF accept THEN FALSE ELSE @,
       !.foreignCallback = @ \/ (accept /\ ~ListenerOwns(m))]

BeginUI(kind) ==
  /\ s.active /\ s.mount = 1 /\ s.ui = "unused" /\ s.events = 0
  /\ kind \in {"engine", "region"}
  /\ s' = [s EXCEPT !.ui = "flushing", !.uiKind = kind,
     !.uiEpoch = s.epoch, !.uiMount = s.mount,
     !.engineLoading = IF kind = "engine" THEN TRUE ELSE @,
     !.loadingOwner = IF kind = "engine" THEN 3 ELSE @]

CompleteFlush ==
  /\ s.ui = "flushing"
  /\ s' = [s EXCEPT !.ui = IF FlushMayPersist THEN "persisting" ELSE "done",
     !.backend = IF FlushMayPersist THEN 3 ELSE @,
     !.badPersistence = @ \/ (FlushMayPersist /\ ~UIOwns)]

CompleteUI(ok) ==
  /\ s.ui = "persisting"
  /\ LET commit == UIMayPublish /\ ok
     IN s' = [s EXCEPT !.ui = IF commit THEN "reading" ELSE "done",
       !.epoch = IF commit THEN @ + 1 ELSE @,
       !.uiEpoch = IF commit THEN s.epoch + 1 ELSE @,
       !.displayed = IF commit /\ s.uiKind = "engine" THEN 3 ELSE @,
       !.endpoint = IF commit /\ s.uiKind = "region" THEN 3 ELSE @,
       !.engineKnown = IF commit THEN TRUE ELSE @,
       !.endpointKnown = IF commit THEN s.uiKind = "region" ELSE @,
       !.key = IF commit THEN 0 ELSE @,
       !.readError = IF commit THEN FALSE ELSE @,
       !.badPublication = @ \/ (UIMayPublish /\ ~UIOwns)]

CompleteUIRead(fail) ==
  /\ s.ui = "reading"
  /\ s' = [s EXCEPT !.ui = "done",
     !.key = IF UIReadMayPublish /\ ~fail THEN 3 ELSE @,
     !.endpoint = IF UIReadMayPublish /\ ~fail THEN 3 ELSE @,
     !.endpointKnown = IF UIReadMayPublish /\ ~fail THEN TRUE ELSE @,
     !.readError = IF UIReadMayPublish /\ fail THEN TRUE ELSE @,
     !.badPublication = @ \/ (UIReadMayPublish /\ ~UIOwns)]

FinalizeUI ==
  /\ s.ui = "done" /\ ~s.uiFinalized
  /\ s' = [s EXCEPT !.uiFinalized = TRUE,
     !.engineLoading = IF FinallyMayClear THEN FALSE ELSE @,
     !.loadingOwner = IF FinallyMayClear THEN 0 ELSE @]

CompleteEngine(i, fail) ==
  /\ s.requested[i] /\ ~s.engineDone[i]
  /\ s' = [s EXCEPT !.engineDone[i] = TRUE, !.failed[i] = fail,
     !.displayed = IF ReadMayPublish(i) /\ ~fail THEN i ELSE @,
     !.engineKnown = IF ReadMayPublish(i) /\ ~fail THEN TRUE ELSE @,
     !.engineLoading = IF ReadMayPublish(i) THEN FALSE ELSE @,
     !.loadingOwner = IF ReadMayPublish(i) THEN 0 ELSE @,
     !.readError = IF ReadMayPublish(i) /\ fail /\ Wrong # "read-error" THEN TRUE ELSE @,
     !.badPublication = @ \/ (ReadMayPublish(i) /\ ~ReadOwns(i))]

CompleteEndpoint(i) ==
  /\ s.requested[i] /\ ~s.endpointDone[i]
  /\ s' = [s EXCEPT !.endpointDone[i] = TRUE,
     !.endpoint = IF ReadMayPublish(i) THEN i ELSE @,
     !.endpointKnown = IF ReadMayPublish(i) THEN TRUE ELSE @,
     !.badPublication = @ \/ (ReadMayPublish(i) /\ ~ReadOwns(i))]

CompleteKey(i) ==
  /\ s.requested[i] /\ ~s.keyDone[i]
  /\ LET publish == ReadMayPublish(i) /\
       (s.readKeyVersion[i] = s.keyVersion \/ Wrong = "key-version")
     IN s' = [s EXCEPT !.keyDone[i] = TRUE, !.key = IF publish THEN i ELSE @,
       !.badPublication = @ \/ (publish /\ ~ReadOwns(i))]

EditKey ==
  /\ s.active /\ ~s.edited /\ KeyReady
  /\ (s.ui \in {"unused", "done"} \/ ~UIOwns)
  /\ \E i \in Ids: ReadOwns(i) /\ s.engineDone[i] /\ ~s.failed[i] /\ ~s.keyDone[i]
  /\ s' = [s EXCEPT !.key = 3, !.keyVersion = @ + 1, !.edited = TRUE, !.editEpoch = s.epoch]

Refresh(i) ==
  /\ s.requested[i] /\ s.engineDone[i] /\ s.endpointDone[i] /\ s.keyDone[i] /\ ~s.refreshed[i]
  /\ ReadOwns(i) /\ Wrong # "progress"
  /\ s' = [s EXCEPT !.refreshed[i] = TRUE]

Next == Unmount \/ Remount \/ CompleteFlush \/ FinalizeUI \/ EditKey
  \/ (\E kind \in {"engine", "region"}: BeginUI(kind))
  \/ (\E outcome \in BOOLEAN: CompleteUI(outcome) \/ CompleteUIRead(outcome))
  \/ UNCHANGED s
  \/ (\E m \in Owners, ok \in BOOLEAN: ResolveSubscription(m, ok))
  \/ (\E i \in Ids, m \in Owners: Event(i, m))
  \/ (\E i \in Ids: CompleteKey(i) \/ CompleteEndpoint(i) \/ Refresh(i)
      \/ (\E fail \in BOOLEAN: CompleteEngine(i, fail)))

Spec == Init /\ [][Next]_vars
  /\ (\A m \in Owners: WF_vars(\E ok \in BOOLEAN: ResolveSubscription(m, ok)))
  /\ (\A i \in Ids: WF_vars(\E fail \in BOOLEAN: CompleteEngine(i, fail)))
  /\ (\A i \in Ids: WF_vars(CompleteKey(i)) /\ WF_vars(CompleteEndpoint(i)) /\ WF_vars(Refresh(i)))
  /\ WF_vars(CompleteFlush)
  /\ WF_vars(FinalizeUI)
  /\ WF_vars(\E outcome \in BOOLEAN: CompleteUI(outcome))
  /\ WF_vars(\E outcome \in BOOLEAN: CompleteUIRead(outcome))

TypeOK == /\ s.active \in BOOLEAN /\ s.mount \in Owners /\ s.events \in 0..2
  /\ s.epoch \in 0..3 /\ s.displayed \in 0..3 /\ s.key \in 0..3
  /\ s.ui \in {"unused", "flushing", "persisting", "reading", "done"}
  /\ s.uiKind \in {"none", "engine", "region"}
  /\ s.engineLoading \in BOOLEAN /\ s.loadingOwner \in 0..3
  /\ \A m \in Owners: s.subscriptions[m] \in {"unused", "pending", "installed", "failed", "disposed"}
NoStalePublication == ~s.badPublication
NoObsoletePersistence == ~s.badPersistence
LatestReadKeepsLoading == \A i \in Ids:
  ReadOwns(i) /\ ~s.engineDone[i] => s.engineLoading /\ s.loadingOwner = i
UnknownIdentityBlocksCredentials == \A i \in Ids: ReadOwns(i) =>
  /\ (~s.engineDone[i] => ~KeyReady /\ ~EndpointReady)
  /\ (~s.endpointDone[i] => ~s.endpointKnown)
OnlyCurrentListenerReads == ~s.foreignCallback
RetiredListenerReleased == \A m \in Owners:
  s.subscriptions[m] = "installed" => ListenerOwns(m)
SubscriptionFailureVisible == s.active /\ s.subscriptions[s.mount] = "failed" => s.warning
CurrentReadFailureVisible == \A i \in Ids:
  ReadOwns(i) /\ s.engineDone[i] /\ s.failed[i] => s.readError
UserDraftSurvives == s.edited /\ s.editEpoch = s.epoch => s.key = 3
LatestReadRefreshes == \A i \in Ids:
  (s.requested[i] /\ s.engineDone[i] /\ s.endpointDone[i] /\ s.keyDone[i] /\ ReadOwns(i))
    ~> (s.refreshed[i] \/ ~ReadOwns(i))
RetiredSubscriptionSettles == \A m \in Owners:
  (~ListenerOwns(m) /\ s.subscriptions[m] = "pending")
    ~> (s.subscriptions[m] \in {"disposed", "failed"})

TwoReadsCompeteNotReached == ~(s.requested[1] /\ s.requested[2] /\
  s.engineDone[2] /\ ~s.engineDone[1] /\ s.ui = "persisting")
LateListenerNotReached == ~(s.mount = 2 /\ s.subscriptions[1] = "disposed" /\ s.active)
DraftOverlapNotReached == ~(s.edited /\ \E i \in Ids: ReadOwns(i) /\ ~s.keyDone[i])
RegionMutationOverlapNotReached == ~(s.uiKind = "region" /\ s.ui = "reading" /\ s.events > 0)
===========================================================================
