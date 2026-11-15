----------------------------- MODULE Broker -----------------------------
EXTENDS Naturals, Sequences
CONSTANTS Mode, Target
VARIABLES input, phase, index, startupFlushed, replyFlushed, calls, replies,
          terminal, terminalAt, terminalReason, fatal, retried, probed
vars == <<input, phase, index, startupFlushed, replyFlushed, calls, replies,
          terminal, terminalAt, terminalReason, fatal, retried, probed>>
Kinds == {"valid", "bad", "backend-false", "transport-failure", "exit", "eof"}
Init == /\ input \in [1..2 -> Kinds]
        /\ phase = "startup" /\ index = 0 /\ startupFlushed = FALSE
        /\ replyFlushed = TRUE /\ calls = <<>> /\ replies = <<>>
        /\ terminal = FALSE /\ terminalAt = 0 /\ terminalReason = "none"
        /\ fatal = FALSE /\ retried = FALSE /\ probed = FALSE
Startup == /\ phase = "startup" /\ phase' = "input"
           /\ startupFlushed' = (Mode # "startup-flush")
           /\ probed' = (Mode = "startup-probe")
           /\ UNCHANGED <<input, index, replyFlushed, calls, replies,
                terminal, terminalAt, terminalReason, fatal, retried>>
Read == /\ phase = "input" /\ index < 2
        /\ (~terminal \/ Mode = "post-terminal-read")
        /\ index' = index + 1 /\ phase' = "handle"
        /\ UNCHANGED <<input, startupFlushed, replyFlushed, calls, replies,
             terminal, terminalAt, terminalReason, fatal, retried, probed>>
Handle == /\ phase = "handle"
          /\ LET kind == input[index] IN
             /\ calls' = IF kind \in {"valid", "backend-false", "transport-failure"}
                              \/ (kind = "exit" /\ Mode = "exit-dispatch")
                          THEN Append(calls, [id |-> index, kind |-> kind]) ELSE calls
             /\ replies' = IF kind = "eof" THEN replies
                           ELSE Append(replies, [id |-> IF kind = "transport-failure" /\ Mode = "fatal-id" THEN 0 ELSE index,
                                                  kind |-> kind])
             /\ terminal' = (terminal \/ kind \in {"exit", "eof", "transport-failure"}
                            \/ (kind \in {"bad", "backend-false"} /\ Mode = "stop-recoverable"))
             /\ terminalAt' = IF ~terminal /\ terminal' THEN index ELSE terminalAt
             /\ terminalReason' = IF ~terminal /\ terminal' THEN kind ELSE terminalReason
             /\ fatal' = (fatal \/ kind = "transport-failure")
             /\ replyFlushed' = (kind = "eof" \/ Mode # "reply-flush")
          /\ phase' = "input"
          /\ UNCHANGED <<input, index, startupFlushed, retried, probed>>
Retry == /\ terminal /\ fatal /\ ~retried /\ Mode = "retry"
         /\ retried' = TRUE
         /\ UNCHANGED <<input, phase, index, startupFlushed, replyFlushed, calls,
              replies, terminal, terminalAt, terminalReason, fatal, probed>>
Finish == /\ phase = "input" /\ (terminal \/ index = 2) /\ phase' = "done"
          /\ UNCHANGED <<input, index, startupFlushed, replyFlushed, calls, replies,
               terminal, terminalAt, terminalReason, fatal, retried, probed>>
Idle == /\ phase = "done" /\ UNCHANGED vars
Next == Startup \/ Read \/ Handle \/ Retry \/ Finish \/ Idle
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
TypeOK == /\ input \in [1..2 -> Kinds] /\ phase \in {"startup", "input", "handle", "done"}
          /\ index \in 0..2 /\ terminalAt \in 0..2
          /\ startupFlushed \in BOOLEAN /\ replyFlushed \in BOOLEAN
          /\ terminal \in BOOLEAN /\ fatal \in BOOLEAN /\ retried \in BOOLEAN /\ probed \in BOOLEAN
          /\ Len(calls) <= 2 /\ Len(replies) <= 2
StartupBeforeInput == index > 0 => startupFlushed
NoStartupProbe == ~probed
LocalExitOnly == \A i \in 1..Len(calls) : calls[i].kind # "exit"
NoQueuedRead == terminal => index <= terminalAt
CorrelatedFatal == \A i \in 1..Len(replies) : replies[i].kind = "transport-failure" => replies[i].id # 0
RecoverableFailures == terminal => terminalReason \in {"exit", "eof", "transport-failure"}
NoRetry == ~retried
FlushedReplies == phase \in {"input", "done"} => replyFlushed
EventuallySettled == <> (phase = "done")
WitnessNotReached == ~CASE Target = "recovery" -> index = 2 /\ input[1] = "bad" /\ input[2] = "valid" /\ Len(calls) = 1
                       [] Target = "backend-recovery" -> index = 2 /\ input[1] = "backend-false" /\ Len(calls) = 2
                       [] Target = "fatal" -> terminal /\ fatal /\ index = 1 /\ Len(replies) = 1
                       [] Target = "local-exit" -> terminalReason = "exit" /\ Len(calls) = 0
                       [] Target = "empty-eof" -> terminalReason = "eof" /\ Len(replies) = 0
                       [] OTHER -> FALSE
=============================================================================
