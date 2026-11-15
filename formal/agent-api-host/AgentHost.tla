--------------------------- MODULE AgentHost ---------------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANT Broken
Clients == {1, 2}
Engines == {"A", "B"}
VARIABLES host, endpoint, hostLease, clients, used, externalBusy,
          phase, owner, native, configured, runtime, requested, generation,
          ticket, nativeLease, copy, starts, transcriptions, effects,
          duplicateTried, busyRejected, invalidRejected, clientKilledHost,
          rejectionBefore, rejectionAfter, stalePublished
EffectFrame == <<native, configured, runtime, effects, starts, transcriptions,
                 nativeLease, copy, phase, owner, generation, ticket, requested>>
EffectFrames == {"stopped", "starting", "running"} \X Engines
    \X (Engines \cup {"unknown"}) \X (0..4) \X (0..2) \X (0..1)
    \X BOOLEAN \X BOOLEAN \X {"none", "starting", "transcribing"}
    \X (Clients \cup {0}) \X (0..1) \X (0..1) \X Engines
vars == <<host, endpoint, hostLease, clients, used, externalBusy,
          phase, owner, native, configured, runtime, requested, generation,
          ticket, nativeLease, copy, starts, transcriptions, effects,
          duplicateTried, busyRejected, invalidRejected, clientKilledHost,
          rejectionBefore, rejectionAfter, stalePublished>>

Init ==
    /\ host = "boot" /\ endpoint = FALSE /\ hostLease = FALSE
    /\ clients = [c \in Clients |-> "new"] /\ used = {}
    /\ externalBusy \in {"none", "recording", "processing"}
    /\ phase = "none" /\ owner = 0 /\ native = "stopped"
    /\ configured = "A" /\ runtime = "unknown" /\ requested = "A"
    /\ generation = 0 /\ ticket = 0 /\ nativeLease = FALSE /\ copy = FALSE
    /\ starts = 0 /\ transcriptions = 0 /\ effects = 0
    /\ duplicateTried = FALSE /\ busyRejected = FALSE /\ invalidRejected = FALSE
    /\ clientKilledHost = FALSE /\ stalePublished = FALSE
    /\ rejectionBefore = EffectFrame /\ rejectionAfter = EffectFrame

ClientReady(c) ==
    /\ clients[c] = "new"
    /\ clients' = [clients EXCEPT ![c] = "ready"]
    /\ UNCHANGED <<host, endpoint, hostLease, used, externalBusy, phase, owner,
          native, configured, runtime, requested, generation, ticket, nativeLease,
          copy, starts, transcriptions, effects, duplicateTried, busyRejected,
          invalidRejected, clientKilledHost, rejectionBefore, rejectionAfter, stalePublished>>

HostReady ==
    /\ host = "boot"
    /\ host' = "ready" /\ endpoint' = TRUE /\ hostLease' = TRUE
    /\ UNCHANGED <<clients, used, externalBusy, phase, owner, native, configured,
          runtime, requested, generation, ticket, nativeLease, copy, starts,
          transcriptions, effects, duplicateTried, busyRejected, invalidRejected,
          clientKilledHost, rejectionBefore, rejectionAfter, stalePublished>>

DuplicateHost ==
    /\ host = "ready" /\ ~duplicateTried
    /\ duplicateTried' = TRUE
    /\ hostLease' = IF Broken = "lease" THEN FALSE ELSE hostLease
    /\ UNCHANGED <<host, endpoint, clients, used, externalBusy, phase, owner,
          native, configured, runtime, requested, generation, ticket, nativeLease,
          copy, starts, transcriptions, effects, busyRejected, invalidRejected,
          clientKilledHost, rejectionBefore, rejectionAfter, stalePublished>>

ClientExit(c) ==
    /\ clients[c] = "ready"
    /\ clients' = [clients EXCEPT ![c] = "closed"]
    /\ host' = IF Broken = "client_exit" /\ host = "ready" THEN "draining" ELSE host
    /\ clientKilledHost' = (clientKilledHost \/ (Broken = "client_exit" /\ host = "ready"))
    /\ nativeLease' = IF Broken = "disconnect" /\ owner = c /\ phase # "none"
                         THEN FALSE ELSE nativeLease
    /\ UNCHANGED <<endpoint, hostLease, used, externalBusy, phase, owner, native,
          configured, runtime, requested, generation, ticket, copy, starts,
          transcriptions, effects, duplicateTried, busyRejected, invalidRejected,
          rejectionBefore, rejectionAfter, stalePublished>>

ClearExternalBusy ==
    /\ externalBusy # "none" /\ externalBusy' = "none"
    /\ UNCHANGED <<host, endpoint, hostLease, clients, used, phase, owner, native,
          configured, runtime, requested, generation, ticket, nativeLease, copy,
          starts, transcriptions, effects, duplicateTried, busyRejected,
          invalidRejected, clientKilledHost, rejectionBefore, rejectionAfter, stalePublished>>

BusyReject(c) ==
    /\ host = "ready" /\ clients[c] = "ready"
    /\ (externalBusy # "none" \/ phase # "none") /\ ~busyRejected
    /\ busyRejected' = TRUE
    /\ rejectionBefore' = EffectFrame
    /\ native' = IF Broken = "busy" THEN (IF native = "stopped" THEN "starting" ELSE "stopped") ELSE native
    /\ runtime' = IF Broken = "busy" THEN "unknown" ELSE runtime
    /\ configured' = configured
    /\ effects' = IF Broken = "busy" THEN effects + 1 ELSE effects
    /\ UNCHANGED <<host, endpoint, hostLease, clients, used, externalBusy, phase,
          owner, requested, generation, ticket, nativeLease, copy, starts,
          transcriptions, duplicateTried, invalidRejected, clientKilledHost,
          stalePublished>>
    /\ rejectionAfter' = EffectFrame'

InvalidReject(c) ==
    /\ host = "ready" /\ clients[c] = "ready" /\ ~invalidRejected
    /\ invalidRejected' = TRUE
    /\ rejectionBefore' = EffectFrame
    /\ native' = IF Broken = "invalid" THEN "stopped" ELSE native
    /\ runtime' = IF Broken = "invalid" THEN "unknown" ELSE runtime
    /\ configured' = IF Broken = "invalid" THEN (IF configured = "A" THEN "B" ELSE "A") ELSE configured
    /\ effects' = IF Broken = "invalid" THEN effects + 1 ELSE effects
    /\ UNCHANGED <<host, endpoint, hostLease, clients, used, externalBusy, phase,
          owner, requested, generation, ticket, nativeLease, copy, starts,
          transcriptions, duplicateTried, busyRejected, clientKilledHost,
          stalePublished>>
    /\ rejectionAfter' = EffectFrame'

SelectEngine(c) ==
    /\ host = "ready" /\ clients[c] = "ready" /\ c \notin used
    /\ externalBusy = "none" /\ phase = "none"
    /\ used' = used \cup {c}
    /\ configured' = IF configured = "A" THEN "B" ELSE "A"
    /\ native' = "stopped" /\ runtime' = "unknown" /\ effects' = effects + 1
    /\ UNCHANGED <<host, endpoint, hostLease, clients, externalBusy, phase, owner,
          requested, generation, ticket, nativeLease, copy, starts,
          transcriptions, duplicateTried, busyRejected, invalidRejected,
          clientKilledHost, rejectionBefore, rejectionAfter, stalePublished>>

BeginStart(c) ==
    /\ host = "ready" /\ clients[c] = "ready" /\ c \notin used
    /\ externalBusy = "none" /\ phase = "none" /\ starts < 2
    /\ used' = used \cup {c} /\ starts' = starts + 1 /\ effects' = effects + 1
    /\ phase' = "starting" /\ owner' = c /\ nativeLease' = TRUE
    /\ native' = "starting" /\ runtime' = "unknown"
    /\ requested' = configured /\ ticket' = generation
    /\ UNCHANGED <<host, endpoint, hostLease, clients, externalBusy, configured,
          generation, copy, transcriptions, duplicateTried, busyRejected,
          invalidRejected, clientKilledHost, rejectionBefore, rejectionAfter, stalePublished>>

FinishStart ==
    /\ phase = "starting"
    /\ LET current == host = "ready" /\ generation = ticket
           publish == current \/ Broken = "stale"
       IN /\ native' = IF publish THEN "running" ELSE "stopped"
          /\ runtime' = IF ~publish THEN "unknown"
                         ELSE IF Broken = "identity" THEN (IF requested = "A" THEN "B" ELSE "A")
                         ELSE requested
          /\ stalePublished' = (stalePublished \/ (~current /\ publish))
    /\ phase' = "none" /\ owner' = 0 /\ nativeLease' = FALSE
    /\ UNCHANGED <<host, endpoint, hostLease, clients, used, externalBusy,
          configured, requested, generation, ticket, copy, starts, transcriptions,
          effects, duplicateTried, busyRejected, invalidRejected, clientKilledHost,
          rejectionBefore, rejectionAfter>>

BeginTranscribe(c) ==
    /\ host = "ready" /\ clients[c] = "ready" /\ c \notin used
    /\ externalBusy = "none" /\ phase = "none" /\ native = "running"
    /\ transcriptions = 0
    /\ used' = used \cup {c} /\ phase' = "transcribing" /\ owner' = c
    /\ nativeLease' = TRUE /\ copy' = TRUE /\ effects' = effects + 1
    /\ UNCHANGED <<host, endpoint, hostLease, clients, externalBusy, native,
          configured, runtime, requested, generation, ticket, starts,
          transcriptions, duplicateTried, busyRejected, invalidRejected,
          clientKilledHost, rejectionBefore, rejectionAfter, stalePublished>>

FinishTranscribe ==
    /\ phase = "transcribing" /\ host = "ready" /\ Broken # "finish"
    /\ phase' = "none" /\ owner' = 0 /\ nativeLease' = FALSE
    /\ copy' = IF Broken = "audio" THEN copy ELSE FALSE
    /\ transcriptions' = transcriptions + 1
    /\ UNCHANGED <<host, endpoint, hostLease, clients, used, externalBusy, native,
          configured, runtime, requested, generation, ticket, starts, effects,
          duplicateTried, busyRejected, invalidRejected, clientKilledHost,
          rejectionBefore, rejectionAfter, stalePublished>>

OwnerQuit ==
    /\ host = "ready" /\ host' = "draining" /\ generation' = generation + 1
    /\ UNCHANGED <<endpoint, hostLease, clients, used, externalBusy, phase, owner,
          native, configured, runtime, requested, ticket, nativeLease, copy,
          starts, transcriptions, effects, duplicateTried, busyRejected,
          invalidRejected, clientKilledHost, rejectionBefore, rejectionAfter, stalePublished>>

CancelOperation ==
    /\ host = "draining" /\ phase # "none" /\ Broken # "cancel"
    /\ phase' = "none" /\ owner' = 0 /\ nativeLease' = FALSE /\ copy' = FALSE
    /\ UNCHANGED <<host, endpoint, hostLease, clients, used, externalBusy, native,
          configured, runtime, requested, generation, ticket, starts,
          transcriptions, effects, duplicateTried, busyRejected, invalidRejected,
          clientKilledHost, rejectionBefore, rejectionAfter, stalePublished>>

StopServer ==
    /\ host = "draining" /\ phase = "none" /\ endpoint
    /\ endpoint' = FALSE
    /\ UNCHANGED <<host, hostLease, clients, used, externalBusy, phase, owner,
          native, configured, runtime, requested, generation, ticket, nativeLease,
          copy, starts, transcriptions, effects, duplicateTried, busyRejected,
          invalidRejected, clientKilledHost, rejectionBefore, rejectionAfter, stalePublished>>

StopBackend ==
    /\ host = "draining" /\ ~endpoint /\ phase = "none" /\ native # "stopped"
    /\ native' = "stopped" /\ runtime' = "unknown"
    /\ UNCHANGED <<host, endpoint, hostLease, clients, used, externalBusy, phase,
          owner, configured, requested, generation, ticket, nativeLease, copy,
          starts, transcriptions, effects, duplicateTried, busyRejected,
          invalidRejected, clientKilledHost, rejectionBefore, rejectionAfter, stalePublished>>

ReleaseHost ==
    /\ host = "draining" /\ ~endpoint /\ phase = "none"
    /\ (native = "stopped" \/ Broken = "release")
    /\ host' = "stopped" /\ hostLease' = FALSE
    /\ UNCHANGED <<endpoint, clients, used, externalBusy, phase, owner, native,
          configured, runtime, requested, generation, ticket, nativeLease, copy,
          starts, transcriptions, effects, duplicateTried, busyRejected,
          invalidRejected, clientKilledHost, rejectionBefore, rejectionAfter, stalePublished>>

Next == HostReady \/ DuplicateHost \/ ClearExternalBusy \/ FinishStart
     \/ FinishTranscribe \/ OwnerQuit \/ CancelOperation \/ StopServer
     \/ StopBackend \/ ReleaseHost
     \/ (\E c \in Clients : ClientReady(c) \/ ClientExit(c) \/ BusyReject(c)
          \/ InvalidReject(c) \/ SelectEngine(c) \/ BeginStart(c) \/ BeginTranscribe(c))
     \* Waiting on the external native pipe is legal, including the deliberate
     \* missing-cancel control. Fairness, not disabled deadlock checks, proves progress.
     \/ (phase # "none" /\ UNCHANGED vars)
     \/ (host = "stopped" /\ UNCHANGED vars)
Spec == Init /\ [][Next]_vars /\ WF_vars(FinishStart) /\ WF_vars(FinishTranscribe)
        /\ WF_vars(CancelOperation) /\ WF_vars(StopServer)
        /\ WF_vars(StopBackend) /\ WF_vars(ReleaseHost)

TypeOK ==
    /\ host \in {"boot", "ready", "draining", "stopped"}
    /\ endpoint \in BOOLEAN /\ hostLease \in BOOLEAN
    /\ clients \in [Clients -> {"new", "ready", "closed"}] /\ used \subseteq Clients
    /\ externalBusy \in {"none", "recording", "processing"}
    /\ phase \in {"none", "starting", "transcribing"} /\ owner \in Clients \cup {0}
    /\ native \in {"stopped", "starting", "running"}
    /\ configured \in Engines /\ runtime \in Engines \cup {"unknown"}
    /\ requested \in Engines /\ generation \in 0..1 /\ ticket \in 0..1
    /\ starts \in 0..2 /\ transcriptions \in 0..1 /\ effects \in 0..4
    /\ \A b \in {nativeLease, copy, duplicateTried, busyRejected, invalidRejected,
                 clientKilledHost, stalePublished}: b \in BOOLEAN
    /\ rejectionBefore \in EffectFrames
    /\ rejectionAfter \in EffectFrames
EndpointHasOneOwner == endpoint => hostLease
AdmittedOperationRetainsLease == phase # "none" => nativeLease
ClientExitIsLocal == ~clientKilledHost
BusyAndInvalidHaveNoEffects == rejectionBefore = rejectionAfter
RetiredStartCannotPublish == ~stalePublished
OwnedAudioLifetime == copy <=> phase = "transcribing"
StoppedIsClean == host = "stopped" =>
    (~endpoint /\ ~hostLease /\ phase = "none" /\ ~nativeLease /\ ~copy /\ native = "stopped")
ObservedIdentityIsActual == native = "running" => runtime = requested
ShutdownProgress == host = "draining" ~> host = "stopped"
OperationProgress == phase # "none" ~> phase = "none"

NoDisconnectedPending == ~(phase = "transcribing" /\ clients[owner] = "closed")
NoStartupRetirement == ~(host = "draining" /\ phase = "starting")
NoBusyRejection == ~busyRejected
NoRestartAfterInvalidRejection == ~(invalidRejected /\ starts = 2)
NoDuplicateAttempt == ~duplicateTried
NoCompletion == transcriptions = 0
NoClientOnlyReady == ~(host = "boot" /\ \E c \in Clients : clients[c] = "ready")
NoCleanShutdown == host # "stopped"
=============================================================================
