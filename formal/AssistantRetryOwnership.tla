------------------------ MODULE AssistantRetryOwnership ----------------------
EXTENDS Naturals, Sequences
CONSTANT Wrong
VARIABLE s, lastEvent
vars == <<s, lastEvent>>
Ids == {1, 2}
Owns(i) == s.requestSession[i] = s.session /\ s.requestGeneration[i] = s.generation
  /\ s.busyOwner = i
CanPublish(i) == IF Wrong = "legacy" THEN TRUE
  ELSE IF Wrong = "session-only" THEN s.requestSession[i] = s.session
  ELSE Owns(i)
Init ==
  /\ s = [session |-> 1, generation |-> 1, phase |-> "result", text |-> "initial",
    closed |-> FALSE, dictation |-> FALSE, reopened |-> FALSE, streamed |-> FALSE,
    busy |-> FALSE, busyOwner |-> 0, started |-> 0,
    requestState |-> [i \in Ids |-> "unused"],
    requestSession |-> [i \in Ids |-> 0], requestGeneration |-> [i \in Ids |-> 0]]
  /\ lastEvent = "init"
BeginRetry(i) ==
  /\ i = s.started + 1 /\ i \in Ids /\ ~s.busy /\ ~s.closed /\ ~s.dictation
  /\ s.phase = "result" /\ (i = 2 => s.reopened)
  /\ s' = [s EXCEPT !.started = i, !.requestState[i] = "pending",
    !.requestSession[i] = s.session, !.requestGeneration[i] = s.generation,
    !.busy = TRUE, !.busyOwner = i, !.text = "", !.phase = "generating"]
  /\ lastEvent' = IF i = 1 THEN "retry:1:begin" ELSE "retry:2:begin"
StreamFinal ==
  /\ s.busy /\ ~s.closed /\ ~s.dictation /\ ~s.streamed
  /\ s' = [s EXCEPT !.phase = "result", !.text = "current-stream", !.streamed = TRUE]
  /\ lastEvent' = "stream:final"
NewDictation ==
  /\ s.started = 1 /\ s.requestState[1] = "pending" /\ s.generation = 1
  /\ s' = [s EXCEPT !.session = 2, !.generation = 2,
    !.phase = "recording", !.text = "dictation-preview", !.dictation = TRUE,
    !.busy = FALSE, !.busyOwner = 0]
  /\ lastEvent' = "recording:new-session"
Close ==
  /\ s.started = 1 /\ s.requestState[1] = "pending" /\ s.generation = 1
  /\ s.phase = "result"
  /\ s' = [s EXCEPT !.generation = 2, !.closed = TRUE,
    !.phase = "idle", !.text = "", !.busy = FALSE, !.busyOwner = 0]
  /\ lastEvent' = "overlay:close:same-session"
Reopen ==
  /\ s.closed /\ ~s.reopened
  /\ s' = [s EXCEPT !.closed = FALSE, !.reopened = TRUE, !.streamed = FALSE,
    !.text = "initial", !.phase = "result"]
  /\ lastEvent' = "assistant:reopen:same-session"
Complete(i, success) ==
  /\ s.requestState[i] = "pending"
  /\ LET publish == CanPublish(i)
         finish == IF Wrong \in {"legacy", "finally"} THEN TRUE ELSE CanPublish(i)
     IN s' = [s EXCEPT !.requestState[i] = IF success THEN "success" ELSE "failure",
       !.text = IF publish /\ success THEN (IF i = 1 THEN "retry-1" ELSE "retry-2") ELSE @,
       !.phase = IF publish THEN "result" ELSE @,
       !.busy = IF finish THEN FALSE ELSE @,
       !.busyOwner = IF finish THEN 0 ELSE @]
  /\ lastEvent' = (IF i = 1 THEN "retry:1:" ELSE "retry:2:")
    \o (IF success THEN "success" ELSE "failure")
Next == NewDictation \/ Close \/ Reopen \/ StreamFinal
  \/ (\E i \in Ids: BeginRetry(i) \/ (\E success \in BOOLEAN: Complete(i, success)))
Spec == Init /\ [][Next]_vars
NewDictationKeepsItsDisplay == s.dictation =>
  s.phase = "recording" /\ s.text = "dictation-preview" /\ ~s.busy
ClosedOverlayStaysEmpty == s.closed => s.phase = "idle" /\ s.text = "" /\ ~s.busy
CurrentRetryKeepsBusy == s.requestState[2] = "pending" => s.busy /\ s.busyOwner = 2
CurrentStreamSurvivesOldCompletion == s.streamed /\ s.requestState[2] = "pending" =>
  s.phase = "result" /\ s.text = "current-stream"
OwnedRetrySettles == s.requestState[2] \in {"success", "failure"} => ~s.busy
\* BeginRetry = handleRetryAssistant's captured session/generation and busy flag.
\* NewDictation = prepareLiveEvent -> resetConversationState. Close uses the same
\* retirement path without changing session. Close means overlay dismissal/reset,
\* not closing a conversation panel; conversation turns have a separate owner.
\* A streamed final can arrive before the command promise settles, enabling
\* overlay dismissal or exposing a disabled retry.
\* Complete explicitly includes both late success and failure plus finally.
\* Two requests, one retirement, one optional same-session reopen bound the model.
=============================================================================
