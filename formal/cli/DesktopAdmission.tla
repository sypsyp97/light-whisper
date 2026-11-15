----------------------- MODULE DesktopAdmission -----------------------
EXTENDS Naturals, Sequences

(* Desired CLI preflight at main/run_session before request_client acquisition
   or EngineClient.request. Baseline 1517896105d1...daf504 lacks that guard;
   Legacy admits the same invalid records. No new production helper is assumed.
   DesktopAdmission.md records source/ownership boundaries and finite bounds.

   Body represents ensure_ascii=True JSON bytes excluding only the internal
   request ID digits and physical newline. Newline widths 1 and 2 represent
   TextIOWrapper(newline=None), including Windows CRLF. Body includes all JSON
   punctuation, escaping, fields and resolved audio path. No UTF-8 re-encoding
   feature is required. Scalar validity represents the independent Rust Unicode
   domain restriction; Python-engine transport retains its existing domain.

   One-shot or two session records, with optional existing owned client after
   eight/nine valid operations. The second record is a valid status operation.
   Validation, lazy acquisition, send, reply and cleanup are separate actions.
   Local synchronous steps and cleanup eventually settle; no broker/network
   responsiveness or wall-clock claim is assumed/proved. No native Rust change.
*)

CONSTANTS Mode, Witness
VARIABLE s
vars == <<s>>

Cap == 65536
Kinds == {"scalar", "high", "low", "escaped_overflow", "exact_one_digit",
          "exact_two_digits", "one_over"}
Digits(id) == IF id < 10 THEN 1 ELSE 2
FirstBody ==
    CASE s.kind = "escaped_overflow" -> 66000
      [] s.kind = "exact_one_digit" -> Cap - s.newline - 1
      [] s.kind = "exact_two_digits" -> Cap - s.newline - 2
      [] s.kind = "one_over" -> Cap - s.newline
      [] OTHER -> 100
Body == IF s.index = 1 THEN FirstBody ELSE 50
CurrentId == IF s.phase = "validate" THEN s.sequence + 1 ELSE s.pending
ScalarValid == s.index = 2 \/ s.kind \notin {"high", "low"}
WireBytes == Body + Digits(CurrentId) + s.newline
Admissible == s.target = "engine" \/ (ScalarValid /\ WireBytes <= Cap)
FirstInvalid == s.target = "app" /\
    (s.kind \in {"high", "low"} \/
     FirstBody + Digits(s.initialSequence + 1) + s.newline > Cap)
Caller == IF s.index = 2 THEN "next" ELSE IF s.callerWidth = 618 THEN "opaque" ELSE "small"

Guard ==
    IF Mode = "Legacy" \/ (s.target = "engine" /\ Mode # "RejectEngine") THEN TRUE
    ELSE /\ (ScalarValid \/ Mode = "SkipUnicode")
         /\ (Mode = "SkipSize" \/
             Body + (IF Mode = "ExternalId" THEN s.callerWidth
                     ELSE IF Mode = "FixedId" THEN 1 ELSE Digits(s.sequence + 1))
                  + (IF Mode = "OmitNewline" THEN 0 ELSE s.newline) <= Cap)

Init ==
    \E surface \in {"oneshot", "session"}, target \in {"app", "engine"},
       live \in BOOLEAN, initial \in {0, 8, 9}, newline \in {1, 2},
       kind \in Kinds, width \in {1, 618} :
       /\ (live => surface = "session" /\ initial \in {8, 9})
       /\ (~live => initial = 0)
       /\ s = [phase |-> "validate", surface |-> surface, target |-> target,
                initiallyLive |-> live, initialSequence |-> initial,
                newline |-> newline, kind |-> kind, callerWidth |-> width,
                index |-> 1, live |-> live, opens |-> IF live THEN 1 ELSE 0,
                closes |-> 0, sequence |-> initial, pending |-> 0,
                writes |-> 0, rejected |-> FALSE, invalidEffects |-> FALSE,
                replies |-> <<>>, code |-> 0]

EagerAcquire ==
    /\ s.phase = "validate" /\ ~s.live /\ Mode = "EagerAcquire"
    /\ s' = [s EXCEPT !.live = TRUE, !.opens = @ + 1,
                      !.invalidEffects = @ \/ ~Admissible]

Validate ==
    /\ s.phase = "validate"
    /\ s' = [s EXCEPT
        !.pending = s.sequence + 1,
        !.rejected = ~Guard,
        !.sequence = IF ~Guard /\ Mode = "ConsumeRejected" THEN @ + 1 ELSE @,
        !.live = IF ~Guard /\ Mode = "DropClient" THEN FALSE ELSE @,
        !.closes = IF ~Guard /\ Mode = "DropClient" /\ s.live THEN @ + 1 ELSE @,
        !.phase = IF Guard THEN IF s.live THEN "send" ELSE "acquire" ELSE "reply"]

Acquire ==
    /\ s.phase = "acquire"
    /\ s' = [s EXCEPT !.live = TRUE, !.opens = @ + 1, !.phase = "send",
                      !.invalidEffects = @ \/ ~Admissible]

Send ==
    /\ s.phase = "send" /\ s.live
    /\ s' = [s EXCEPT !.sequence = @ + 1, !.writes = @ + 1, !.phase = "reply",
                      !.invalidEffects = @ \/ ~Admissible]

Reply ==
    /\ s.phase = "reply"
    /\ ~(Mode = "StallAfterInvalid" /\ s.rejected)
    /\ s' = [s EXCEPT
        !.replies = Append(@, [caller |-> IF Mode = "WrongCaller" THEN "wire" ELSE Caller,
                               success |-> ~s.rejected]),
        !.code = IF s.surface = "oneshot" /\ s.rejected /\ Mode # "WrongExitCode" THEN 2 ELSE 0,
        !.phase = IF s.surface = "oneshot" \/ s.index = 2 \/
                     (Mode = "StopAfterInvalid" /\ s.rejected)
                  THEN "closing" ELSE "validate",
        !.index = IF s.surface = "session" /\ s.index = 1 /\
                     ~(Mode = "StopAfterInvalid" /\ s.rejected) THEN 2 ELSE @]

Close ==
    /\ s.phase = "closing"
    /\ s' = [s EXCEPT !.phase = "done",
                      !.closes = IF s.live /\ Mode # "ForgetCleanup" THEN @ + 1 ELSE @,
                      !.live = IF Mode = "ForgetCleanup" THEN @ ELSE FALSE]

Done == /\ s.phase = "done" /\ UNCHANGED vars
Stall == /\ s.phase = "reply" /\ s.rejected /\ Mode = "StallAfterInvalid"
         /\ UNCHANGED vars
Advance == EagerAcquire \/ Validate \/ Acquire \/ Send \/ Reply \/ Close
Spec == Init /\ [][Advance \/ Done \/ Stall]_vars /\ WF_vars(Advance)

TypeOK ==
    /\ s.phase \in {"validate", "acquire", "send", "reply", "closing", "done"}
    /\ s.surface \in {"oneshot", "session"} /\ s.target \in {"app", "engine"}
    /\ s.initiallyLive \in BOOLEAN /\ s.live \in BOOLEAN
    /\ s.initialSequence \in {0, 8, 9} /\ s.newline \in {1, 2}
    /\ s.kind \in Kinds /\ s.callerWidth \in {1, 618}
    /\ s.index \in 1..2 /\ s.sequence \in 0..13 /\ s.pending \in 0..12
    /\ s.opens \in 0..2 /\ s.closes \in 0..2 /\ s.writes \in 0..2
    /\ s.rejected \in BOOLEAN /\ s.invalidEffects \in BOOLEAN /\ s.code \in {0, 2}
    /\ s.replies \in Seq([caller : {"opaque", "small", "next", "wire"}, success : BOOLEAN])
    /\ Len(s.replies) <= 2

NoInvalidEffects == ~s.invalidEffects
DecisionMatches == s.phase \in {"acquire", "send", "reply"} => s.rejected = ~Admissible
LazyFirstRecord == FirstInvalid /\ ~s.initiallyLive /\ s.index = 1 => s.opens = 0
LiveClientPreserved == FirstInvalid /\ s.initiallyLive /\ s.index = 1 /\
    s.phase \notin {"closing", "done"} => s.live /\ s.opens = 1 /\ s.closes = 0
InternalSequence == s.sequence = s.initialSequence + s.writes
CallerIdentity == \A i \in 1..Len(s.replies) :
    s.replies[i].caller = IF i = 2 THEN "next" ELSE IF s.callerWidth = 618 THEN "opaque" ELSE "small"
EngineUnchanged == s.target = "engine" /\ s.phase \in {"acquire", "send", "reply"} => ~s.rejected
ClientReused == s.opens <= 1
OneShotError == s.phase = "done" /\ s.surface = "oneshot" /\ FirstInvalid =>
    s.code = 2 /\ Len(s.replies) = 1 /\ ~s.replies[1].success
SessionContinuation == s.phase = "done" /\ s.surface = "session" =>
    Len(s.replies) = 2 /\ s.replies[2].success
OwnedCleanup == s.phase = "done" => ~s.live /\ s.opens = s.closes
EventuallyFinished == <> (s.phase = "done")

WitnessReached == s.phase = "done" /\
    CASE Witness = "fresh_recovery" -> s.surface = "session" /\ FirstInvalid /\
              ~s.initiallyLive /\ Len(s.replies) = 2 /\ ~s.replies[1].success /\ s.replies[2].success
      [] Witness = "live_recovery" -> s.surface = "session" /\ FirstInvalid /\
              s.initiallyLive /\ Len(s.replies) = 2 /\ ~s.replies[1].success /\ s.replies[2].success
      [] Witness = "id9_exact" -> s.target = "app" /\ s.kind = "exact_one_digit" /\
              s.initialSequence = 8 /\ s.newline = 2 /\ s.replies[1].success
      [] Witness = "id10_over" -> s.target = "app" /\ s.kind = "exact_one_digit" /\
              s.initialSequence = 9 /\ s.newline = 2 /\ ~s.replies[1].success
      [] Witness = "engine_unicode" -> s.target = "engine" /\ s.kind = "high" /\ s.replies[1].success
      [] Witness = "escaped_overflow" -> s.target = "app" /\ s.kind = "escaped_overflow" /\
              ~s.replies[1].success /\ s.opens = IF s.surface = "oneshot" THEN 0 ELSE 1
      [] OTHER -> FALSE
WitnessNotReached == ~WitnessReached
=============================================================================
