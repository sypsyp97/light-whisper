------------------------ MODULE RuntimeConfiguration ------------------------
EXTENDS Naturals

(* Only engine_start/engine_restart admission through lifecycle transfer to the
   common startup call. Restart invocation, awaited stop settlement and release
   are separate steps. No successful controller reply is projected at transfer.
   The explicit engine_set argument belongs to ControllerAdmission instead.
   One configuration writer may settle under its lifecycle gate while dispatch
   waits. Correct validation samples the saved engine after acquiring that gate.
   The lifecycle-before-ASR ordering is the Stage A target. Its pending request
   owns no ASR lease, so the configuration writer can acquire ASR and settle.
   The original ASR-before-lifecycle controller excludes that full GUI schedule.
   Wrong = "snapshot" retains the old read in the Stage A-only candidate, rather
   than claiming a full original-source GUI reproduction. Wrong = "identity"
   bypasses the actual local-engine allowlist after the current read.
   No common startup completion, provider, network or wall-clock claim is made. *)
CONSTANT Wrong
VARIABLE s
vars == <<s>>
LocalEngines == {"localA", "localB"}
Configurations == LocalEngines \cup {"cloud", "unknown"}
(* paths::configured_engine maps unknown/missing raw values to qwen3-asr-0.6b. *)
ConfiguredEngine(raw) == IF raw = "unknown" THEN "localA" ELSE raw
RuntimeActions == {"engine_start", "engine_restart"}
TerminalPC == {"rejected", "transferred"}
PrefixPC == {"initial_validate", "acquire", "wait", "validate", "restart_stop",
             "wait_restart_stop", "restart_release", "start_transfer"}

Init ==
    \E action \in RuntimeActions, config \in Configurations, held \in BOOLEAN :
      s = [pc |-> "initial_validate", action |-> action, configured |-> config,
           snapshot |-> ConfiguredEngine(config), validated |-> "unread", reply |-> "none",
           lifecycleHeld |-> held, ownLifecycle |-> FALSE,
           actorAsr |-> FALSE, actorPhase |-> "none",
           changedWhileWaiting |-> FALSE, admitted |-> FALSE,
           stops |-> 0, generation |-> 0, actorGeneration |-> 0,
           ready |-> ConfiguredEngine(config) \in LocalEngines,
           actorReady |-> ConfiguredEngine(config) \in LocalEngines]

(* Keep the existing early rejection of an initially nonlocal saved engine.
   It acquires no lifecycle guard. A valid initial sample is checked again after
   the first actual await, because the configuration writer may settle there. *)
ValidateInitialConfiguration ==
    /\ s.pc = "initial_validate"
    /\ s' = IF s.snapshot \notin LocalEngines
            THEN [s EXCEPT !.pc = "rejected", !.reply = "invalid_request"]
            ELSE [s EXCEPT !.pc = "acquire"]

AcquireLifecycle ==
    /\ s.pc = "acquire"
    /\ s' = IF s.lifecycleHeld
            THEN [s EXCEPT !.pc = "wait"]
            ELSE [s EXCEPT !.ownLifecycle = (Wrong # "validation_lease"), !.pc = "validate"]

(* Stage A waits before acquiring its ASR lease. No third ASR owner is present
   in this bounded partition, so the lifecycle owner can acquire the real GUI
   writer's ASR lease. Starting this actor is optional, not fair-forced. *)
AcquireActorAsr ==
    /\ s.pc = "wait" /\ s.lifecycleHeld /\ ~s.ownLifecycle
    /\ s.actorPhase = "none" /\ ~s.actorAsr /\ ~s.changedWhileWaiting
    /\ s' = [s EXCEPT !.actorAsr = TRUE, !.actorPhase = "leased"]

(* GUI set_engine accepts only known local/cloud values. Raw unknown is an
   initial saved-file partition, normalized by the read, not a GUI write. *)
ChangeConfigurationWhileWaiting ==
    /\ s.pc = "wait" /\ s.lifecycleHeld /\ ~s.ownLifecycle
    /\ s.actorAsr /\ s.actorPhase = "leased" /\ ~s.changedWhileWaiting
    /\ \E config \in (LocalEngines \cup {"cloud"}) \ {s.configured} :
         s' = [s EXCEPT !.configured = config, !.changedWhileWaiting = TRUE,
               !.actorPhase = "stopping"]

(* This projects only the already-owned actor's successful stop/reap before it
   releases either gate. Cloud has no configured key in this fixture partition.
   Actor-owned effects establish the comparison frame for subsequent CLI work. *)
SettleActorStop ==
    /\ s.lifecycleHeld /\ ~s.ownLifecycle /\ s.actorAsr /\ s.actorPhase = "stopping"
    /\ Wrong # "actor_settlement"
    /\ s' = [s EXCEPT !.generation = @ + 1, !.actorGeneration = @ + 1,
              !.ready = FALSE, !.actorReady = FALSE, !.actorPhase = "settled"]

ReleaseActorAsr ==
    /\ s.lifecycleHeld /\ s.actorAsr /\ s.actorPhase = "settled"
    /\ s' = [s EXCEPT !.actorAsr = FALSE, !.actorPhase = "released"]

ReleaseExternalLifecycle ==
    /\ s.lifecycleHeld /\ ~s.actorAsr /\ s.actorPhase \in {"none", "released"}
    /\ s' = [s EXCEPT !.lifecycleHeld = FALSE,
              !.pc = IF s.pc = "wait" THEN "acquire" ELSE @]

ValidateCurrentConfiguration ==
    /\ s.pc = "validate" /\ s.ownLifecycle
    /\ LET engine == IF Wrong = "snapshot" THEN s.snapshot ELSE ConfiguredEngine(s.configured)
           next == [s EXCEPT !.validated = engine]
       IN s' = IF engine \notin LocalEngines /\ Wrong # "identity"
               THEN [next EXCEPT !.pc = "rejected", !.reply = "invalid_request",
                                 !.ownLifecycle = IF Wrong = "terminal_release" THEN @ ELSE FALSE]
               ELSE [next EXCEPT !.admitted = TRUE,
                     !.pc = IF s.action = "engine_restart" THEN "restart_stop"
                            ELSE "start_transfer"]

(* stop_server increments generation before its first process-mutex await.
   The caller retains lifecycle across that await, with no controller reply. *)
BeginRestartStop ==
    /\ s.pc = "restart_stop" /\ s.ownLifecycle
    /\ s' = [s EXCEPT !.stops = 1, !.generation = @ + 1,
              !.pc = "wait_restart_stop",
              !.ownLifecycle = IF Wrong = "restart_lease" THEN FALSE ELSE @]

(* Successful stop settlement is environmental, separately fair. stop_server
   clears readiness before returning; the caller still owns lifecycle here.
   Detailed stop failure/reap behavior belongs to the existing host model. *)
SettleRestartStop ==
    /\ s.pc = "wait_restart_stop" /\ s.ownLifecycle
    /\ Wrong # "restart_settlement"
    /\ s' = [s EXCEPT !.ready = FALSE, !.pc = "restart_release"]

TransferRestartAdmission ==
    /\ s.pc = "restart_release" /\ s.ownLifecycle
    /\ s' = [s EXCEPT !.pc = "transferred",
              !.ownLifecycle = IF Wrong = "terminal_release" THEN @ ELSE FALSE]

(* Transfer is the terminal boundary before the common service call reacquires
   lifecycle. It makes no claim that a child was spawned or became ready. *)
TransferStartAdmission ==
    /\ s.pc = "start_transfer" /\ s.ownLifecycle
    /\ s' = [s EXCEPT !.pc = "transferred",
              !.ownLifecycle = IF Wrong = "terminal_release" THEN @ ELSE FALSE]

PrefixStep == ValidateInitialConfiguration \/ AcquireLifecycle \/ ValidateCurrentConfiguration
           \/ BeginRestartStop \/ TransferStartAdmission \/ TransferRestartAdmission
ActorStep == ChangeConfigurationWhileWaiting \/ SettleActorStop \/ ReleaseActorAsr
Waiting == s.pc \in {"wait", "wait_restart_stop"} /\ UNCHANGED s
Terminal == s.pc \in TerminalPC /\ UNCHANGED s
Next == PrefixStep \/ AcquireActorAsr \/ ActorStep \/ ReleaseExternalLifecycle \/ SettleRestartStop
     \/ Waiting \/ Terminal
Spec == Init /\ [][Next]_vars /\ WF_vars(PrefixStep) /\ WF_vars(ReleaseExternalLifecycle)
        /\ WF_vars(ActorStep)
        /\ WF_vars(SettleRestartStop)

TypeOK ==
    /\ s.pc \in PrefixPC \cup TerminalPC /\ s.action \in RuntimeActions
    /\ s.configured \in Configurations /\ s.snapshot \in Configurations
    /\ s.validated \in Configurations \cup {"unread"}
    /\ s.reply \in {"none", "invalid_request"}
    /\ s.stops \in 0..1 /\ s.generation \in 0..2 /\ s.actorGeneration \in 0..1
    /\ s.actorPhase \in {"none", "leased", "stopping", "settled", "released"}
    /\ \A field \in {"lifecycleHeld", "ownLifecycle", "changedWhileWaiting",
                     "actorAsr", "admitted", "ready", "actorReady"} : s[field] \in BOOLEAN
InvalidSavedConfigurationNoEffects == ConfiguredEngine(s.configured) \notin LocalEngines =>
    s.stops = 0 /\ s.generation = s.actorGeneration /\ s.ready = s.actorReady
InvalidSavedConfigurationRejected ==
    ConfiguredEngine(s.configured) \notin LocalEngines /\ s.pc \in TerminalPC => s.reply = "invalid_request"
AdmittedIdentityIsCurrentLocal == s.admitted =>
    s.validated = ConfiguredEngine(s.configured) /\ ConfiguredEngine(s.configured) \in LocalEngines
LifecycleHeldDuringValidation == s.pc \in {"validate", "restart_stop", "wait_restart_stop",
                                         "restart_release", "start_transfer"}
    => s.ownLifecycle /\ ~s.lifecycleHeld /\ ~s.actorAsr
TerminalGuardsReleased == s.pc \in TerminalPC => ~s.ownLifecycle /\ ~s.actorAsr
AdmissionCompletes == <>(s.pc \in TerminalPC)

NoStartChangedConfigurationRejection == ~(s.action = "engine_start"
    /\ s.changedWhileWaiting /\ s.snapshot \in LocalEngines
    /\ ConfiguredEngine(s.configured) \notin LocalEngines /\ s.reply = "invalid_request"
    /\ s.pc = "rejected" /\ ~s.ownLifecycle)
NoRestartChangedConfigurationRejection == ~(s.action = "engine_restart"
    /\ s.changedWhileWaiting /\ s.snapshot \in LocalEngines
    /\ ConfiguredEngine(s.configured) \notin LocalEngines /\ s.reply = "invalid_request"
    /\ s.pc = "rejected" /\ ~s.ownLifecycle)
NoChangedLocalIdentityAdmission == ~(s.changedWhileWaiting
    /\ s.snapshot \in LocalEngines /\ ConfiguredEngine(s.configured) \in LocalEngines
    /\ s.snapshot # ConfiguredEngine(s.configured)
    /\ s.validated = ConfiguredEngine(s.configured) /\ s.pc = "transferred" /\ ~s.ownLifecycle)
NoPostActorRestartPending == ~(s.action = "engine_restart" /\ s.changedWhileWaiting
    /\ s.actorPhase = "released" /\ ~s.actorAsr /\ s.pc = "wait_restart_stop"
    /\ s.stops = 1 /\ s.generation = s.actorGeneration + 1
    /\ s.ownLifecycle /\ s.reply = "none")
=============================================================================
