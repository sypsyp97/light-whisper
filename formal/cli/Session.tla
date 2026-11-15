----------------------------- MODULE Session -----------------------------
EXTENDS Integers, Sequences, FiniteSets

(* Public main(..., "session") contract synchronized to run_session/owned_engine.
   Bound: zero to three input records, at most two engine operations. Repeated
   opaque client IDs (41,-7,41) and monotonic wire IDs occupy separate domains.
   bad_id / bad_fields partition invalid records; blank is an ignored line.

   Binding: main's stdin owner validates each record, acquires one EngineClient
   lazily, calls request serially, and emits completed operation results while
   retaining the owner. EOF closes without a JSON reply. Explicit exit closes
   ExitStack BEFORE ACK; startup/transport/interrupt exceptions unwind the owner
   BEFORE publishing the pending failure. EngineClient already owns
   startup, wire correlation and child reaping; its fine-grained pipe/sender
   protocol is checked separately by EngineClient.tla / SenderHandoff.tla.
   Acquire / Start / Dispatch / Complete / TransportFailure / Reap / ExitReply /
   FailureReply remain separate steps. CleanupFailure represents an error after
   resource release has settled; it changes the exit status and pending reply,
   or emits only stderr when no request is pending. Hard-kill containment and
   failures that prevent resource release belong to the separate transport
   models and are outside this cleanup-reporting abstraction.

   Fairness: each enabled CLI/engine step eventually settles. The engine may
   settle with success, backend false, timeout or EOF; no successful inference
   assumption. SkipReapProgress deliberately permits a closing stutter to test
   the cleanup progress obligation. No concurrency, crash durability, actual
   GPU inference, unbounded inputs, or live-desktop connection is proved here.
*)

CONSTANTS Mode, Witness
VARIABLE s
vars == <<s>>

Operations == {"status", "stats", "transcribe"}
Invalid == {"bad_id", "bad_fields"}
Events == Operations \cup Invalid \cup {"blank", "exit"}
Inputs == {lines \in UNION {[1..n -> Events] : n \in 0..3} :
            Cardinality({i \in DOMAIN lines : lines[i] \in Operations}) <= 2}
(* A tagged value represents JSON null without heterogeneous TLC equality. *)
ClientId(line) == [valid |-> s.input[line] # "bad_id",
                   value |-> IF s.input[line] = "bad_id" THEN 0
                             ELSE IF line = 2 THEN -7 ELSE 41]
Reply(line, ok, kind, observed, expected) ==
    [line |-> line, client_id |-> ClientId(line), success |-> ok,
     kind |-> kind, stats |-> observed, expected |-> expected]

Init == \E lines \in Inputs : s =
    [phase |-> "reading", input |-> lines, position |-> 1, pending |-> 0,
     acquired |-> IF Mode = "EagerSpawn" THEN 1 ELSE 0,
     live |-> (Mode = "EagerSpawn"), reaped |-> 0, valid_seen |-> FALSE,
     wire |-> 0, dispatched |-> <<>>, replies |-> <<>>,
     terminal_line |-> 0, reason |-> "none", code |-> 0,
     completed |-> 0, backend_count |-> 0, failure |-> "none",
     cleanup_failed |-> FALSE, diagnostic |-> FALSE, terminal_replies |-> 0]

ReadBlank ==
    /\ s.phase = "reading" /\ s.position <= Len(s.input)
    /\ s.input[s.position] = "blank"
    /\ s' = [s EXCEPT !.position = @ + 1, !.pending = 0,
              !.replies = IF Mode = "ReplyBlank"
                          THEN Append(@, Reply(s.position, FALSE, "invalid", 0, 0)) ELSE @]

ReadInvalid ==
    /\ s.phase = "reading" /\ s.position <= Len(s.input)
    /\ s.input[s.position] \in Invalid
    /\ s' = [s EXCEPT !.position = @ + 1, !.pending = 0,
              !.replies = Append(@, Reply(s.position, FALSE, "invalid", 0, 0)),
              !.dispatched = IF Mode = "DispatchInvalid"
                 THEN Append(@, [line |-> s.position, action |-> s.input[s.position], wire |-> s.wire + 1])
                 ELSE @,
              !.wire = IF Mode = "DispatchInvalid" THEN @ + 1 ELSE @,
              !.phase = IF Mode = "StopInvalid" THEN "closing" ELSE @,
              !.reason = IF Mode = "StopInvalid" THEN "validation" ELSE @]

ReadOperation ==
    /\ s.phase = "reading" /\ s.position <= Len(s.input)
    /\ s.input[s.position] \in Operations
    /\ s' = [s EXCEPT !.pending = s.position, !.position = @ + 1,
              !.valid_seen = TRUE,
              !.phase = IF s.live /\ Mode # "RestartEachRequest" THEN "dispatch" ELSE "acquiring"]

Acquire ==
    /\ s.phase = "acquiring"
    /\ s' = [s EXCEPT !.acquired = @ + 1, !.live = TRUE,
              !.backend_count = 0, !.phase = "starting"]

StartOK == /\ s.phase = "starting" /\ s' = [s EXCEPT !.phase = "dispatch"]
StartFailure ==
    /\ s.phase = "starting"
    /\ s' = [s EXCEPT !.failure = "startup",
              !.phase = "closing", !.terminal_line = s.pending,
              !.reason = "startup", !.code = 1]

Dispatch ==
    /\ s.phase = "dispatch"
    /\ s' = [s EXCEPT !.wire = @ + 1, !.phase = "waiting",
              !.dispatched = Append(@,
                [line |-> s.pending, action |-> s.input[s.pending],
                 wire |-> IF Mode = "UseClientWireId" THEN ClientId(s.pending).value ELSE s.wire + 1])]

Complete(ok) ==
    /\ s.phase = "waiting"
    /\ LET stat == s.input[s.pending] = "stats" /\ ok
           response == Reply(s.pending, ok, "backend",
                         IF stat /\ Mode # "LoseStats" THEN s.backend_count ELSE 0,
                         IF stat THEN s.completed ELSE 0)
           published == IF Mode = "ReplyWireId"
                        THEN [response EXCEPT !.client_id = [valid |-> TRUE, value |-> s.wire]] ELSE response
       IN s' = [s EXCEPT !.replies = Append(@, published),
                 !.pending = IF Mode = "RetainPending" THEN @ ELSE 0,
                 !.completed = IF ok /\ s.input[s.pending] = "transcribe" THEN @ + 1 ELSE @,
                 !.backend_count = IF ok /\ s.input[s.pending] = "transcribe" THEN @ + 1 ELSE @,
                 !.phase = IF ~ok /\ Mode = "StopBackendFalse" THEN "closing" ELSE "reading",
                 !.reason = IF ~ok /\ Mode = "StopBackendFalse" THEN "backend_false" ELSE @]

TransportFailure(reason) ==
    /\ s.phase = "waiting"
    /\ s' = [s EXCEPT !.failure = reason,
              !.terminal_line = s.pending, !.reason = reason, !.code = 1,
              !.phase = IF Mode = "ContinueTransport" THEN "reading" ELSE "closing"]

ReadExit ==
    /\ s.phase = "reading" /\ s.position <= Len(s.input)
    /\ s.input[s.position] = "exit"
    /\ s' = [s EXCEPT !.pending = s.position, !.position = @ + 1,
              !.phase = "closing", !.reason = "exit",
              !.replies = IF Mode = "EarlyExitAck"
                 THEN Append(@, Reply(s.position, TRUE, "exit", 0, 0)) ELSE @,
              !.terminal_replies = IF Mode = "EarlyExitAck" THEN 1 ELSE @]

ReadEOF ==
    /\ s.phase = "reading" /\ s.position > Len(s.input)
    /\ s' = [s EXCEPT !.phase = "closing", !.reason = "eof", !.failure = "none"]

AfterCleanup == IF s.reason = "exit" THEN "exit_reply"
                ELSE IF s.failure # "none" THEN "failure_reply" ELSE "done"

Reap ==
    /\ s.phase = "closing" /\ Mode # "SkipReapProgress"
    /\ s' = [s EXCEPT !.phase = AfterCleanup,
              !.live = IF Mode = "SkipReap" THEN @ ELSE FALSE,
              !.reaped = IF s.live /\ Mode # "SkipReap" THEN @ + 1 ELSE @]

CleanupFailure ==
    /\ s.phase = "closing" /\ s.live /\ Mode # "SkipReapProgress"
    /\ s' = [s EXCEPT !.phase = IF s.reason = "exit" THEN "exit_reply"
                              ELSE IF s.pending > 0 THEN "failure_reply" ELSE "done",
              !.live = FALSE, !.reaped = @ + 1,
              !.cleanup_failed = TRUE, !.code = IF Mode = "IgnoreCleanupFailure" THEN 0 ELSE 1,
              !.diagnostic = (s.pending = 0),
              !.replies = IF Mode = "ExtraEOFReply" /\ s.reason = "eof"
                 THEN Append(@, Reply(s.dispatched[Len(s.dispatched)].line, FALSE, "cleanup", 0, 0)) ELSE @,
              !.terminal_replies = IF Mode = "ExtraEOFReply" /\ s.reason = "eof" THEN 1 ELSE @]

ExitReply ==
    /\ s.phase = "exit_reply"
    /\ LET ack == Reply(s.pending, s.code = 0, "exit", 0, 0)
       IN s' = [s EXCEPT !.phase = "done",
                 !.replies = CASE Mode = "DuplicateExit" -> Append(Append(@, ack), ack)
                              [] Mode \in {"DropExit", "EarlyExitAck"} -> @
                              [] OTHER -> Append(@, ack),
                 !.terminal_replies = CASE Mode = "DuplicateExit" -> @ + 2
                              [] Mode \in {"DropExit", "EarlyExitAck"} -> @
                              [] OTHER -> @ + 1]

FailureReply ==
    /\ s.phase = "failure_reply"
    /\ s' = [s EXCEPT !.phase = "done", !.terminal_replies = @ + 1,
              !.replies = Append(@, Reply(s.pending, FALSE,
                                         IF s.failure = "none" THEN "cleanup" ELSE s.failure, 0, 0))]

InterruptReading ==
    /\ s.phase = "reading"
    /\ s' = [s EXCEPT !.phase = "closing", !.reason = "interrupt",
              !.failure = IF s.pending > 0 THEN "interrupt" ELSE "none",
              !.diagnostic = (s.pending = 0), !.code = IF Mode = "SwallowInterrupt" THEN 0 ELSE 130]

InterruptWaiting ==
    /\ s.phase = "waiting"
    /\ s' = [s EXCEPT !.phase = "closing", !.reason = "interrupt", !.failure = "interrupt",
              !.terminal_line = s.pending, !.code = IF Mode = "SwallowInterrupt" THEN 0 ELSE 130]

StuckCleanup == /\ s.phase = "closing" /\ Mode = "SkipReapProgress" /\ UNCHANGED vars
Done == /\ s.phase = "done" /\ UNCHANGED vars
Next == ReadBlank \/ ReadInvalid \/ ReadOperation \/ Acquire \/ StartOK \/ StartFailure
        \/ Dispatch \/ Complete(TRUE) \/ Complete(FALSE)
        \/ TransportFailure("timeout") \/ TransportFailure("engine_eof")
        \/ ReadExit \/ ReadEOF \/ Reap \/ CleanupFailure \/ ExitReply \/ FailureReply
        \/ InterruptReading \/ InterruptWaiting \/ StuckCleanup \/ Done
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ s.input \in Inputs /\ s.position \in 1..4 /\ s.pending \in 0..3
    /\ s.phase \in {"reading", "acquiring", "starting", "dispatch", "waiting", "closing",
                     "exit_reply", "failure_reply", "done"}
    /\ s.acquired \in 0..2 /\ s.reaped \in 0..2 /\ s.live \in BOOLEAN
    /\ s.valid_seen \in BOOLEAN /\ s.wire \in 0..3 /\ s.code \in {0, 1, 130}
    /\ s.reason \in {"none", "validation", "backend_false", "startup", "timeout", "engine_eof", "exit", "eof", "interrupt"}
    /\ s.failure \in {"none", "startup", "timeout", "engine_eof", "interrupt"}
    /\ s.cleanup_failed \in BOOLEAN /\ s.diagnostic \in BOOLEAN /\ s.terminal_replies \in 0..2
    /\ s.dispatched \in Seq([line : 1..3, action : Events, wire : Int])
    /\ s.replies \in Seq([line : 1..3, client_id : [valid : BOOLEAN, value : Int], success : BOOLEAN,
                          kind : {"invalid", "backend", "startup", "timeout", "engine_eof", "exit", "interrupt", "cleanup"},
                          stats : 0..2, expected : 0..2])
    /\ s.completed \in 0..2 /\ s.backend_count \in 0..2

SingleOwner == s.acquired <= 1
LazyAcquisition == s.acquired > 0 => s.valid_seen
InputGuard == \A i \in DOMAIN s.dispatched : s.dispatched[i].action \in Operations
WireMonotonic == \A i \in DOMAIN s.dispatched : s.dispatched[i].wire = i
ClientIdentity == \A i \in DOMAIN s.replies :
                    s.replies[i].client_id = ClientId(s.replies[i].line)
RepliesPerLine == \A i, j \in DOMAIN s.replies : i # j => s.replies[i].line # s.replies[j].line
ReplyOrder == \A i, j \in DOMAIN s.replies : i < j => s.replies[i].line < s.replies[j].line
BlankIgnored == \A i \in DOMAIN s.replies : s.input[s.replies[i].line] # "blank"
NoPostFailureDispatch == s.terminal_line > 0 =>
                          \A i \in DOMAIN s.dispatched : s.dispatched[i].line <= s.terminal_line
StatsRetained == \A i \in DOMAIN s.replies : s.replies[i].stats = s.replies[i].expected
RecoverableFailures == s.reason \notin {"validation", "backend_false"}
ExitAcknowledged == s.phase = "done" /\ s.reason = "exit" =>
                     \E i \in DOMAIN s.replies : s.replies[i].line = s.pending /\ s.replies[i].kind = "exit"
ExitAckAfterReap == \A i \in DOMAIN s.replies :
                     s.replies[i].kind = "exit" /\ s.replies[i].success =>
                       ~s.live /\ s.acquired = s.reaped
CleanupFailureReported == s.cleanup_failed =>
                            s.code = 1 /\ (s.pending = 0 => s.diagnostic)
                            /\ (s.phase = "done" /\ s.reason = "exit" =>
                                 \A i \in DOMAIN s.replies : s.replies[i].kind = "exit" => ~s.replies[i].success)
EOFNoExtraReply == s.reason = "eof" => s.terminal_replies = 0
InterruptResult == s.reason = "interrupt" /\ ~s.cleanup_failed => s.code = 130
CleanExit == s.phase = "done" => ~s.live /\ s.acquired = s.reaped
EventuallyDone == <> (s.phase = "done")

WitnessReached ==
    CASE Witness = "reuse" -> s.phase = "done" /\ s.acquired = 1 /\ Len(s.dispatched) = 2 /\ s.code = 0
      [] Witness = "recovery" -> s.phase = "done" /\ Len(s.replies) >= 2
                                /\ s.replies[1].kind = "invalid" /\ s.replies[2].kind = "backend"
      [] Witness = "backend_false" -> s.phase = "done" /\ Len(s.replies) >= 2
                                /\ s.replies[1].kind = "backend" /\ ~s.replies[1].success
                                /\ s.replies[2].kind = "backend" /\ s.replies[2].success
      [] Witness = "transport" -> s.phase = "done" /\ s.reason \in {"timeout", "engine_eof"}
                                  /\ Len(s.dispatched) = 1 /\ s.code = 1
      [] Witness = "startup" -> s.phase = "done" /\ s.reason = "startup"
                                /\ s.acquired = 1 /\ Len(s.dispatched) = 0 /\ s.code = 1
      [] Witness = "empty_eof" -> s.phase = "done" /\ Len(s.input) = 0 /\ s.acquired = 0
      [] Witness = "exit_without_engine" -> s.phase = "done" /\ s.reason = "exit" /\ s.acquired = 0
      [] Witness = "stats" -> s.phase = "done" /\ \E i \in DOMAIN s.replies : s.replies[i].stats = 1
      [] Witness = "exit_after_reap" -> s.phase = "done" /\ s.reason = "exit"
                                       /\ s.acquired = 1 /\ s.reaped = 1 /\ s.code = 0
      [] Witness = "cleanup_exit" -> s.phase = "done" /\ s.reason = "exit" /\ s.cleanup_failed
      [] Witness = "cleanup_eof" -> s.phase = "done" /\ s.reason = "eof" /\ s.cleanup_failed
      [] Witness = "interrupt_read" -> s.phase = "done" /\ s.reason = "interrupt" /\ s.code = 130
                                      /\ s.acquired = 1 /\ s.failure = "none" /\ Len(s.replies) = 1
      [] Witness = "interrupt_wait" -> s.phase = "done" /\ s.code = 130 /\ s.failure = "interrupt"
      [] OTHER -> FALSE
WitnessNotReached == ~WitnessReached
=============================================================================
