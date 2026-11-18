--------------------------- MODULE MainPolishStatus ---------------------------
EXTENDS Naturals, TLC

CONSTANTS Sessions, MaxRevision, Mode

VARIABLES current, revision, phase, stage, finalId, closed, audit

Vars == <<current, revision, phase, stage, finalId, closed, audit>>
Ids == 1..Sessions
Phases == {"idle", "recording", "dictation", "assistant", "complete"}
Stages == {"none", "polishing", "finishing"}
Signals == {"polishing", "fallback", "applied", "unchanged", "error"}
Trace(kind, signal, owner) ==
  [kind |-> kind, signal |-> signal, owner |-> owner, stage |-> stage]

Init ==
  /\ current = 0
  /\ revision = 0
  /\ phase = "idle"
  /\ stage = "none"
  /\ finalId = 0
  /\ closed = {}
  /\ audit = [kind |-> "init", signal |-> "applied", owner |-> 0, stage |-> "none"]

Owner == IF phase = "dictation" /\ current > finalId THEN current ELSE 0

Recording(id, rev, nextPhase) ==
  /\ id >= current
  /\ IF id > current THEN nextPhase \in {"recording", "dictation", "assistant"}
      ELSE /\ rev > revision
           /\ CASE phase = "recording" -> nextPhase \in {"recording", "dictation", "assistant", "complete"}
                [] phase \in {"dictation", "assistant"} -> nextPhase \in {phase, "complete"}
                [] OTHER -> FALSE
  /\ current' = id
  /\ revision' = rev
  /\ phase' = nextPhase
  /\ stage' = IF stage # "none" /\ Mode = "preserve-reset" THEN stage
               ELSE IF id = current /\ phase = nextPhase THEN stage ELSE "none"
  /\ audit' = Trace("recording", "applied", id)
  /\ UNCHANGED <<finalId, closed>>

Polish(id, signal) ==
  /\ LET owns == Owner # 0 /\ id = Owner
         active == signal \in {"polishing", "fallback"}
         terminal == signal \in {"applied", "unchanged"}
         canSet == owns \/ (Mode = "foreign-event" /\ phase = "dictation" /\ current > finalId)
         finish == IF Mode = "ignore-terminal" THEN stage ELSE "finishing"
         reopen == active /\ (stage # "finishing" \/ Mode = "reopen")
     IN /\ stage' = IF ~canSet THEN stage
                    ELSE IF terminal \/ (signal = "error" /\ Mode = "error-finishes") THEN finish
                    ELSE IF reopen THEN "polishing" ELSE stage
        /\ closed' = IF owns /\ terminal THEN closed \cup {id} ELSE closed
        /\ audit' = Trace(IF owns THEN "own-signal" ELSE "foreign-signal", signal, id)
  /\ UNCHANGED <<current, revision, phase, finalId>>

Final(id) ==
  /\ id >= current
  /\ id >= finalId
  /\ finalId' = id
  /\ stage' = IF stage = "polishing" /\ Mode # "ignore-final" THEN "finishing" ELSE stage
  /\ audit' = Trace("final", "applied", id)
  /\ UNCHANGED <<current, revision, phase, closed>>

Next ==
  \/ \E id \in Ids, rev \in 1..MaxRevision, nextPhase \in Phases : Recording(id, rev, nextPhase)
  \/ \E id \in 0..Sessions, signal \in Signals : Polish(id, signal)
  \/ \E id \in Ids : Final(id)

Spec == Init /\ [][Next]_Vars

TypeOK ==
  /\ current \in 0..Sessions
  /\ revision \in 0..MaxRevision
  /\ phase \in Phases
  /\ stage \in Stages
  /\ finalId \in 0..Sessions
  /\ closed \subseteq Ids
  /\ audit \in [kind : {"init", "recording", "own-signal", "foreign-signal", "final"},
                 signal : Signals, owner : 0..Sessions, stage : Stages]

OnlyOwnerChangesLabel ==
  audit.kind = "foreign-signal" => stage = audit.stage

ClearsOnPhaseExit ==
  phase # "dictation" => stage = "none"

RecoverableErrorKeepsStage ==
  audit.kind = "own-signal" /\ audit.signal = "error" => stage = audit.stage

DoesNotReopen ==
  current \in closed => stage # "polishing"

FinalClearsPolishing ==
  current <= finalId => stage # "polishing"

NoFallbackAfterAppliedWitness ==
  ~(audit.kind = "own-signal" /\ audit.signal = "fallback" /\ current \in closed)

NoPolishWitness == stage # "polishing"

=============================================================================
