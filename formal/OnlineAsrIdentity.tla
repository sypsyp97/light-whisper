-------------------------- MODULE OnlineAsrIdentity --------------------------
EXTENDS Naturals, Sequences
CONSTANTS Wrong, StartEngine
VARIABLE s, lastEvent
vars == <<s, lastEvent>>
Ids == 0..2
Name(i) == CASE i = 0 -> "0" [] i = 1 -> "1" [] OTHER -> "2"
Region(engine) == IF engine = "alibaba" THEN "domestic" ELSE "international"
Slot(engine, region) == CASE engine = "alibaba" ->
  IF region = "domestic" THEN "cn" ELSE "intl"
  [] engine = "glm" -> "glm" [] OTHER -> "none"
Read(engine, context, keyVersion, engineState, endpointState, keyState) ==
  [engine |-> engine, region |-> Region(engine), context |-> context, keyVersion |-> keyVersion,
   engineState |-> engineState, endpointState |-> endpointState, keyState |-> keyState]
EmptyRead == Read("local", 3, 0, "absent", "absent", "absent")
NoErrors == [engine |-> FALSE, endpoint |-> FALSE, key |-> FALSE]
Init ==
  /\ s = [epoch |-> 0, batch |-> 0, switchBatch |-> 0, backend |-> StartEngine, engine |-> "local",
    knownEngine |-> FALSE, knownEndpoint |-> FALSE, region |-> "international",
    endpointOwner |-> 3, truthEngine |-> FALSE, truthEndpoint |-> FALSE,
    reads |-> [i \in Ids |-> IF i = 0
      THEN Read(StartEngine, 0, 0, "pending", "pending", "pending") ELSE EmptyRead],
    phase |-> "idle", switched |-> FALSE, retried |-> FALSE, committed |-> FALSE,
    errors |-> NoErrors, keyVersion |-> 0, edits |-> 0, draft |-> "none", draftSlot |-> "none",
    expectedSlot |-> "none", inFlight |-> FALSE, writes |-> <<>>, expectedWrites |-> <<>>,
    flushFailed |-> FALSE, overlap |-> FALSE, staleSeen |-> FALSE,
    retryHadEndpoint |-> FALSE, lateSibling |-> FALSE]
  /\ lastEvent = "init"
ExpectedReady == s.truthEngine /\ (s.backend = "glm" \/
  (s.backend = "alibaba" /\ s.truthEndpoint))
ConfigError == s.errors.engine \/ s.errors.endpoint \/ s.errors.key
Current(i) == s.reads[i].context = s.epoch
Publishes(i) == Current(i) /\
  (Wrong # "retry-retires-pending" \/ ~s.retried \/ i = s.batch)
KeyReady == s.knownEngine /\ (s.engine = "glm" \/
  (s.engine = "alibaba" /\ (s.knownEndpoint \/
    (Wrong = "initial-ready" /\ ~s.switched) \/
    (Wrong = "switch-failure-ready" /\ s.committed /\ ConfigError))))
SwitchReadSettled == s.phase = "reading" /\
  (s.reads[s.switchBatch].endpointState = "failure" \/
   s.reads[s.switchBatch].keyState = "failure" \/
   (s.reads[s.switchBatch].endpointState = "success" /\
    s.reads[s.switchBatch].keyState = "success"))
Outcome(ok) == IF ok THEN "success" ELSE "failure"
CompleteEngine(i, ok) ==
  /\ ~SwitchReadSettled
  /\ s.reads[i].engineState = "pending"
  /\ s' = [s EXCEPT !.reads[i].engineState = Outcome(ok),
    !.engine = IF Publishes(i) /\ ok THEN s.reads[i].engine ELSE @,
    !.knownEngine = @ \/ (Publishes(i) /\ ok),
    !.truthEngine = @ \/ (Current(i) /\ ok),
    !.errors.engine = @ \/ (Publishes(i) /\ ~ok)]
  /\ lastEvent' = "engine:" \o Name(i) \o ":" \o Outcome(ok)
CompleteEndpoint(i, ok) ==
  /\ ~SwitchReadSettled
  /\ s.reads[i].endpointState = "pending"
  /\ LET publish == Publishes(i) \/ Wrong = "stale-epoch" IN
     s' = [s EXCEPT !.reads[i].endpointState = Outcome(ok),
       !.knownEndpoint = @ \/ (publish /\ ok),
       !.region = IF publish /\ ok THEN s.reads[i].region ELSE @,
       !.endpointOwner = IF publish /\ ok THEN s.reads[i].context ELSE @,
       !.truthEndpoint = @ \/ (Current(i) /\ ok),
       !.errors.endpoint = @ \/ (publish /\ ~ok),
       !.staleSeen = @ \/ ~Current(i),
       !.lateSibling = @ \/ (s.retried /\ i < s.batch /\ Current(i) /\ ok)]
  /\ lastEvent' = "endpoint:" \o Name(i) \o ":" \o Outcome(ok)
CompleteKey(i, ok) ==
  /\ ~SwitchReadSettled
  /\ s.reads[i].keyState = "pending"
  /\ s' = [s EXCEPT !.reads[i].keyState = Outcome(ok),
    !.knownEndpoint = IF Wrong = "key-failure" /\ s.committed /\ Current(i) /\ ~ok
      THEN FALSE ELSE @,
    !.errors.key = @ \/ (Publishes(i) /\ s.reads[i].keyVersion = s.keyVersion /\ ~ok)]
  /\ lastEvent' = "key:" \o Name(i) \o ":" \o Outcome(ok)
Edit(value) ==
  /\ ~SwitchReadSettled
  /\ s.edits < 2
  /\ LET accept == KeyReady /\ s.phase = "idle" IN
     s' = [s EXCEPT !.edits = @ + 1,
       !.keyVersion = IF accept THEN @ + 1 ELSE @,
       !.errors.key = IF accept THEN FALSE ELSE @,
       !.draft = IF accept THEN value ELSE @,
       !.draftSlot = IF accept THEN Slot(s.engine, s.region) ELSE @,
       !.expectedSlot = IF accept THEN
         IF ExpectedReady THEN Slot(s.backend, Region(s.backend)) ELSE "none" ELSE @,
       !.overlap = @ \/ s.phase # "idle"]
  /\ lastEvent' = "edit:" \o value
Dispatch ==
  /\ ~SwitchReadSettled
  /\ s.draft # "none" /\ ~s.inFlight
  /\ s' = [s EXCEPT !.draft = "none", !.inFlight = TRUE,
    !.writes = Append(@, <<s.draftSlot, s.draft>>),
    !.expectedWrites = Append(@, <<s.expectedSlot, s.draft>>)]
  /\ lastEvent' = "timer:600"
CompleteWrite(ok) ==
  /\ ~SwitchReadSettled
  /\ s.inFlight
  /\ s' = [s EXCEPT !.inFlight = FALSE,
    !.flushFailed = IF s.phase = "flush" /\ ~ok THEN TRUE ELSE @,
    !.phase = IF s.phase = "flush" /\ ~ok THEN "idle" ELSE @]
  /\ lastEvent' = "write:" \o Outcome(ok)
BeginSwitch ==
  /\ s.knownEngine /\ s.phase = "idle" /\ ~s.switched
  /\ s.backend # "alibaba" /\ s.batch < 2
  /\ ~s.inFlight \/ s.draft = "none"
  /\ s' = [s EXCEPT !.phase = "flush", !.switched = TRUE,
    !.draft = IF ~s.inFlight THEN "none" ELSE @,
    !.inFlight = @ \/ s.draft # "none",
    !.writes = IF s.draft # "none" /\ ~s.inFlight
      THEN Append(@, <<s.draftSlot, s.draft>>) ELSE @,
    !.expectedWrites = IF s.draft # "none" /\ ~s.inFlight
      THEN Append(@, <<s.expectedSlot, s.draft>>) ELSE @]
  /\ lastEvent' = "switch:begin"
CompleteSwitch(ok) ==
  /\ s.phase = "flush" /\ ~s.inFlight /\ s.draft = "none" /\ ~s.flushFailed
  /\ s' = IF ok THEN [s EXCEPT !.epoch = @ + 1, !.batch = @ + 1,
      !.switchBatch = s.batch + 1, !.backend = "alibaba",
      !.engine = "alibaba", !.knownEngine = TRUE, !.truthEngine = TRUE,
      !.knownEndpoint = FALSE, !.truthEndpoint = FALSE, !.errors = NoErrors,
      !.phase = "reading", !.committed = TRUE, !.lateSibling = FALSE,
      !.reads[s.batch + 1] = Read("alibaba", s.epoch + 1, s.keyVersion,
        "absent", "pending", "pending")]
    ELSE [s EXCEPT !.phase = "idle"]
  /\ lastEvent' = "switch:" \o Outcome(ok)
FinishSwitchRead ==
  /\ SwitchReadSettled
  /\ s' = [s EXCEPT !.phase = "idle"]
  /\ lastEvent' = "switch:settled"
Retry ==
  /\ ConfigError /\ s.phase = "idle" /\ ~s.retried /\ s.batch < 2
  /\ s' = [s EXCEPT !.batch = @ + 1, !.retried = TRUE,
    !.retryHadEndpoint = s.truthEndpoint, !.errors = NoErrors,
    !.reads[s.batch + 1] = Read(s.backend, s.epoch, s.keyVersion,
      IF s.errors.engine THEN "pending" ELSE "absent",
      IF s.errors.endpoint THEN "pending" ELSE "absent",
      IF s.errors.key THEN "pending" ELSE "absent")]
  /\ lastEvent' = "retry:begin"
Next == (\E i \in Ids, ok \in BOOLEAN:
    CompleteEngine(i, ok) \/ CompleteEndpoint(i, ok) \/ CompleteKey(i, ok))
  \/ (\E value \in {"draft", "clear"}: Edit(value))
  \/ Dispatch \/ BeginSwitch \/ FinishSwitchRead \/ Retry
  \/ (\E ok \in BOOLEAN: CompleteWrite(ok) \/ CompleteSwitch(ok))
Spec == Init /\ [][Next]_vars
WritesRequireKnownIdentity == /\ s.writes = s.expectedWrites
  /\ \A i \in 1..Len(s.expectedWrites): s.expectedWrites[i][1] # "none"
EndpointPublicationOwned == s.knownEndpoint => s.endpointOwner = s.epoch
KnownEndpointRetained == s.truthEndpoint => s.knownEndpoint /\
  s.region = Region(s.backend)
SharedGlmSlotAvailable == s.truthEngine /\ s.backend = "glm" => KeyReady
LocalHasNoCredentialSlot == s.knownEngine /\ s.engine = "local" => ~KeyReady
CommittedEngineRetained == s.committed /\ s.knownEngine => s.engine = "alibaba"
ConfirmedEngineRetained == s.truthEngine => s.knownEngine /\ s.engine = s.backend
NoRetry == ~s.retried
RecoveryNotReached == ~(s.retried /\ s.retryHadEndpoint /\
  s.reads[0].engineState = "failure" /\ ExpectedReady /\ Len(s.writes) > 0)
FlushNotReached == ~(s.committed /\ s.overlap /\ Len(s.writes) > 0)
SiblingNotReached == ~(s.committed /\ s.retried /\ s.lateSibling /\
  ExpectedReady /\ (\E i \in 1..Len(s.writes): s.writes[i][1] = "cn"))
\* Explicit bound: three read batches, two configuration contexts (only engine
\* mutation changes context), <=one engine switch and read-only retry,
\* two edit/clear attempts, <=two dispatched writes, GLM/local/Alibaba engines,
\* representative Alibaba-domestic/GLM-international region combinations.
\* Read success/failure and completion order
\* are environmental inputs. Reads are not cancelled; every old pending read
\* remains enabled. Retry issues only ERROR fields; READY/PENDING siblings
\* retain their owner and may publish after retry. Key read errors are recoverable
\* without invalidating identity; accepted user edits retire old key results and
\* clear the key error. Aggregate switch reads settle on first failure or both
\* successes; the sibling remains live. FinishSwitchRead is the drained promise
\* continuation, before the next public action. Writes record invocation intent
\* separately from completion.
\* Timer 600 abstracts time; real-hook replay checks 599/600 ms. Switch flush
\* starts before mutation; its rejection aborts mutation. A switch with both
\* an in-flight save and a second pending draft is excluded (unit tests own
\* that debouncer queue). Region mutation, catalogs, unmount,
\* backend storage internals and unrelated windows are outside this companion.
=============================================================================
