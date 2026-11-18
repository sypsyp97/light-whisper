----------------------------- MODULE SubtitleWaveform -----------------------------
EXTENDS Naturals, TLC

CONSTANTS Mounts, Sessions, Mode

VARIABLES generation, mounted, listeners, latestSession, session, phase, version, level, sample, audit

Vars == <<generation, mounted, listeners, latestSession, session, phase, version, level, sample, audit>>
Generations == 1..Mounts
SessionIds == 1..Sessions
Active == {"starting", "recording"}
Phases == {"idle", "starting", "recording", "processing"}
ListenerStates == {"unused", "pending", "listening", "retiring", "released"}
NoSample == [generation |-> 0, session |-> 0]
Trace(kind, valid, suppliedGeneration, suppliedSession) ==
  [kind |-> kind, valid |-> valid, version |-> version, level |-> level,
   generation |-> suppliedGeneration, session |-> suppliedSession, previousSession |-> session]

Init ==
  /\ generation = 0
  /\ mounted = FALSE
  /\ listeners = [id \in Generations |-> "unused"]
  /\ latestSession = 0
  /\ session = 0
  /\ phase = "idle"
  /\ version = FALSE
  /\ level = 0
  /\ sample = NoSample
  /\ audit = [kind |-> "init", valid |-> FALSE, version |-> FALSE, level |-> 0,
              generation |-> 0, session |-> 0, previousSession |-> 0]

Mount ==
  /\ ~mounted
  /\ generation < Mounts
  /\ generation' = generation + 1
  /\ mounted' = TRUE
  /\ listeners' = [listeners EXCEPT ![generation'] = "pending"]
  /\ latestSession' = 0
  /\ session' = 0
  /\ phase' = "idle"
  /\ level' = 0
  /\ sample' = NoSample
  /\ audit' = Trace("mount", FALSE, generation', 0)
  /\ version' = ~version

Acknowledge(id) ==
  /\ listeners[id] \in {"pending", "retiring"}
  /\ listeners' = [listeners EXCEPT ![id] =
       IF listeners[id] = "pending" \/ Mode = "late-leak" THEN "listening" ELSE "released"]
  /\ audit' = Trace("ack", FALSE, id, 0)
  /\ UNCHANGED <<generation, mounted, latestSession, session, phase, version, level, sample>>

Unmount ==
  /\ mounted
  /\ mounted' = FALSE
  /\ listeners' = [listeners EXCEPT ![generation] =
       IF listeners[generation] = "pending" THEN "retiring" ELSE "released"]
  /\ latestSession' = 0
  /\ session' = 0
  /\ phase' = "idle"
  /\ level' = 0
  /\ sample' = NoSample
  /\ audit' = Trace("unmount", FALSE, generation, 0)
  /\ version' = ~version
  /\ UNCHANGED generation

Interim(id) ==
  /\ mounted
  /\ id > latestSession
  /\ latestSession' = id
  /\ audit' = Trace("interim", FALSE, generation, id)
  /\ version' = ~version
  /\ UNCHANGED <<generation, mounted, listeners, session, phase, level, sample>>

Lifecycle(id, nextPhase) ==
  /\ mounted
  /\ id >= latestSession
  /\ IF id > session THEN nextPhase \in Active
      ELSE \/ phase = "starting" /\ nextPhase \in {"recording", "processing", "idle"}
           \/ phase = "recording" /\ nextPhase \in {"processing", "idle"}
           \/ phase = "processing" /\ nextPhase = "idle"
  /\ latestSession' = id
  /\ phase' = nextPhase
  /\ LET afterInterim == id = latestSession /\ id > session
         reset == session' # session \/ nextPhase \notin Active
     IN /\ session' = IF afterInterim /\ Mode = "late-state" THEN session ELSE id
        /\ level' = IF reset /\ Mode # "no-reset" THEN 0 ELSE level
        /\ sample' = IF reset /\ Mode # "no-reset" THEN NoSample ELSE sample
        /\ audit' = Trace(IF afterInterim THEN "lifecycle-after-interim" ELSE "lifecycle",
                          FALSE, generation, id)
  /\ version' = ~version
  /\ UNCHANGED <<generation, mounted, listeners>>

Receive(id, source, value) ==
  /\ listeners[id] # "unused"
  /\ LET isCurrent == mounted /\ id = generation
         isActive == phase \in Active
         isOwner == source = latestSession /\ source = session /\ source # 0
         valid == isCurrent /\ isActive /\ isOwner
         update == (isCurrent \/ Mode = "stale-listener")
                   /\ (isActive \/ Mode = "inactive-phase")
                   /\ (isOwner \/ Mode = "wrong-session")
     IN /\ level' = IF update THEN value ELSE level
        /\ sample' = IF update THEN [generation |-> id, session |-> source] ELSE sample
        /\ version' = IF valid /\ Mode = "parent-render" THEN ~version ELSE version
        /\ audit' = Trace("sample", valid, id, source)
  /\ UNCHANGED <<generation, mounted, listeners, latestSession, session, phase>>

Quiescent ==
  /\ ~mounted
  /\ generation = Mounts
  /\ \A id \in Generations : listeners[id] = "released"
  /\ UNCHANGED Vars

Next ==
  \/ Mount
  \/ \E id \in Generations : Acknowledge(id)
  \/ Unmount
  \/ \E id \in SessionIds : Interim(id)
  \/ \E id \in SessionIds, nextPhase \in Phases : Lifecycle(id, nextPhase)
  \/ \E id \in Generations, source \in 0..Sessions, value \in {0, 1} : Receive(id, source, value)
  \/ Quiescent

Spec ==
  /\ Init
  /\ [][Next]_Vars
  /\ \A id \in Generations : WF_Vars(Acknowledge(id))

TypeOK ==
  /\ generation \in 0..Mounts
  /\ mounted \in BOOLEAN
  /\ listeners \in [Generations -> ListenerStates]
  /\ latestSession \in 0..Sessions
  /\ session \in 0..latestSession
  /\ phase \in Phases
  /\ version \in BOOLEAN
  /\ level \in {0, 1}
  /\ sample \in [generation : 0..Mounts, session : 0..Sessions]
  /\ audit \in [kind : {"init", "mount", "ack", "unmount", "interim", "lifecycle",
                        "lifecycle-after-interim", "sample"},
                 valid : BOOLEAN, version : BOOLEAN, level : {0, 1},
                 generation : 0..Mounts, session : 0..Sessions, previousSession : 0..Sessions]

ActiveLifecycleOwnsSamples ==
  audit.kind \in {"lifecycle", "lifecycle-after-interim"} /\ phase \in Active =>
    session = latestSession

OwnedLevels ==
  /\ ActiveLifecycleOwnsSamples
  /\ level # 0 => sample # NoSample
  /\ sample # NoSample =>
       mounted /\ phase \in Active /\ sample.generation = generation /\ sample.session = session

IsolatedRender ==
  audit.kind = "sample" => version = audit.version

UnownedCallbackIgnored ==
  audit.kind = "sample" /\ ~audit.valid => level = audit.level

RetiredReleased ==
  \A id \in Generations :
    listeners[id] \in {"unused", "retiring", "released"} \/ (mounted /\ id = generation)

EventuallyReleased ==
  \A id \in Generations : listeners[id] = "retiring" ~> listeners[id] = "released"

NoEarlyInterimLifecycleWitness ==
  ~(audit.kind = "lifecycle-after-interim" /\ phase = "recording" /\ audit.previousSession > 0
    /\ session = latestSession)

NoCallbackAfterRemountWitness ==
  ~(audit.kind = "sample" /\ ~audit.valid /\ mounted /\ phase = "recording"
    /\ generation = Mounts /\ audit.generation < generation /\ audit.session = session)

NoLateAcknowledgementWitness ==
  ~(audit.kind = "ack" /\ audit.generation < generation)

===============================================================================
