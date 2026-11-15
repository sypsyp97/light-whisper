--------------------------- MODULE JsonNumbers ---------------------------
EXTENDS Integers, Sequences

(* Additive admission model, not a replacement for EngineClient/Session.
   One already-admitted public command owns one child. One startup and one
   action reply may contain nested NaN or overflow-to-infinity. Nesting is
   abstracted by the whole-frame finite predicate; executable tests traverse
   object -> array -> object and retain finite floats and arbitrary integers.
   An irrelevant log/foreign envelope precedes the matching frame. A valid
   matching reply is queued immediately behind each bad frame. Treating a bad
   matching frame as ignorable can therefore make a command incorrectly pass.
   Session has one queued operation. External signed IDs and wire IDs occupy
   separate domains. Cleanup and failure publication remain separate actions.
   No constraints, symmetry, VIEW, clock fairness or backend retry assumption.
   WF(Advance) assumes each enabled local step/owned cleanup eventually settles;
   bounded timeout/kill internals remain in the existing transport models.
*)
CONSTANTS Mode, Witness
VARIABLE s
vars == <<s>>

Init ==
    /\ s \in [surface : {"one_shot", "session"}, badAt : {"none", "startup", "action"},
              number : {"nan", "overflow"}, noise : {"none", "foreign", "log"},
              noiseAt : {"startup", "action"},
              externalId : {-7, 80}, phase : {"startup"}, live : {TRUE},
              ignored : {FALSE}, badSeen : {FALSE}, dispatches : {0},
              consumed : {1}, attempts : {1}, code : {0}, replies : {<<>>}]
    /\ (s.noise = "foreign" => s.noiseAt = "action")

Reply(kind, finite) == [kind |-> kind, finite |-> finite,
                        id |-> IF Mode = "WrongIdentity"
                               THEN IF s.externalId = -7 THEN 80 ELSE -7
                               ELSE s.externalId,
                        afterReap |-> ~s.live]
ReceivePhase == s.phase \in {"startup", "action"}
CanMatch == ReceivePhase /\ (s.noise = "none" \/ s.noiseAt # s.phase \/ s.ignored)
AfterSuccess == IF s.surface = "session" /\ Mode # "FiniteStopsEarly" THEN "tail" ELSE "reap"

IgnoreNoise ==
    /\ ReceivePhase /\ s.noise # "none" /\ s.noiseAt = s.phase /\ ~s.ignored
    /\ s' = IF Mode = "ValidateNoise"
             THEN [s EXCEPT !.ignored = TRUE, !.badSeen = TRUE,
                            !.phase = "reap", !.code = 1]
             ELSE [s EXCEPT !.ignored = TRUE]

ReceiveFinite ==
    /\ CanMatch /\ s.badAt # s.phase
    /\ s' = IF s.phase = "startup"
             THEN [s EXCEPT !.phase = "dispatch"]
             ELSE [s EXCEPT !.phase = AfterSuccess,
                            !.replies = Append(@, Reply("backend", TRUE))]

ReceiveBad ==
    /\ CanMatch /\ s.badAt = s.phase
    /\ s' = IF Mode \in {"LegacyAccept", "SkipBad"}
             THEN [s EXCEPT !.badSeen = TRUE,
                   !.phase = IF s.phase = "startup" THEN "dispatch" ELSE AfterSuccess,
                   !.replies = IF s.phase = "startup" THEN @
                               ELSE Append(@, Reply("backend", Mode = "SkipBad"))]
             ELSE [s EXCEPT !.badSeen = TRUE, !.code = 1,
                   !.phase = IF Mode = "PublishBeforeReap" THEN "failure" ELSE "reap"]

Dispatch ==
    /\ s.phase = "dispatch"
    /\ s' = [s EXCEPT !.phase = "action", !.dispatches = @ + 1]

ReadTail ==
    /\ s.phase = "tail"
    /\ s' = [s EXCEPT !.consumed = 2, !.dispatches = @ + 1, !.phase = "reap",
                       !.replies = Append(@, Reply("backend", TRUE))]

Reap ==
    /\ s.phase = "reap"
    /\ ~(Mode = "StallRejectedCleanup" /\ s.badSeen)
    /\ s' = [s EXCEPT !.live = FALSE,
                       !.phase = IF s.code = 1 THEN "failure" ELSE "done"]

FailureReply ==
    /\ s.phase = "failure"
    /\ s' = [s EXCEPT !.phase = "done",
                       !.replies = IF Mode = "HideFailure" THEN @
                                  ELSE Append(@, Reply("failure", TRUE))]

Advance == IgnoreNoise \/ ReceiveFinite \/ ReceiveBad \/ Dispatch \/ ReadTail \/ Reap \/ FailureReply
Done == /\ s.phase = "done" /\ UNCHANGED vars
StallRejectedCleanup == /\ Mode = "StallRejectedCleanup" /\ s.phase = "reap" /\ s.badSeen
                        /\ UNCHANGED vars
Spec == Init /\ [][Advance \/ Done \/ StallRejectedCleanup]_vars /\ WF_vars(Advance)

TypeOK ==
    /\ s.surface \in {"one_shot", "session"} /\ s.badAt \in {"none", "startup", "action"}
    /\ s.number \in {"nan", "overflow"} /\ s.noise \in {"none", "foreign", "log"}
    /\ s.noiseAt \in {"startup", "action"}
    /\ s.externalId \in {-7, 80}
    /\ s.phase \in {"startup", "dispatch", "action", "tail", "reap", "failure", "done"}
    /\ s.live \in BOOLEAN /\ s.ignored \in BOOLEAN /\ s.badSeen \in BOOLEAN
    /\ s.dispatches \in 0..2 /\ s.consumed \in 1..2 /\ s.attempts = 1 /\ s.code \in {0, 1}
    /\ s.replies \in Seq([kind : {"backend", "failure"}, finite : BOOLEAN,
                           id : {-7, 80}, afterReap : BOOLEAN])
    /\ Len(s.replies) <= 2

StrictOutput == \A i \in DOMAIN s.replies : s.replies[i].finite
MatchingBadIsTerminal == s.badSeen =>
    /\ s.phase \in {"reap", "failure", "done"} /\ s.code = 1
    /\ s.consumed = 1 /\ s.dispatches = IF s.badAt = "startup" THEN 0 ELSE 1
BadReplyVisible == s.badSeen /\ s.phase = "done" =>
    /\ Len(s.replies) = 1 /\ s.replies[1].kind = "failure"
FailureAfterReap == \A i \in DOMAIN s.replies :
    s.replies[i].kind = "failure" => s.replies[i].afterReap
CorrelationPreserved == \A i \in DOMAIN s.replies : s.replies[i].id = s.externalId
IrrelevantFramesIgnored == s.badAt = "none" => ~s.badSeen /\ s.code = 0
FiniteFramesContinue == s.badAt = "none" /\ s.phase = "done" =>
    /\ s.code = 0
    /\ Len(s.replies) = IF s.surface = "session" THEN 2 ELSE 1
CleanExit == s.phase = "done" => ~s.live
EventuallyDone == <> (s.phase = "done")

WitnessReached == s.phase = "done" /\
    CASE Witness = "startup" -> s.badAt = "startup" /\ s.badSeen /\ s.code = 1
      [] Witness = "action" -> s.badAt = "action" /\ s.surface = "session" /\ s.badSeen /\ s.code = 1
      [] Witness = "noise" -> s.badAt = "none" /\ s.noise = "foreign" /\ s.ignored /\ s.code = 0
      [] Witness = "finite" -> s.badAt = "none" /\ s.surface = "session" /\ Len(s.replies) = 2
      [] OTHER -> FALSE
WitnessNotReached == ~WitnessReached
=============================================================================
