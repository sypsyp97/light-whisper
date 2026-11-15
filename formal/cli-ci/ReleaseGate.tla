-------------------------- MODULE ReleaseGate --------------------------
EXTENDS TLC

CONSTANTS CheckLocal, CheckCli, CheckExisting, CheckAggregate, CheckSha,
          WaitBeforeTag, CheckHead, CheckRemote, CheckClean, TagBeforeRelease,
          CompleteRelease, WitnessCli

VARIABLES phase, localChecks, candidate, head, remote, dirty, runHead,
          aggregate, fiveJobs, cli, ciAccepted, tagCreated, drifted, fetched, observedHead, observedRemote, observedDirty, tagTarget

vars == <<phase, localChecks, candidate, head, remote, dirty, runHead,
          aggregate, fiveJobs, cli, ciAccepted, tagCreated, drifted, fetched, observedHead, observedRemote, observedDirty, tagTarget>>

CliTerminal == {"success", "missing", "skipped", "failure", "cancelled"}
Terminal == phase \in {"blocked", "released"}

Init == /\ phase = "checks"
        /\ localChecks = "pending"
        /\ candidate = "unset"
        /\ head = "A" /\ remote = "A" /\ dirty = FALSE
        /\ runHead = "unset"
        /\ aggregate = "pending" /\ fiveJobs = "pending" /\ cli = "pending"
        /\ ciAccepted = FALSE /\ tagCreated = FALSE /\ drifted = FALSE
        /\ fetched = FALSE /\ observedHead = "unset" /\ observedRemote = "unset"
        /\ observedDirty = FALSE /\ tagTarget = "unset"

LocalResult == /\ phase = "checks" /\ localChecks = "pending"
               /\ localChecks' \in {"success", "failure"}
               /\ UNCHANGED <<phase, candidate, head, remote, dirty, runHead,
                              aggregate, fiveJobs, cli, ciAccepted, tagCreated, drifted, fetched, observedHead, observedRemote, observedDirty, tagTarget>>

FreezeCandidate == /\ phase = "checks" /\ localChecks # "pending"
                   /\ IF ~CheckLocal \/ localChecks = "success"
                         THEN /\ phase' = "wait" /\ candidate' = "A"
                         ELSE /\ phase' = "blocked" /\ UNCHANGED candidate
                   /\ UNCHANGED <<localChecks, head, remote, dirty, runHead,
                                  aggregate, fiveJobs, cli, ciAccepted, tagCreated, drifted, fetched, observedHead, observedRemote, observedDirty, tagTarget>>

ResolveCI == /\ phase = "wait" /\ aggregate = "pending"
             /\ aggregate' \in {"success", "failure"}
             /\ fiveJobs' \in {"success", "failure"}
             /\ cli' \in CliTerminal
             /\ runHead' \in {"A", "B"}
             /\ UNCHANGED <<phase, localChecks, candidate, head, remote, dirty,
                            ciAccepted, tagCreated, drifted, fetched, observedHead, observedRemote, observedDirty, tagTarget>>

CiEligible == /\ (~CheckCli \/ cli = "success")
              /\ (~CheckExisting \/ fiveJobs = "success")
              /\ (~CheckAggregate \/ aggregate = "success")
              /\ (~CheckSha \/ runHead = candidate)

DecideCI == /\ phase = "wait" /\ aggregate # "pending"
            /\ phase' = IF CiEligible THEN "accepted" ELSE "blocked"
            /\ ciAccepted' = CiEligible
            /\ UNCHANGED <<localChecks, candidate, head, remote, dirty, runHead,
                           aggregate, fiveJobs, cli, tagCreated, drifted, fetched,
                           observedHead, observedRemote, observedDirty, tagTarget>>

\* Each Git observation and fetch are separate process boundaries. The finite
\* interference is one optional change during fetch, after the initial checks.
\* Exclusive publisher/no external writers during final observations through
\* tagging is an explicit assumption, not an atomic filesystem/Git transaction.
InitialHead == /\ phase = "accepted"
               /\ observedHead' = head
               /\ phase' = IF head = candidate THEN "initial-clean" ELSE "blocked"
               /\ UNCHANGED <<localChecks, candidate, head, remote, dirty, runHead,
                              aggregate, fiveJobs, cli, ciAccepted, tagCreated, drifted,
                              fetched, observedRemote, observedDirty, tagTarget>>

InitialClean == /\ phase = "initial-clean"
                /\ observedDirty' = dirty
                /\ phase' = IF ~dirty THEN "fetch" ELSE "blocked"
                /\ UNCHANGED <<localChecks, candidate, head, remote, dirty, runHead,
                               aggregate, fiveJobs, cli, ciAccepted, tagCreated, drifted,
                               fetched, observedHead, observedRemote, tagTarget>>

Fetch == /\ phase = "fetch"
         /\ \E change \in {"none", "head", "remote", "dirty"}:
              /\ head' = IF change = "head" THEN "B" ELSE head
              /\ remote' = IF change = "remote" THEN "B" ELSE remote
              /\ dirty' = ((change = "dirty") \/ dirty)
              /\ drifted' = (change # "none")
         /\ phase' = "remote" /\ fetched' = TRUE
         /\ UNCHANGED <<localChecks, candidate, runHead, aggregate, fiveJobs, cli,
                        ciAccepted, tagCreated, observedHead, observedRemote,
                        observedDirty, tagTarget>>

ObserveRemote == /\ phase = "remote"
                 /\ observedRemote' = IF CheckRemote THEN remote ELSE observedRemote
                 /\ phase' = IF CheckRemote /\ remote # candidate THEN "blocked" ELSE "head"
                 /\ UNCHANGED <<localChecks, candidate, head, remote, dirty, runHead,
                                aggregate, fiveJobs, cli, ciAccepted, tagCreated, drifted,
                                fetched, observedHead, observedDirty, tagTarget>>

\* The head/clean negatives omit the new post-fetch read and rejection. Their
\* initial observations stay stale, reproducing the actual baseline call path.
ObserveHead == /\ phase = "head"
               /\ observedHead' = IF CheckHead THEN head ELSE observedHead
               /\ phase' = IF CheckHead /\ head # candidate THEN "blocked" ELSE "clean"
               /\ UNCHANGED <<localChecks, candidate, head, remote, dirty, runHead,
                              aggregate, fiveJobs, cli, ciAccepted, tagCreated, drifted,
                              fetched, observedRemote, observedDirty, tagTarget>>

ObserveClean == /\ phase = "clean"
                /\ observedDirty' = IF CheckClean THEN dirty ELSE observedDirty
                /\ phase' = IF CheckClean /\ dirty THEN "blocked" ELSE "tag-ready"
                /\ UNCHANGED <<localChecks, candidate, head, remote, dirty, runHead,
                               aggregate, fiveJobs, cli, ciAccepted, tagCreated, drifted,
                               fetched, observedHead, observedRemote, tagTarget>>

Tag == /\ phase = "tag-ready"
       /\ phase' = "tagged" /\ tagCreated' = TRUE /\ tagTarget' = candidate
       /\ UNCHANGED <<localChecks, candidate, head, remote, dirty, runHead,
                      aggregate, fiveJobs, cli, ciAccepted, drifted, fetched,
                      observedHead, observedRemote, observedDirty>>

SkipWait == /\ ~WaitBeforeTag /\ phase = "wait"
            /\ phase' = "tagged" /\ tagCreated' = TRUE /\ tagTarget' = candidate
            /\ UNCHANGED <<localChecks, candidate, head, remote, dirty, runHead,
                           aggregate, fiveJobs, cli, ciAccepted, drifted, fetched,
                           observedHead, observedRemote, observedDirty>>

Publish == /\ CompleteRelease
           /\ (phase = "tagged" \/ (~TagBeforeRelease /\ phase = "accepted"))
           /\ phase' = "released"
           /\ UNCHANGED <<localChecks, candidate, head, remote, dirty, runHead,
                          aggregate, fiveJobs, cli, ciAccepted, tagCreated, drifted, fetched, observedHead, observedRemote, observedDirty, tagTarget>>

AwaitOrDone == UNCHANGED vars
Next == LocalResult \/ FreezeCandidate \/ ResolveCI \/ DecideCI \/ InitialHead
        \/ InitialClean \/ Fetch \/ ObserveRemote \/ ObserveHead \/ ObserveClean
        \/ Tag \/ SkipWait \/ Publish \/ AwaitOrDone
Spec == Init /\ [][Next]_vars
        /\ WF_vars(LocalResult) /\ WF_vars(FreezeCandidate)
        /\ WF_vars(ResolveCI) /\ WF_vars(DecideCI)
        /\ WF_vars(InitialHead) /\ WF_vars(InitialClean) /\ WF_vars(Fetch)
        /\ WF_vars(ObserveRemote) /\ WF_vars(ObserveHead) /\ WF_vars(ObserveClean)
        /\ WF_vars(Tag) /\ WF_vars(Publish)

TypeOK == /\ phase \in {"checks", "wait", "accepted", "initial-clean", "fetch", "remote",
                        "head", "clean", "tag-ready", "tagged", "released", "blocked"}
          /\ localChecks \in {"pending", "success", "failure"}
          /\ candidate \in {"unset", "A"}
          /\ head \in {"A", "B"} /\ remote \in {"A", "B"}
          /\ runHead \in {"unset", "A", "B"}
          /\ aggregate \in {"pending", "success", "failure"}
          /\ fiveJobs \in {"pending", "success", "failure"}
          /\ cli \in CliTerminal \cup {"pending"}
          /\ dirty \in BOOLEAN /\ ciAccepted \in BOOLEAN
          /\ tagCreated \in BOOLEAN /\ drifted \in BOOLEAN /\ fetched \in BOOLEAN
          /\ observedHead \in {"unset", "A", "B"}
          /\ observedRemote \in {"unset", "A", "B"}
          /\ observedDirty \in BOOLEAN /\ tagTarget \in {"unset", "A", "B"}

LocalSafety == candidate # "unset" => localChecks = "success"
CliSafety == ciAccepted => cli = "success"
ExistingSafety == ciAccepted => fiveJobs = "success"
AggregateSafety == ciAccepted => aggregate = "success"
ShaSafety == ciAccepted => runHead = candidate
WaitSafety == phase \in {"tagged", "released"} => ciAccepted
FinalIdentitySafety == phase \in {"tagged", "released"} =>
                       /\ fetched /\ head = candidate /\ remote = candidate /\ ~dirty
                       /\ tagTarget = candidate
TagSafety == phase = "released" => tagCreated /\ tagTarget = candidate
EventuallyDecided == <>Terminal

ReleasedNotReached == phase # "released"
BadCliBlockedNotReached == ~(phase = "blocked" /\ localChecks = "success"
                             /\ runHead = candidate /\ aggregate = "success"
                             /\ fiveJobs = "success" /\ cli = WitnessCli)
DriftBlockedNotReached == ~(phase = "blocked" /\ ciAccepted /\ drifted)
=============================================================================
