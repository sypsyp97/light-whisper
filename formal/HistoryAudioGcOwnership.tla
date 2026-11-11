---------------------- MODULE HistoryAudioGcOwnership ----------------------
EXTENDS Naturals, FiniteSets
CONSTANT Wrong
VARIABLE s
vars == <<s>>
Tasks == 1..2
TaskPhases == {"idle", "leased", "inserted", "released", "failed"}
GcPhases == {"idle", "refs", "leases", "unlink", "done"}
References == (IF s.source THEN 1 ELSE 0) + Cardinality(s.replacements)
LeaseCount == Cardinality(s.leases)
CanWrite == ~s.gcLocked

Init == s = [source |-> TRUE, present |-> TRUE,
  phase |-> [task \in Tasks |-> "idle"], leases |-> {}, replacements |-> {},
  gcPhase |-> "idle", gcLocked |-> FALSE, gcAttempt |-> 0,
  observedRefs |-> 0, observedLeases |-> 0,
  sawTwoLeases |-> FALSE, sawReplacement |-> FALSE,
  sawQueryFailure |-> FALSE, sawUnlinkFailure |-> FALSE, sawCollected |-> FALSE]

\* get_for_reprocess reads the source and acquires its lease in one IMMEDIATE
\* transaction. Two independent reprocess tasks may protect the same WAV.
Acquire(task) ==
  /\ CanWrite /\ s.source /\ s.present /\ s.phase[task] = "idle"
  /\ s' = [s EXCEPT !.phase[task] = "leased", !.leases = @ \cup {task},
       !.sawTwoLeases = @ \/ LeaseCount = 1]
DeleteSource ==
  /\ CanWrite /\ s.source
  /\ s' = [s EXCEPT !.source = FALSE]
InsertReplacement(task) ==
  /\ CanWrite /\ s.phase[task] = "leased"
  /\ s' = [s EXCEPT !.phase[task] = "inserted",
       !.replacements = @ \cup {task}, !.sawReplacement = TRUE]
\* The public save_audio -> insert path can persist its first reference without
\* a reprocess lease. Keep this distinct from lease-owning reprocess completion.
PersistSavedAudio(task) ==
  /\ CanWrite /\ s.present /\ s.phase[task] = "idle"
  /\ s' = [s EXCEPT !.phase[task] = "released", !.replacements = @ \cup {task}]
Release(task) ==
  /\ CanWrite /\ s.phase[task] \in {"leased", "inserted"}
  /\ s' = [s EXCEPT !.leases = @ \ {task},
       !.phase[task] = IF @ = "inserted" THEN "released" ELSE "failed"]
DeleteReplacement(task) ==
  /\ CanWrite /\ task \in s.replacements
  /\ s' = [s EXCEPT !.replacements = @ \ {task}]

\* GC owns a database write transaction through BOTH reads AND unlink. The
\* split-read mutant removes only that exclusion, reproducing the old code.
BeginGc ==
  /\ s.gcPhase = "idle" /\ s.gcAttempt < 3
  /\ s' = [s EXCEPT !.gcPhase = "refs", !.gcAttempt = @ + 1,
       !.gcLocked = Wrong # "split-read"]
ReadReferences ==
  /\ s.gcPhase = "refs"
  /\ s' = [s EXCEPT !.observedRefs = References, !.gcPhase = "leases"]
ReadLeases ==
  /\ s.gcPhase = "leases"
  /\ s' = [s EXCEPT !.observedLeases = LeaseCount, !.gcPhase = "unlink",
       !.gcLocked = IF Wrong = "early-unlock" THEN FALSE ELSE @]
QueryFailure ==
  /\ s.gcPhase \in {"refs", "leases"}
  /\ s' = [s EXCEPT !.gcPhase = "done", !.sawQueryFailure = TRUE]
MayUnlink == s.observedRefs = 0 /\
  (s.observedLeases = 0 \/ Wrong = "ignore-lease")
Unlink ==
  /\ s.gcPhase = "unlink"
  /\ s' = [s EXCEPT !.gcPhase = "done",
       !.present = IF MayUnlink THEN FALSE ELSE @,
       !.sawCollected = @ \/ (MayUnlink /\ s.present)]
UnlinkFailure ==
  /\ s.gcPhase = "unlink" /\ MayUnlink /\ s.present
  /\ s' = [s EXCEPT !.gcPhase = "done", !.sawUnlinkFailure = TRUE]
EndGc ==
  /\ s.gcPhase = "done"
  /\ s' = [s EXCEPT !.gcPhase = "idle", !.gcLocked = FALSE]

Next == DeleteSource \/ BeginGc \/ ReadReferences \/ ReadLeases \/ QueryFailure
  \/ Unlink \/ UnlinkFailure \/ EndGc
  \/ (\E task \in Tasks: Acquire(task) \/ InsertReplacement(task) \/ PersistSavedAudio(task)
       \/ Release(task) \/ DeleteReplacement(task))
Spec == Init /\ [][Next]_vars
TypeOK ==
  /\ s.source \in BOOLEAN /\ s.present \in BOOLEAN /\ s.gcLocked \in BOOLEAN
  /\ s.phase \in [Tasks -> TaskPhases]
  /\ s.leases \subseteq Tasks /\ s.replacements \subseteq Tasks
  /\ s.gcPhase \in GcPhases /\ s.gcAttempt \in 0..3
  /\ s.observedRefs \in 0..3 /\ s.observedLeases \in 0..2
ReferencedOrLeasedAudioRetained == References > 0 \/ LeaseCount > 0 => s.present
ReprocessOwnsLease == \A task \in Tasks:
  s.phase[task] \in {"leased", "inserted"} <=> task \in s.leases
\* Intentional reachability witness, not a safety claim: three bounded GC
\* attempts can cover two leases, replacement, both I/O failures and collection.
BoundaryCoverageNotReached == ~(s.sawTwoLeases /\ s.sawReplacement /\
  s.sawQueryFailure /\ s.sawUnlinkFailure /\ s.sawCollected)
=============================================================================
