-------------------------- MODULE ControllerAdmission --------------------------
EXTENDS Naturals, FiniteSets

(* One actual engine_set dispatch prefix. Observations are sampled where source
   reads them. Initial watch value has already been seen by the receiver. A later
   watch notification wins only when the biased select polls it before polling
   the request. An external change after that check may precede a synchronous
   write without being observed in that poll. No stronger cancellation claim.

   First config write transfers to the existing AgentHost lifecycle model. The
   optional common continuation below records write/stop/generation/event
   prefixes, without fairness or post-commit cancellation/completion claims.
   Internal pre-existing mutex owners eventually settle; user shutdown and
   download changes are optional, never fair-forced. The prior lifecycle owner
   may publish one recording claim or startup owner while dispatch waits. These
   actors are not fair-forced and cannot claim after admission. All counters
   bound one call. Wrong = "pre_lifecycle_admission" retains the legacy order.
*)
CONSTANT Wrong
VARIABLE s
vars == <<s>>
Engines == {"A", "B"}
Requests == Engines \cup {"invalid"}
PrefixPC == {"initial", "poll", "validate", "recording", "processing", "lease",
             "starting", "lifecycle", "wait_lifecycle", "download", "wait_download", "write"}
TerminalPC == {"rejected", "accepted"}
(* Uniform encoding of None, Some(TRUE), Some(FALSE); changed stays Boolean. *)
Observation == {"unread", "true", "false"}
Observe(value) == IF value THEN "true" ELSE "false"

EffectFrame == <<s.configured, s.native, s.writes, s.stops, s.generation,
                s.events, s.ready, s.inline>>
InitialFrame == <<"A", s.initialNative, 0, 0, 0, 0,
                 s.initialNative = "live", s.initialNative = "live">>
Rejected(base, code) == [base EXCEPT !.pc = "rejected",
    !.reply = IF Wrong = "reply" THEN "none" ELSE code,
    !.ownAsr = IF Wrong = "reject_release" THEN @ ELSE FALSE,
    !.ownLifecycle = IF Wrong = "reject_release" THEN @ ELSE FALSE]

Init ==
    \E down \in BOOLEAN, request \in Requests,
       busy \in {"none", "recording", "processing"}, lease \in BOOLEAN,
       starting \in BOOLEAN, lifecycle \in BOOLEAN, slot \in BOOLEAN,
       download \in BOOLEAN :
    \E native \in (IF starting THEN {"none"} ELSE {"none", "live"}) :
      s = [pc |-> "initial", resume |-> "validate", reply |-> "none",
           shutdown |-> down, watchVersion |-> 0, watchSeen |-> 0,
           requested |-> request, externalBusy |-> busy, externalLease |-> lease,
           starting |-> starting, lifecycleHeld |-> lifecycle, slotHeld |-> slot,
           downloading |-> download, initialDownload |-> download, downloadChanged |-> FALSE,
           downloadStartedWhileWaiting |-> FALSE, shutdownWhileWaiting |-> FALSE,
           recordingClaimedWhileWaiting |-> FALSE, startupClaimedWhileWaiting |-> FALSE,
           ownAsr |-> FALSE, ownLifecycle |-> FALSE,
           leaseAcquired |-> FALSE, lifecycleAcquired |-> FALSE,
           observed |-> [initial |-> "unread", changed |-> FALSE,
             engine |-> "unread", recording |-> "unread", processing |-> "unread",
             leaseAvailable |-> "unread", starting |-> "unread", download |-> "unread"],
           configured |-> "A", native |-> native, initialNative |-> native,
           writes |-> 0, stops |-> 0, generation |-> 0, events |-> 0,
           ready |-> native = "live", inline |-> native = "live",
           writeHadAsr |-> FALSE, writeHadLifecycle |-> FALSE,
           continuation |-> "none"]

InitialBorrow ==
    /\ s.pc = "initial"
    /\ LET next == [s EXCEPT !.observed.initial = Observe(s.shutdown)]
       IN s' = IF s.shutdown /\ Wrong # "initial"
               THEN Rejected(next, "owner_shutdown")
               ELSE [next EXCEPT !.pc = "poll"]

PollSelect ==
    /\ s.pc = "poll"
    /\ LET changed == s.watchVersion # s.watchSeen
           next == [s EXCEPT !.observed.changed = @ \/ changed,
                             !.watchSeen = s.watchVersion]
       IN s' = IF changed /\ Wrong # "watch"
               THEN Rejected(next, "owner_shutdown")
               ELSE [next EXCEPT !.pc = s.resume]

ValidateEngine ==
    /\ s.pc = "validate"
    /\ LET valid == s.requested \in Engines
           next == [s EXCEPT !.observed.engine = Observe(valid)]
       IN s' = IF ~valid /\ Wrong # "engine"
               THEN Rejected(next, "invalid_request")
               ELSE [next EXCEPT !.pc = IF Wrong = "pre_lifecycle_admission"
                                      THEN "recording" ELSE "lifecycle"]

CheckRecording ==
    /\ s.pc = "recording"
    /\ LET blocked == s.externalBusy = "recording"
           next == [s EXCEPT !.observed.recording = Observe(blocked)]
       IN s' = IF blocked /\ Wrong # "recording"
               THEN Rejected(next, "busy")
               ELSE [next EXCEPT !.pc = "processing"]

CheckProcessing ==
    /\ s.pc = "processing"
    /\ LET blocked == s.externalBusy = "processing"
           next == [s EXCEPT !.observed.processing = Observe(blocked)]
       IN s' = IF blocked /\ Wrong # "processing"
               THEN Rejected(next, "busy")
               ELSE [next EXCEPT !.pc = "lease"]

TryAsrLease ==
    /\ s.pc = "lease"
    /\ LET available == ~s.externalLease
           next == [s EXCEPT !.observed.leaseAvailable = Observe(available)]
       IN s' = IF ~available /\ Wrong # "lease"
               THEN Rejected(next, "busy")
               ELSE [next EXCEPT !.ownAsr = TRUE, !.leaseAcquired = TRUE,
                                 !.pc = "starting"]

CheckStarting ==
    /\ s.pc = "starting"
    /\ LET next == [s EXCEPT !.observed.starting = Observe(s.starting)]
       IN s' = IF s.starting /\ Wrong # "starting"
               THEN Rejected(next, "busy")
               ELSE [next EXCEPT !.pc = IF Wrong = "pre_lifecycle_admission"
                                      THEN "lifecycle" ELSE "download"]

AcquireLifecycle ==
    /\ s.pc = "lifecycle"
    /\ s' = IF s.lifecycleHeld /\ Wrong # "lifecycle"
            THEN [s EXCEPT !.pc = "wait_lifecycle", !.resume = "lifecycle"]
            ELSE [s EXCEPT !.ownLifecycle = (Wrong # "lifecycle"),
                  !.lifecycleAcquired = (Wrong # "lifecycle"),
                  !.pc = IF Wrong = "pre_lifecycle_admission" THEN "download" ELSE "recording"]

InspectDownload ==
    /\ s.pc = "download"
    /\ IF s.slotHeld
       THEN s' = [s EXCEPT !.pc = "wait_download", !.resume = "download",
                  !.ownAsr = IF Wrong = "release_lease" THEN FALSE ELSE @,
                  !.ownLifecycle = IF Wrong = "release_lifecycle" THEN FALSE ELSE @]
       ELSE LET next == [s EXCEPT !.observed.download = Observe(s.downloading)]
            IN s' = IF s.downloading /\ Wrong # "download"
                    THEN Rejected(next, "busy")
                    ELSE [next EXCEPT !.pc = "write"]

WriteConfiguration ==
    /\ s.pc = "write"
    /\ s' = [s EXCEPT !.configured = s.requested, !.writes = 1,
              !.writeHadAsr = s.ownAsr, !.writeHadLifecycle = s.ownLifecycle,
              !.pc = "accepted", !.reply = "accepted", !.continuation = "written"]

OwnerShutdown ==
    /\ s.pc \in PrefixPC /\ ~s.shutdown
    /\ s' = [s EXCEPT !.shutdown = TRUE, !.watchVersion = 1,
              !.shutdownWhileWaiting = @ \/ (s.pc \in {"wait_lifecycle", "wait_download"}),
              !.pc = IF s.pc \in {"wait_lifecycle", "wait_download"} THEN "poll" ELSE @]

ChangeDownload ==
    /\ s.pc \in PrefixPC /\ ~s.downloadChanged
    /\ s' = [s EXCEPT !.downloading = ~@, !.downloadChanged = TRUE,
              !.downloadStartedWhileWaiting =
                (s.pc = "wait_lifecycle" /\ ~s.downloading)]

(* audio.rs holds lifecycle while waiting for paste_dispatch. When that wait
   settles it checks readiness and publishes Starting plus its snapshot under
   recording.recording, without checking native_asr_owner. The actor below owns
   the pre-existing lifecycle gate; publication must precede releasing that gate.
   No capture, foreground/window work or later recording completion is modeled. *)
ClaimRecordingWhileWaiting ==
    /\ s.pc = "wait_lifecycle" /\ s.lifecycleHeld /\ ~s.ownLifecycle
    /\ s.externalBusy = "none" /\ s.native = "live" /\ s.ready /\ ~s.starting
    /\ ~s.recordingClaimedWhileWaiting /\ ~s.startupClaimedWhileWaiting
    /\ s' = [s EXCEPT !.externalBusy = "recording", !.recordingClaimedWhileWaiting = TRUE]

(* start_server checks process absence and claims a nonzero startup owner while
   holding lifecycle, before releasing it for find_engine. Existing readiness
   is false here; this does not invent a recording claim against a stopped child. *)
ClaimStartupWhileWaiting ==
    /\ s.pc = "wait_lifecycle" /\ s.lifecycleHeld /\ ~s.ownLifecycle
    /\ s.externalBusy = "none" /\ s.native = "none" /\ ~s.ready /\ ~s.starting
    /\ ~s.recordingClaimedWhileWaiting /\ ~s.startupClaimedWhileWaiting
    /\ s' = [s EXCEPT !.starting = TRUE, !.startupClaimedWhileWaiting = TRUE]

ReleaseLifecycle ==
    /\ s.lifecycleHeld /\ Wrong # "settlement"
    /\ s' = [s EXCEPT !.lifecycleHeld = FALSE,
              !.pc = IF s.pc = "wait_lifecycle" THEN "poll" ELSE @]

ReleaseDownloadSlot ==
    /\ s.slotHeld /\ Wrong # "settlement"
    /\ s' = [s EXCEPT !.slotHeld = FALSE,
              !.pc = IF s.pc = "wait_download" THEN "poll" ELSE @]

(* Optional projections of the common successful continuation. Admission is
   already terminal at the first write. These actions have no fairness and do
   not prove native settlement, event delivery, or cancellation after that write.
   The separate AgentHost model owns that lifecycle contract. *)
BeginCommonStop ==
    /\ s.pc = "accepted" /\ s.continuation = "written"
    /\ s' = [s EXCEPT !.stops = 1, !.generation = 1, !.continuation = "stopping"]
SettleCommonStop ==
    /\ s.pc = "accepted" /\ s.continuation = "stopping"
    /\ s' = [s EXCEPT !.native = "none", !.ready = FALSE, !.inline = FALSE,
              !.continuation = "stopped"]
EmitCommonEvent ==
    /\ s.pc = "accepted" /\ s.continuation = "stopped"
    /\ s' = [s EXCEPT !.events = 1, !.continuation = "emitted",
              !.ownAsr = FALSE, !.ownLifecycle = FALSE]

PrefixStep == InitialBorrow \/ PollSelect \/ ValidateEngine \/ CheckRecording
           \/ CheckProcessing \/ TryAsrLease \/ CheckStarting \/ AcquireLifecycle
           \/ InspectDownload \/ WriteConfiguration
Waiting == s.pc \in {"wait_lifecycle", "wait_download"} /\ UNCHANGED s
Terminal == s.pc \in TerminalPC /\ UNCHANGED s
Next == PrefixStep \/ OwnerShutdown \/ ChangeDownload \/ ReleaseLifecycle
     \/ ClaimRecordingWhileWaiting \/ ClaimStartupWhileWaiting
     \/ ReleaseDownloadSlot \/ BeginCommonStop \/ SettleCommonStop
     \/ EmitCommonEvent \/ Waiting \/ Terminal
Spec == Init /\ [][Next]_vars /\ WF_vars(PrefixStep)
        /\ WF_vars(ReleaseLifecycle) /\ WF_vars(ReleaseDownloadSlot)

TypeOK ==
    /\ s.pc \in PrefixPC \cup TerminalPC
    /\ s.resume \in {"validate", "lifecycle", "download"}
    /\ s.reply \in {"none", "owner_shutdown", "busy", "invalid_request", "accepted"}
    /\ s.requested \in Requests /\ s.configured \in Requests
    /\ s.externalBusy \in {"none", "recording", "processing"}
    /\ s.watchSeen \in 0..1 /\ s.watchVersion \in 0..1
    /\ s.native \in {"none", "live"} /\ s.initialNative \in {"none", "live"}
    /\ s.writes \in 0..1 /\ s.stops \in 0..1 /\ s.generation \in 0..1 /\ s.events \in 0..1
    /\ s.continuation \in {"none", "written", "stopping", "stopped", "emitted"}
    /\ \A field \in {"shutdown", "externalLease", "starting", "lifecycleHeld", "slotHeld",
        "downloading", "initialDownload", "downloadChanged", "downloadStartedWhileWaiting", "shutdownWhileWaiting",
        "recordingClaimedWhileWaiting", "startupClaimedWhileWaiting",
        "ownAsr", "ownLifecycle", "leaseAcquired", "lifecycleAcquired",
        "ready", "inline", "writeHadAsr", "writeHadLifecycle"} :
          s[field] \in BOOLEAN
    /\ \A field \in {"initial", "engine", "recording", "processing", "leaseAvailable",
                     "starting", "download"} : s.observed[field] \in Observation
    /\ s.observed.changed \in BOOLEAN

InitialShutdownNoEffects == s.observed.initial = "true" => EffectFrame = InitialFrame
ObservedShutdownNoEffects == s.observed.changed => EffectFrame = InitialFrame
InvalidEngineNoEffects == s.observed.engine = "false" => EffectFrame = InitialFrame
RecordingNoEffects == s.observed.recording = "true" => EffectFrame = InitialFrame
ProcessingNoEffects == s.observed.processing = "true" => EffectFrame = InitialFrame
LeaseUnavailableNoEffects == s.observed.leaseAvailable = "false" => EffectFrame = InitialFrame
StartingNoEffects == s.observed.starting = "true" => EffectFrame = InitialFrame
DownloadNoEffects == s.observed.download = "true" => EffectFrame = InitialFrame
LifecycleHeldAtWrite == s.writes = 1 => s.writeHadLifecycle
AsrHeldAtWrite == s.writes = 1 => s.writeHadAsr
RejectedGuardsReleased == s.pc = "rejected" => ~s.ownAsr /\ ~s.ownLifecycle
LeaseRetainedBeforeWrite == s.leaseAcquired /\ s.pc \in PrefixPC => s.ownAsr
LifecycleRetainedBeforeWrite == s.lifecycleAcquired /\ s.pc \in PrefixPC => s.ownLifecycle
RecordingClaimDuringWaitNoEffects == s.recordingClaimedWhileWaiting => EffectFrame = InitialFrame
StartupClaimDuringWaitNoEffects == s.startupClaimedWhileWaiting => EffectFrame = InitialFrame
ExpectedRejection == CASE s.observed.initial = "true" \/ s.observed.changed -> "owner_shutdown"
    [] s.observed.engine = "false" -> "invalid_request"
    [] s.observed.recording = "true" \/ s.observed.processing = "true"
       \/ s.observed.leaseAvailable = "false" \/ s.observed.starting = "true"
       \/ s.observed.download = "true" -> "busy"
    [] OTHER -> "none"
RejectionMatchesObservedGuard == s.pc = "rejected" => s.reply = ExpectedRejection
AdmissionCompletes == <>(s.pc \in TerminalPC)

NoInitialShutdownRejection == ~(s.observed.initial = "true" /\ s.watchVersion = 0 /\ s.watchSeen = 0 /\ s.reply = "owner_shutdown")
NoObservedShutdownRejection == ~(s.observed.changed /\ s.reply = "owner_shutdown")
NoInvalidRejection == ~(s.observed.engine = "false" /\ s.reply = "invalid_request")
NoRecordingRejection == ~(s.observed.recording = "true" /\ s.reply = "busy")
NoProcessingRejection == ~(s.observed.processing = "true" /\ s.reply = "busy")
NoLeaseRejection == ~(s.observed.leaseAvailable = "false" /\ s.reply = "busy")
NoAsrFreeStartingRejection == ~(s.observed.starting = "true" /\ ~s.externalLease /\ s.initialNative = "none" /\ s.reply = "busy")
NoDownloadRejection == ~(s.initialDownload /\ s.observed.download = "true" /\ s.reply = "busy")
NoDownloadStartedDuringWait == ~(s.downloadStartedWhileWaiting /\ s.observed.download = "true" /\ s.reply = "busy")
NoShutdownDuringWait == ~(s.shutdownWhileWaiting /\ s.observed.changed /\ s.reply = "owner_shutdown")
NoAcceptedCommonEffects == ~(s.writes = 1 /\ s.stops = 1 /\ s.generation = 1 /\ s.events = 1)
NoUnobservedRetirementWrite == ~(s.shutdown /\ ~s.observed.changed /\ s.observed.initial = "false" /\ s.writes = 1)
NoRecordingClaimDuringWaitRejection == ~(s.recordingClaimedWhileWaiting
    /\ s.observed.recording = "true" /\ s.reply = "busy" /\ s.pc = "rejected"
    /\ ~s.ownAsr /\ ~s.ownLifecycle)
NoStartupClaimDuringWaitRejection == ~(s.startupClaimedWhileWaiting
    /\ s.observed.starting = "true" /\ s.reply = "busy" /\ s.pc = "rejected"
    /\ ~s.ownAsr /\ ~s.ownLifecycle)
=============================================================================
