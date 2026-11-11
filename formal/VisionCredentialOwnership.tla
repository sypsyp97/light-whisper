----------------------- MODULE VisionCredentialOwnership --------------------
EXTENDS Naturals, FiniteSets
CONSTANT Wrong
Ids == 1..2
Providers == {"cerebras", "openai"}
VARIABLE s
Init == s = [provider |-> "cerebras", switches |-> 0, edits |-> 0, reads |-> 0,
  latestRead |-> 0, draft |-> [p \in Providers |-> 0],
  disk |-> [p \in Providers |-> 0], owner |-> [i \in Ids |-> "cerebras"],
  phase |-> [i \in Ids |-> "unused"], retried |-> {},
  readPhase |-> [i \in Ids |-> "unused"],
  readProvider |-> [i \in Ids |-> "cerebras"],
  readSwitch |-> [i \in Ids |-> 0], readEdit |-> [i \in Ids |-> 0],
  readDirty |-> [i \in Ids |-> FALSE],
  badRead |-> FALSE, lostDraft |-> FALSE, wrongProvider |-> FALSE]
Edit(i) ==
  /\ i = s.edits + 1 /\ i \in Ids
  /\ s' = [s EXCEPT !.edits = i, !.draft[s.provider] = i,
    !.owner[i] = s.provider, !.phase[i] = "debounced"]
Switch ==
  /\ s.switches < 2
  /\ LET pending == {i \in Ids: s.phase[i] = "debounced"}
     IN s' = [s EXCEPT !.provider = IF @ = "cerebras" THEN "openai" ELSE "cerebras",
       !.switches = @ + 1,
       !.phase = [i \in Ids |-> IF i \in pending
         THEN IF Wrong = "drop-on-switch" THEN "dropped" ELSE "queued" ELSE s.phase[i]],
       !.lostDraft = @ \/ (Wrong = "drop-on-switch" /\ pending # {})]
Dispatch(i) ==
  /\ s.phase[i] = "debounced"
  /\ s' = [s EXCEPT !.phase[i] = "queued"]
BeginWrite(i) ==
  /\ s.phase[i] = "queued"
  /\ Wrong = "overlap" \/ \A j \in Ids: s.phase[j] # "pending"
  /\ \A j \in Ids: j < i => s.phase[j] \notin {"debounced", "queued"}
  /\ s' = [s EXCEPT !.phase[i] = "pending"]
Save(i, success) ==
  /\ s.phase[i] = "pending" /\ success \in BOOLEAN
  /\ LET target == IF Wrong = "current-provider" THEN s.provider ELSE s.owner[i]
     IN s' = [s EXCEPT !.phase[i] = IF success THEN "saved" ELSE "failed",
       !.disk[target] = IF success THEN i ELSE @,
       !.draft[s.owner[i]] = IF success /\ @ = i THEN 0 ELSE @,
       !.wrongProvider = @ \/ (success /\ target # s.owner[i])]
Retry(i) ==
  /\ s.phase[i] = "failed" /\ s.draft[s.owner[i]] = i /\ i \notin s.retried
  /\ s' = [s EXCEPT !.phase[i] = "queued", !.retried = @ \cup {i}]
ReadBegin(i) ==
  /\ i = s.reads + 1 /\ i \in Ids
  /\ s' = [s EXCEPT !.reads = i, !.latestRead = i, !.readPhase[i] = "pending",
    !.readProvider[i] = s.provider, !.readSwitch[i] = s.switches,
    !.readEdit[i] = s.edits, !.readDirty[i] = s.draft[s.provider] # 0]
ReadOwnsView(i) == i = s.latestRead /\ s.readProvider[i] = s.provider
  /\ s.readSwitch[i] = s.switches /\ s.readEdit[i] = s.edits
  /\ ~s.readDirty[i] /\ s.draft[s.provider] = 0
ReadComplete(i) ==
  /\ s.readPhase[i] = "pending"
  /\ LET accept == IF Wrong = "read-edit" THEN i = s.latestRead
         ELSE IF Wrong = "read-dirty-snapshot" THEN i = s.latestRead
           /\ s.readProvider[i] = s.provider /\ s.readSwitch[i] = s.switches
           /\ s.readEdit[i] = s.edits /\ s.draft[s.provider] = 0
         ELSE ReadOwnsView(i)
     IN s' = [s EXCEPT !.readPhase[i] = "done", !.badRead = @ \/ (accept /\ ~ReadOwnsView(i))]
Next == Switch \/ (\E i \in Ids: Edit(i) \/ Dispatch(i) \/ BeginWrite(i)
  \/ Retry(i) \/ ReadBegin(i) \/ ReadComplete(i) \/ (\E ok \in BOOLEAN: Save(i, ok)))
Spec == Init /\ [][Next]_s
OnlyOwnedReadPublishes == ~s.badRead
ProviderSwitchPreservesDraft == ~s.lostDraft
CapturedProviderReceivesKey == ~s.wrongProvider
StorageWritesAreSerial == Cardinality({i \in Ids: s.phase[i] = "pending"}) <= 1
=============================================================================
