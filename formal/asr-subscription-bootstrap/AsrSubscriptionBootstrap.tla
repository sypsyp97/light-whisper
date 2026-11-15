---------------------- MODULE AsrSubscriptionBootstrap ----------------------
EXTENDS Naturals, Integers, Sequences, FiniteSets

CONSTANTS MaxWrites, MaxReads, EagerInitialRead, HydrateAfterAck, HydrateOnFailure,
          FenceGeneration, GuardDispose, ReleaseListener, ReportFailure

VARIABLE s

ReadIds == 1..MaxReads
Revisions == 0..MaxWrites
EngineAt(revision) == IF revision % 2 = 0
                       THEN "qwen3-asr-0.6b" ELSE "confucius4-r2t2"
EmptyRead == [phase |-> "Unused", generation |-> 0, revision |-> -1,
              value |-> "None", startedMounted |-> TRUE]

RequestRead(state) ==
  [state EXCEPT
    !.reads[state.nextRead] = [phase |-> "Requested",
      generation |-> state.generation, revision |-> -1,
      value |-> "None", startedMounted |-> state.mounted],
    !.nextRead = @ + 1]

Init ==
  LET initial == [mounted |-> TRUE, registration |-> "Requested",
    installed |-> FALSE, releases |-> 0, diagnostics |-> 0,
    diskRevision |-> 0, diskValue |-> EngineAt(0), emitted |-> 0,
    generation |-> 0, nextRead |-> 1,
    reads |-> [read \in ReadIds |-> EmptyRead],
    publications |-> <<>>, emissions |-> <<>>]
  IN s = IF EagerInitialRead THEN RequestRead(initial) ELSE initial

NativeInstall ==
  /\ s.registration = "Requested"
  /\ s' = [s EXCEPT !.registration = "Native", !.installed = TRUE]

JsAcknowledge ==
  /\ s.registration = "Native"
  /\ LET release == ~s.mounted /\ ReleaseListener
         after == [s EXCEPT !.registration = "Acknowledged",
           !.installed = IF release THEN FALSE ELSE @,
           !.releases = IF release THEN @ + 1 ELSE @]
         hydrate == HydrateAfterAck /\ ~EagerInitialRead
           /\ (~GuardDispose \/ (s.mounted /\ s.generation = 0))
     IN s' = IF hydrate THEN RequestRead(after) ELSE after

RegistrationFails ==
  /\ s.registration = "Requested"
  /\ LET after == [s EXCEPT !.registration = "Failed",
           !.diagnostics = IF s.mounted /\ ReportFailure THEN 1 ELSE 0]
         hydrate == HydrateOnFailure /\ ~EagerInitialRead /\ s.mounted /\ s.generation = 0
     IN s' = IF hydrate THEN RequestRead(after) ELSE after

NativeWrite ==
  /\ s.diskRevision < MaxWrites
  /\ s.emitted = s.diskRevision
  /\ s' = [s EXCEPT !.diskRevision = @ + 1,
                    !.diskValue = EngineAt(s.diskRevision + 1)]

NativeEmit ==
  /\ s.emitted < s.diskRevision
  /\ LET delivered == s.installed /\ s.mounted
         after == [s EXCEPT !.emitted = s.diskRevision,
           !.emissions = Append(@, [revision |-> s.diskRevision,
             installed |-> s.installed, registration |-> s.registration,
             mounted |-> s.mounted]),
           !.generation = IF delivered THEN @ + 1 ELSE @]
     IN s' = IF delivered THEN RequestRead(after) ELSE after

Snapshot(read) ==
  /\ s.reads[read].phase = "Requested"
  /\ s' = [s EXCEPT !.reads[read].phase = "Snapshotted",
                    !.reads[read].revision = s.diskRevision,
                    !.reads[read].value = s.diskValue]

Complete(read) ==
  /\ s.reads[read].phase = "Snapshotted"
  /\ LET publish == s.mounted /\
           (~FenceGeneration \/ s.reads[read].generation = s.generation)
         result == [read |-> read, revision |-> s.reads[read].revision,
           value |-> s.reads[read].value,
           requestedGeneration |-> s.reads[read].generation,
           completedGeneration |-> s.generation]
     IN s' = [s EXCEPT !.reads[read].phase = "Completed",
       !.publications = IF publish THEN Append(@, result) ELSE @]

Unmount ==
  /\ s.mounted
  /\ LET release == s.registration = "Acknowledged" /\ ReleaseListener
     IN s' = [s EXCEPT !.mounted = FALSE, !.generation = @ + 1,
       !.installed = IF release THEN FALSE ELSE @,
       !.releases = IF release THEN @ + 1 ELSE @]

AllReadsCompleted == \A read \in ReadIds:
  s.reads[read].phase \in {"Unused", "Completed"}
EnvironmentCompleted == s.diskRevision = MaxWrites /\ s.emitted = MaxWrites
Settled == EnvironmentCompleted /\ AllReadsCompleted
Terminal == Settled /\ s.registration \in {"Acknowledged", "Failed"}
Idle == /\ Terminal /\ UNCHANGED s

Next == NativeInstall \/ JsAcknowledge \/ RegistrationFails \/ NativeWrite
  \/ NativeEmit \/ Unmount \/ (\E read \in ReadIds: Snapshot(read) \/ Complete(read))
  \/ Idle

Spec == Init /\ [][Next]_s
  /\ WF_s(NativeInstall \/ RegistrationFails)
  /\ WF_s(JsAcknowledge)
  /\ WF_s(NativeWrite) /\ WF_s(NativeEmit)
  /\ \A read \in ReadIds: WF_s(Snapshot(read)) /\ WF_s(Complete(read))

TypeOK ==
  /\ s.mounted \in BOOLEAN /\ s.installed \in BOOLEAN
  /\ s.registration \in {"Requested", "Native", "Acknowledged", "Failed"}
  /\ s.releases \in 0..1 /\ s.diagnostics \in 0..1
  /\ s.diskRevision \in Revisions /\ s.diskValue = EngineAt(s.diskRevision)
  /\ s.emitted \in 0..s.diskRevision /\ s.generation \in 0..(MaxWrites + 1)
  /\ s.nextRead \in 1..(MaxReads + 1)
  /\ DOMAIN s.reads = ReadIds
  /\ \A read \in ReadIds:
       /\ s.reads[read].phase \in {"Unused", "Requested", "Snapshotted", "Completed"}
       /\ s.reads[read].generation \in 0..(MaxWrites + 1)
       /\ s.reads[read].revision \in {-1} \cup Revisions
       /\ s.reads[read].value \in {"None", "qwen3-asr-0.6b", "confucius4-r2t2"}
       /\ s.reads[read].startedMounted \in BOOLEAN
  /\ Len(s.publications) <= MaxReads /\ Len(s.emissions) <= MaxWrites

PublicationProvenance == \A i \in 1..Len(s.publications):
  /\ s.publications[i].revision \in Revisions
  /\ s.publications[i].value = EngineAt(s.publications[i].revision)
  /\ s.publications[i].value = s.reads[s.publications[i].read].value

PublicationGenerationOwned == \A i \in 1..Len(s.publications):
  s.publications[i].requestedGeneration = s.publications[i].completedGeneration

PublicationNeverRegresses == \A i \in 2..Len(s.publications):
  s.publications[i - 1].revision <= s.publications[i].revision

NoReadAfterDispose == \A read \in ReadIds:
  s.reads[read].phase # "Unused" => s.reads[read].startedMounted

ListenerReleasedAfterAck ==
  (~s.mounted /\ s.registration = "Acknowledged") =>
    (~s.installed /\ s.releases = 1)

FailureVisible == (s.mounted /\ s.registration = "Failed") => s.diagnostics = 1

LatestPublished == Len(s.publications) > 0 /\
  s.publications[Len(s.publications)].revision = s.diskRevision /\
  s.publications[Len(s.publications)].value = s.diskValue

SettledSavedConfig ==
  (s.mounted /\ s.registration = "Acknowledged" /\ Settled) => LatestPublished

LatestSavedEventually ==
  (s.mounted /\ s.registration = "Acknowledged" /\ EnvironmentCompleted)
    ~> (~s.mounted \/ LatestPublished)

FallbackEventuallyUsable == (s.mounted /\ s.registration = "Failed")
  ~> (~s.mounted \/ Len(s.publications) > 0)

ReadRequestsSettle == \A read \in ReadIds:
  (s.reads[read].phase \in {"Requested", "Snapshotted"})
    ~> (s.reads[read].phase = "Completed")

MissedBeforeInstall == \E i \in 1..Len(s.emissions):
  ~s.emissions[i].installed /\ s.emissions[i].registration = "Requested"
PreAckDelivery == \E i \in 1..Len(s.emissions):
  s.emissions[i].installed /\ s.emissions[i].registration = "Native"
StaleCompletion == \E read \in ReadIds:
  s.reads[read].phase = "Completed" /\ s.reads[read].generation < s.generation

NoRecoveredMissedEvent == ~(MissedBeforeInstall /\ s.mounted /\
  s.registration = "Acknowledged" /\ Settled /\ LatestPublished)
NoPreAckDelivery == ~PreAckDelivery
NoStaleCompletion == ~(s.mounted /\ StaleCompletion /\ LatestPublished)
NoFallbackHydration == ~(s.mounted /\ s.registration = "Failed"
  /\ Len(s.publications) > 0)
NoLateRelease == ~(~s.mounted /\ s.registration = "Acknowledged" /\ s.releases = 1)
=============================================================================
