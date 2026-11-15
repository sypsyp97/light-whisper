-------------------------- MODULE SenderHandoff --------------------------
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS WaitForTail, RetainDeadline, ShareTailBudget

VARIABLES phase, process, nextID, slot, writers, outcomes, responseReady,
          budget, published, exitDecided, exitSent, skippedExit, stubborn,
          timedOut, sendFailed, timeoutWithTail, timeoutBlocked, killed,
          graceful, sawPublishedTail, handoffBudget

vars == <<phase, process, nextID, slot, writers, outcomes, responseReady,
          budget, published, exitDecided, exitSent, skippedExit, stubborn,
          timedOut, sendFailed, timeoutWithTail, timeoutBlocked, killed,
          graceful, sawPublishedTail, handoffBudget>>

Init == /\ phase = "ready" /\ process = "running"
        /\ nextID = 1 /\ slot = 0 /\ writers = {}
        /\ outcomes = [id \in 1..2 |-> "none"]
        /\ responseReady = FALSE /\ budget = "available" /\ published = <<>>
        /\ exitDecided = FALSE /\ exitSent = FALSE /\ skippedExit = FALSE
        /\ stubborn \in BOOLEAN /\ timedOut = FALSE /\ sendFailed = FALSE
        /\ timeoutWithTail = FALSE /\ timeoutBlocked = FALSE
        /\ killed = FALSE /\ graceful = FALSE /\ sawPublishedTail = FALSE
        /\ handoffBudget = [read |-> FALSE, elapsed |-> 0, allotted |-> 0]

Request == /\ phase = "ready" /\ nextID <= 2
           /\ phase' = "sending" /\ slot' = nextID /\ nextID' = nextID + 1
           /\ writers' = writers \cup {nextID}
           /\ responseReady' = FALSE /\ budget' = "available"
           /\ handoffBudget' = [read |-> FALSE, elapsed |-> 0, allotted |-> 0]
           /\ UNCHANGED <<process, outcomes, published, exitDecided, exitSent,
                          skippedExit, stubborn, timedOut, sendFailed, timeoutWithTail,
                          timeoutBlocked, killed, graceful, sawPublishedTail>>

WriteOutcome == /\ phase = "sending" /\ outcomes[slot] = "none"
                /\ outcomes' \in {[outcomes EXCEPT ![slot] = "ok"],
                                  [outcomes EXCEPT ![slot] = "failed"]}
                /\ sawPublishedTail' = TRUE
                /\ UNCHANGED <<phase, process, nextID, slot, writers, responseReady,
                               budget, published, exitDecided, exitSent, skippedExit,
                               stubborn, timedOut, sendFailed, timeoutWithTail,
                               timeoutBlocked, killed, graceful, handoffBudget>>

StopWriter == \E id \in writers:
                /\ outcomes[id] # "none" \/ process # "running"
                /\ writers' = writers \ {id}
                /\ UNCHANGED <<phase, process, nextID, slot, outcomes, responseReady,
                               budget, published, exitDecided, exitSent, skippedExit,
                               stubborn, timedOut, sendFailed, timeoutWithTail,
                               timeoutBlocked, killed, graceful, sawPublishedTail, handoffBudget>>

ReceiveResponse == /\ phase \in {"sending", "receiving"} /\ ~responseReady
                   /\ responseReady' = TRUE
                   /\ UNCHANGED <<phase, process, nextID, slot, writers, outcomes,
                                  budget, published, exitDecided, exitSent, skippedExit,
                                  stubborn, timedOut, sendFailed, timeoutWithTail,
                                  timeoutBlocked, killed, graceful, sawPublishedTail, handoffBudget>>

ObserveWrite == /\ phase = "sending" /\ outcomes[slot] = "ok" /\ ~handoffBudget.read
                /\ \E elapsed \in 0..3:
                     /\ (budget = "expired" => elapsed >= 2)
                     /\ handoffBudget' = [read |-> TRUE, elapsed |-> elapsed,
                          allotted |-> IF ShareTailBudget
                                       THEN IF elapsed < 2 THEN 2 - elapsed ELSE 0
                                       ELSE 2]
                     /\ budget' = IF elapsed >= 2 THEN "expired" ELSE budget
                /\ UNCHANGED <<phase, process, nextID, slot, writers, outcomes, responseReady,
                               published, exitDecided, exitSent, skippedExit,
                               stubborn, timedOut, sendFailed, timeoutWithTail,
                               timeoutBlocked, killed, graceful, sawPublishedTail>>

Handoff == /\ phase = "sending" /\ outcomes[slot] = "ok" /\ budget = "available"
           /\ handoffBudget.read
           /\ (~WaitForTail \/ slot \notin writers)
           /\ phase' = "receiving"
           /\ UNCHANGED <<process, nextID, slot, writers, outcomes, responseReady,
                          budget, published, exitDecided, exitSent, skippedExit,
                          stubborn, timedOut, sendFailed, timeoutWithTail,
                          timeoutBlocked, killed, graceful, sawPublishedTail, handoffBudget>>

PublishResponse == /\ phase = "receiving" /\ responseReady /\ budget = "available"
                   /\ phase' = "ready"
                   /\ published' = Append(published, [id |-> slot, quiescent |-> slot \notin writers])
                   /\ UNCHANGED <<process, nextID, slot, writers, outcomes, responseReady,
                                  budget, exitDecided, exitSent, skippedExit,
                                  stubborn, timedOut, sendFailed, timeoutWithTail,
                                  timeoutBlocked, killed, graceful, sawPublishedTail, handoffBudget>>

Deadline == /\ phase \in {"sending", "receiving"} /\ budget = "available"
            /\ budget' = "expired"
            /\ UNCHANGED <<phase, process, nextID, slot, writers, outcomes, responseReady,
                           published, exitDecided, exitSent, skippedExit,
                           stubborn, timedOut, sendFailed, timeoutWithTail,
                           timeoutBlocked, killed, graceful, sawPublishedTail, handoffBudget>>

FailDeadline == /\ phase \in {"sending", "receiving"} /\ budget = "expired" /\ RetainDeadline
                /\ phase' = "closing" /\ timedOut' = TRUE
                /\ timeoutWithTail' = (slot \in writers /\ outcomes[slot] = "ok")
                /\ timeoutBlocked' = (slot \in writers /\ outcomes[slot] = "none")
                /\ UNCHANGED <<process, nextID, slot, writers, outcomes, responseReady,
                               budget, published, exitDecided, exitSent, skippedExit,
                               stubborn, sendFailed, killed, graceful, sawPublishedTail, handoffBudget>>

FailWrite == /\ phase = "sending" /\ outcomes[slot] = "failed"
             /\ phase' = "closing" /\ sendFailed' = TRUE
             /\ UNCHANGED <<process, nextID, slot, writers, outcomes, responseReady,
                            budget, published, exitDecided, exitSent, skippedExit,
                            stubborn, timedOut, timeoutWithTail, timeoutBlocked,
                            killed, graceful, sawPublishedTail, handoffBudget>>

Quit == /\ phase = "ready" /\ phase' = "closing"
        /\ UNCHANGED <<process, nextID, slot, writers, outcomes, responseReady,
                       budget, published, exitDecided, exitSent, skippedExit,
                       stubborn, timedOut, sendFailed, timeoutWithTail,
                       timeoutBlocked, killed, graceful, sawPublishedTail, handoffBudget>>

DecideExit == /\ phase = "closing" /\ process = "running" /\ ~exitDecided
              /\ exitDecided' = TRUE
              /\ exitSent' = (slot \notin writers) /\ skippedExit' = (slot \in writers)
              /\ UNCHANGED <<phase, process, nextID, slot, writers, outcomes, responseReady,
                             budget, published, stubborn, timedOut, sendFailed,
                             timeoutWithTail, timeoutBlocked, killed, graceful, sawPublishedTail, handoffBudget>>

GracefulEnd == /\ phase = "closing" /\ process = "running" /\ exitSent /\ ~stubborn
               /\ process' = "ended" /\ graceful' = TRUE
               /\ UNCHANGED <<phase, nextID, slot, writers, outcomes, responseReady,
                              budget, published, exitDecided, exitSent, skippedExit,
                              stubborn, timedOut, sendFailed, timeoutWithTail,
                              timeoutBlocked, killed, sawPublishedTail, handoffBudget>>

Kill == /\ phase = "closing" /\ process = "running" /\ exitDecided
        /\ (exitSent \/ skippedExit)
        /\ process' = "ended" /\ killed' = TRUE
        /\ UNCHANGED <<phase, nextID, slot, writers, outcomes, responseReady,
                       budget, published, exitDecided, exitSent, skippedExit,
                       stubborn, timedOut, sendFailed, timeoutWithTail,
                       timeoutBlocked, graceful, sawPublishedTail, handoffBudget>>

Reap == /\ phase = "closing" /\ process = "ended"
        /\ phase' = "reaped" /\ process' = "reaped"
        /\ UNCHANGED <<nextID, slot, writers, outcomes, responseReady,
                       budget, published, exitDecided, exitSent, skippedExit,
                       stubborn, timedOut, sendFailed, timeoutWithTail,
                       timeoutBlocked, killed, graceful, sawPublishedTail, handoffBudget>>

AwaitOwner == /\ phase \in {"sending", "receiving", "closing", "reaped"} /\ UNCHANGED vars
ReadyFinish == Request \/ Quit
EndProcess == GracefulEnd \/ Kill
Next == ReadyFinish \/ ObserveWrite \/ WriteOutcome \/ StopWriter \/ ReceiveResponse \/ Handoff
        \/ PublishResponse \/ Deadline \/ FailDeadline \/ FailWrite \/ DecideExit
        \/ EndProcess \/ Reap \/ AwaitOwner

Spec == Init /\ [][Next]_vars
        /\ WF_vars(ReadyFinish) /\ WF_vars(Deadline) /\ WF_vars(FailDeadline)
        /\ WF_vars(FailWrite) /\ WF_vars(StopWriter) /\ WF_vars(DecideExit)
        /\ WF_vars(EndProcess) /\ WF_vars(Reap)

TypeOK == /\ phase \in {"ready", "sending", "receiving", "closing", "reaped"}
          /\ process \in {"running", "ended", "reaped"}
          /\ nextID \in 1..3 /\ slot \in 0..2 /\ writers \subseteq 1..2
          /\ outcomes \in [1..2 -> {"none", "ok", "failed"}]
          /\ responseReady \in BOOLEAN /\ budget \in {"available", "expired"}
          /\ published \in Seq([id: 1..2, quiescent: BOOLEAN]) /\ Len(published) <= 2
          /\ exitDecided \in BOOLEAN /\ exitSent \in BOOLEAN /\ skippedExit \in BOOLEAN
          /\ stubborn \in BOOLEAN /\ timedOut \in BOOLEAN /\ sendFailed \in BOOLEAN
          /\ timeoutWithTail \in BOOLEAN /\ timeoutBlocked \in BOOLEAN
          /\ killed \in BOOLEAN /\ graceful \in BOOLEAN /\ sawPublishedTail \in BOOLEAN
          /\ handoffBudget \in [read: BOOLEAN, elapsed: 0..3, allotted: 0..2]
NoExtraTailBudget == handoffBudget.read =>
                      IF handoffBudget.elapsed < 2
                      THEN handoffBudget.elapsed + handoffBudget.allotted <= 2
                      ELSE handoffBudget.allotted = 0
NoPrematureHandoff == \A i \in 1..Len(published): published[i].quiescent
NoLostSenderSlot == writers \subseteq {slot}
GracefulExitNotSkipped == exitDecided /\ ~timedOut /\ ~sendFailed => exitSent
ReapedMeansReleased == phase = "reaped" => process = "reaped"
EventuallyReaped == <> (phase = "reaped")
RequestEventuallySettles == (phase \in {"sending", "receiving"}) ~> (phase \in {"ready", "closing", "reaped"})
TailHandoffNotReached == ~(sawPublishedTail /\ Len(published) = 2 /\ graceful /\ phase = "reaped")
TailTimeoutSkipReapedNotReached == ~(timeoutWithTail /\ handoffBudget.read
                                    /\ handoffBudget.elapsed = 1 /\ handoffBudget.allotted = 1
                                    /\ skippedExit /\ killed /\ phase = "reaped")
BlockedTimeoutSkipReapedNotReached == ~(timeoutBlocked /\ skippedExit /\ killed /\ phase = "reaped")
=============================================================================
