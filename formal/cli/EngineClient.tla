--------------------------- MODULE EngineClient ---------------------------
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS Correlate, RejectFailedStartup, WaitForProcess, EnableKill, EnableReap,
          ProtectReaderSetup, ResilientCleanup, SkipUnstartedJoins

VARIABLES phase, process, startupOK, startupRejected, nextID, expected,
          envelope, published, exitSent, stubborn, sawNoise, sawStale, timedOut, lifecycle

vars == <<phase, process, startupOK, startupRejected, nextID, expected,
          envelope, published, exitSent, stubborn, sawNoise, sawStale, timedOut, lifecycle>>

Init == /\ phase = "new" /\ process = "absent"
        /\ startupOK = FALSE /\ startupRejected = FALSE
        /\ nextID = 1 /\ expected = 0 /\ envelope = 0 /\ published = <<>>
        /\ exitSent = FALSE /\ stubborn \in BOOLEAN
        /\ sawNoise = FALSE /\ sawStale = FALSE /\ timedOut = FALSE
        /\ lifecycle = [readerStarted |-> FALSE, readerFailed |-> FALSE,
                         cleanupFailed |-> FALSE, workerUnstarted |-> FALSE,
                         joined |-> FALSE, badJoin |-> FALSE]

Spawn == /\ phase = "new" /\ phase' = "acquired" /\ process' = "running"
         /\ UNCHANGED <<startupOK, startupRejected, nextID, expected, envelope,
                        published, exitSent, stubborn, sawNoise, sawStale, timedOut, lifecycle>>


StartReader == /\ phase = "acquired" /\ phase' = "starting"
               /\ lifecycle' = [lifecycle EXCEPT !.readerStarted = TRUE]
               /\ UNCHANGED <<process, startupOK, startupRejected, nextID, expected,
                              envelope, published, exitSent, stubborn, sawNoise, sawStale, timedOut>>

ReaderSetupFailure == /\ phase = "acquired"
                      /\ phase' = IF ProtectReaderSetup THEN "closing" ELSE "abandoned"
                      /\ lifecycle' \in {
                           [lifecycle EXCEPT !.readerFailed = TRUE, !.workerUnstarted = FALSE],
                           [lifecycle EXCEPT !.readerFailed = TRUE, !.workerUnstarted = TRUE]}
                      /\ UNCHANGED <<process, startupOK, startupRejected, nextID, expected,
                                     envelope, published, exitSent, stubborn, sawNoise, sawStale, timedOut>>

CleanupFailure == /\ phase = "closing" /\ ~lifecycle.cleanupFailed /\ ~lifecycle.joined
                  /\ phase' = IF ResilientCleanup THEN "closing" ELSE "abandoned"
                  /\ lifecycle' \in IF process = "running" THEN {
                       [lifecycle EXCEPT !.cleanupFailed = TRUE],
                       [lifecycle EXCEPT !.cleanupFailed = TRUE, !.workerUnstarted = TRUE]}
                     ELSE {[lifecycle EXCEPT !.cleanupFailed = TRUE]}
                  /\ UNCHANGED <<process, startupOK, startupRejected, nextID, expected,
                                 envelope, published, exitSent, stubborn, sawNoise, sawStale, timedOut>>

JoinWorkers == /\ phase = "closing" /\ process = "ended" /\ ~lifecycle.joined
               /\ lifecycle' = [lifecycle EXCEPT !.joined = TRUE,
                      !.badJoin = lifecycle.workerUnstarted /\ ~SkipUnstartedJoins]
               /\ UNCHANGED <<phase, process, startupOK, startupRejected, nextID, expected,
                              envelope, published, exitSent, stubborn, sawNoise, sawStale, timedOut>>

Noise == /\ phase \in {"starting", "waiting"} /\ ~sawNoise
         /\ sawNoise' = TRUE
         /\ UNCHANGED <<phase, process, startupOK, startupRejected, nextID,
                        expected, envelope, published, exitSent, stubborn,
                        sawStale, timedOut, lifecycle>>

StartupSuccess == /\ phase = "starting" /\ phase' = "ready" /\ startupOK' = TRUE
                  /\ UNCHANGED <<process, startupRejected, nextID, expected,
                     envelope, published, exitSent, stubborn, sawNoise, sawStale, timedOut, lifecycle>>

StartupFalse == /\ phase = "starting"
                /\ phase' = IF RejectFailedStartup THEN "closing" ELSE "ready"
                /\ startupRejected' = TRUE
                /\ UNCHANGED <<process, startupOK, nextID, expected, envelope,
                     published, exitSent, stubborn, sawNoise, sawStale, timedOut, lifecycle>>

EOF == /\ phase \in {"starting", "waiting"}
       /\ phase' = "closing" /\ process' \in {"running", "ended"}
       /\ UNCHANGED <<startupOK, startupRejected, nextID, expected, envelope,
                      published, exitSent, stubborn, sawNoise, sawStale, timedOut, lifecycle>>

Timeout == /\ phase \in {"starting", "waiting"}
           /\ phase' = "closing" /\ timedOut' = TRUE
           /\ UNCHANGED <<process, startupOK, startupRejected, nextID, expected,
                          envelope, published, exitSent, stubborn, sawNoise, sawStale, lifecycle>>

Request == /\ phase = "ready" /\ nextID <= 2
           /\ phase' = "waiting" /\ expected' = nextID /\ nextID' = nextID + 1
           /\ envelope' = 0
           /\ UNCHANGED <<process, startupOK, startupRejected, published,
                          exitSent, stubborn, sawNoise, sawStale, timedOut, lifecycle>>

ReceiveMatching == /\ phase = "waiting" /\ envelope # expected
                   /\ envelope' = expected
                   /\ UNCHANGED <<phase, process, startupOK, startupRejected, nextID,
                      expected, published, exitSent, stubborn, sawNoise, sawStale, timedOut, lifecycle>>

ReceiveStale == /\ phase = "waiting" /\ envelope # 3
                /\ envelope' = 3 /\ sawStale' = TRUE
                /\ UNCHANGED <<phase, process, startupOK, startupRejected, nextID,
                   expected, published, exitSent, stubborn, sawNoise, timedOut, lifecycle>>

Publish == /\ phase = "waiting" /\ envelope # 0
           /\ (~Correlate \/ envelope = expected)
           /\ phase' = "ready"
           /\ published' = Append(published, [wanted |-> expected, got |-> envelope])
           /\ UNCHANGED <<process, startupOK, startupRejected, nextID, expected,
                          envelope, exitSent, stubborn, sawNoise, sawStale, timedOut, lifecycle>>

Quit == /\ phase = "ready" /\ phase' = "closing"
        /\ UNCHANGED <<process, startupOK, startupRejected, nextID, expected,
                       envelope, published, exitSent, stubborn, sawNoise, sawStale, timedOut, lifecycle>>

SendExit == /\ phase = "closing" /\ process = "running" /\ ~exitSent /\ ~lifecycle.cleanupFailed
            /\ exitSent' = TRUE
            /\ UNCHANGED <<phase, process, startupOK, startupRejected, nextID,
                           expected, envelope, published, stubborn, sawNoise, sawStale, timedOut, lifecycle>>

GracefulEnd == /\ phase = "closing" /\ process = "running" /\ exitSent /\ ~stubborn
               /\ process' = "ended"
               /\ UNCHANGED <<phase, startupOK, startupRejected, nextID, expected,
                              envelope, published, exitSent, stubborn, sawNoise, sawStale, timedOut, lifecycle>>

Kill == /\ phase = "closing" /\ process = "running" /\ (exitSent \/ lifecycle.cleanupFailed) /\ EnableKill
        /\ process' = "ended"
        /\ UNCHANGED <<phase, startupOK, startupRejected, nextID, expected,
                       envelope, published, exitSent, stubborn, sawNoise, sawStale, timedOut, lifecycle>>

Reap == /\ phase = "closing" /\ EnableReap
        /\ (~WaitForProcess \/ (process = "ended" /\ lifecycle.joined))
        /\ phase' = "reaped"
        /\ process' = IF process = "ended" THEN "reaped" ELSE process
        /\ UNCHANGED <<startupOK, startupRejected, nextID, expected, envelope,
                       published, exitSent, stubborn, sawNoise, sawStale, timedOut, lifecycle>>

Terminal == /\ phase \in {"reaped", "abandoned"} /\ UNCHANGED vars
AwaitCleanup == /\ phase = "closing" /\ UNCHANGED vars
ReaderSetupFinish == StartReader \/ ReaderSetupFailure
StartupFinish == StartupSuccess \/ StartupFalse \/ EOF \/ Timeout
ReadyFinish == Request \/ Quit
WaitingFinish == Publish \/ EOF \/ Timeout
EndProcess == GracefulEnd \/ Kill
Next == Spawn \/ ReaderSetupFinish \/ CleanupFailure \/ JoinWorkers \/ Noise \/ StartupFinish \/ ReadyFinish \/ ReceiveMatching
        \/ ReceiveStale \/ WaitingFinish \/ SendExit \/ EndProcess \/ Reap \/ Terminal \/ AwaitCleanup

Spec == Init /\ [][Next]_vars
        /\ WF_vars(Spawn) /\ WF_vars(ReaderSetupFinish) /\ WF_vars(StartupFinish) /\ WF_vars(ReadyFinish)
        /\ WF_vars(WaitingFinish) /\ WF_vars(SendExit) /\ WF_vars(EndProcess)
        /\ WF_vars(JoinWorkers) /\ WF_vars(Reap)

TypeOK == /\ phase \in {"new", "acquired", "starting", "ready", "waiting", "closing", "reaped", "abandoned"}
          /\ process \in {"absent", "running", "ended", "reaped"}
          /\ startupOK \in BOOLEAN /\ startupRejected \in BOOLEAN
          /\ nextID \in 1..3 /\ expected \in 0..2 /\ envelope \in 0..3
          /\ published \in Seq([wanted: 1..2, got: 1..3]) /\ Len(published) <= 2
          /\ exitSent \in BOOLEAN /\ stubborn \in BOOLEAN
          /\ sawNoise \in BOOLEAN /\ sawStale \in BOOLEAN /\ timedOut \in BOOLEAN
          /\ lifecycle \in [readerStarted: BOOLEAN, readerFailed: BOOLEAN,
                             cleanupFailed: BOOLEAN, workerUnstarted: BOOLEAN,
                             joined: BOOLEAN, badJoin: BOOLEAN]
OnlyCorrelated == \A i \in 1..Len(published): published[i].wanted = published[i].got
ReadyRequiresStartup == phase \in {"ready", "waiting"} => startupOK
ReapedMeansReleased == phase = "reaped" => process = "reaped"
FaultRetainsOwnership == (lifecycle.readerFailed \/ lifecycle.cleanupFailed) => phase \in {"closing", "reaped"}
NoUnstartedJoin == ~lifecycle.badJoin
ReaderFailureReapedNotReached == ~(lifecycle.readerFailed /\ lifecycle.workerUnstarted /\ phase = "reaped")
CleanupFailureReapedNotReached == ~(lifecycle.cleanupFailed /\ phase = "reaped")
EventuallyReaped == <> (phase = "reaped")
StaleThenTwoResponsesNotReached == ~(sawStale /\ Len(published) = 2)
TimeoutReapedNotReached == ~(timedOut /\ phase = "reaped")
RejectedStartupReapedNotReached == ~(startupRejected /\ phase = "reaped")
=============================================================================
