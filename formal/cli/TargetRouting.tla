------------------------- MODULE TargetRouting -------------------------
EXTENDS Naturals

(* Desired Python routing only, not native Rust broker/server verification.
   Source: scripts/light_whisper_cli.py parser -> main -> resolve_target,
   app_command or engine_command/owned_engine; run_session lazily owns one client;
   EngineClient.__enter__/request/__exit__ owns child transport and cleanup.
   Baseline 3327fb849ba2c771957dcd02a0750eef827d78a1 has uncalled target
   declarations. Main must review final production correspondence after GREEN.

   Four independent option kinds share target semantics; each remains an input
   representative. File and nonfile inputs cover the is_file boundary; missing
   paths and directories are equivalent nonfiles here and separate Python cases.
   Available auto candidates enumerate all 8 subsets. One synchronous command
   or up to two session requests model routing/ownership, not session validation,
   live desktop state, native launch, scheduling or child deadline internals.
   A broker may fail startup. Closing affects the owned child, never desktop.
   Terminal stuttering is intentional. Fairness covers only this synchronous
   bounded driver path; it assumes transport/cleanup return as the existing
   EngineClient contract requires, not that a Rust server eventually responds.
*)

CONSTANTS Mode, Witness
Targets == {"app", "engine"}
Hints == {"none", "engine", "python", "engine_exe", "data_dir"}
Locations == {"release", "debug", "installed"}

VARIABLES phase, explicit, hint, appExe, operation, explicitFile, available,
          backendOK, target, rejected, chosenPath, commandKind, environmentKind,
          dataKind, resource, desktopAlive, exitCode, launched, requests

vars == <<phase, explicit, hint, appExe, operation, explicitFile, available,
          backendOK, target, rejected, chosenPath, commandKind, environmentKind,
          dataKind, resource, desktopAlive, exitCode, launched, requests>>

Conflict == (explicit = "app" /\ hint # "none") \/
            (explicit = "engine" /\ appExe) \/ (appExe /\ hint # "none")

Init ==
    /\ phase = "input"
    /\ explicit \in Targets \cup {"omitted"}
    /\ hint \in Hints
    /\ appExe \in BOOLEAN
    /\ operation \in {"doctor", "status", "session"}
    /\ explicitFile \in BOOLEAN
    /\ available \in SUBSET Locations
    /\ backendOK \in BOOLEAN
    /\ target = "none"
    /\ rejected = FALSE
    /\ chosenPath = "none"
    /\ commandKind = "none"
    /\ environmentKind = "none"
    /\ dataKind = "none"
    /\ resource = "none"
    /\ desktopAlive = TRUE
    /\ exitCode = 0
    /\ launched = FALSE
    /\ requests = 0

Select ==
    /\ phase = "input"
    /\ rejected' = (Conflict /\ Mode # "IgnoreConflict")
    /\ target' = IF explicit # "omitted" THEN explicit
                 ELSE IF appExe THEN "app"
                 ELSE IF hint # "none" THEN "engine"
                 ELSE IF Mode = "DefaultEngine" THEN "engine" ELSE "app"
    /\ phase' = IF Conflict /\ Mode # "IgnoreConflict" THEN "done" ELSE "selected"
    /\ exitCode' = IF Conflict /\ Mode # "IgnoreConflict" THEN 2 ELSE 0
    /\ UNCHANGED <<explicit, hint, appExe, operation, explicitFile, available,
                    backendOK, chosenPath, commandKind, environmentKind, dataKind,
                    resource, desktopAlive, launched, requests>>

ChoosePath ==
    IF appExe THEN IF explicitFile THEN "explicit"
                   ELSE IF Mode = "FallbackExplicit" /\ "release" \in available THEN "release" ELSE "none"
    ELSE IF Mode = "SkipRelease" /\ "debug" \in available THEN "debug"
    ELSE IF "release" \in available THEN "release"
    ELSE IF "debug" \in available THEN "debug"
    ELSE IF "installed" \in available THEN "installed"
    ELSE "none"

Doctor ==
    /\ phase = "selected" /\ operation = "doctor"
    /\ phase' = "done"
    /\ resource' = IF Mode = "DoctorSpawns" THEN "broker" ELSE "none"
    /\ launched' = (Mode = "DoctorSpawns")
    /\ UNCHANGED <<explicit, hint, appExe, operation, explicitFile, available,
                    backendOK, target, rejected, chosenPath, commandKind,
                    environmentKind, dataKind, desktopAlive, exitCode, requests>>

Launch ==
    /\ phase = "selected" /\ operation # "doctor"
    /\ LET path == ChoosePath
           missing == target = "app" /\ path = "none" /\ Mode # "IgnoreMissingApp"
           useEngine == (target = "engine" \/ Mode = "RouteAppToEngine") /\ Mode # "RouteEngineToApp"
       IN /\ phase' = IF missing THEN "done" ELSE IF backendOK THEN "active" ELSE "closing"
          /\ exitCode' = IF missing THEN 2 ELSE IF backendOK THEN 0 ELSE 1
          /\ chosenPath' = IF target = "app" THEN path ELSE "none"
          /\ commandKind' = IF missing THEN "none" ELSE IF useEngine THEN "serve" ELSE "agent_client"
          /\ environmentKind' = IF missing THEN "none"
                                ELSE IF useEngine \/ Mode = "RewriteAppEnvironment"
                                     THEN "cpu2_offline" ELSE "inherited"
          /\ dataKind' = IF missing THEN "none"
                         ELSE IF useEngine \/ Mode = "TemporaryAppData" THEN "temporary" ELSE "desktop"
          /\ resource' = IF missing THEN "none" ELSE IF useEngine THEN "engine" ELSE "broker"
          /\ launched' = ~missing
    /\ UNCHANGED <<explicit, hint, appExe, operation, explicitFile, available,
                    backendOK, target, rejected, desktopAlive, requests>>

Request ==
    /\ phase = "active"
    /\ requests' = requests + 1
    /\ phase' = IF operation = "session" /\ requests = 0 THEN "active" ELSE "closing"
    /\ UNCHANGED <<explicit, hint, appExe, operation, explicitFile, available,
                    backendOK, target, rejected, chosenPath, commandKind,
                    environmentKind, dataKind, resource, desktopAlive, exitCode, launched>>

Close ==
    /\ phase = "closing"
    /\ Mode # "StuckCleanup"
    /\ phase' = "done"
    /\ resource' = IF Mode = "ForgetReap" THEN resource ELSE "none"
    /\ desktopAlive' = ~(Mode = "StopDesktop" /\ target = "app")
    /\ UNCHANGED <<explicit, hint, appExe, operation, explicitFile, available,
                    backendOK, target, rejected, chosenPath, commandKind,
                    environmentKind, dataKind, exitCode, launched, requests>>

Done == /\ phase = "done" /\ UNCHANGED vars
Stall == /\ phase = "closing" /\ Mode = "StuckCleanup" /\ UNCHANGED vars
Advance == Select \/ Doctor \/ Launch \/ Request \/ Close
Spec == Init /\ [][Advance \/ Done \/ Stall]_vars /\ WF_vars(Advance)

TypeOK ==
    /\ phase \in {"input", "selected", "active", "closing", "done"}
    /\ explicit \in Targets \cup {"omitted"}
    /\ hint \in Hints
    /\ appExe \in BOOLEAN /\ explicitFile \in BOOLEAN /\ backendOK \in BOOLEAN
    /\ operation \in {"doctor", "status", "session"}
    /\ available \in SUBSET Locations
    /\ target \in Targets \cup {"none"}
    /\ chosenPath \in Locations \cup {"explicit", "none"}
    /\ commandKind \in {"none", "serve", "agent_client"}
    /\ environmentKind \in {"none", "inherited", "cpu2_offline"}
    /\ dataKind \in {"none", "desktop", "temporary"}
    /\ resource \in {"none", "broker", "engine"}
    /\ rejected \in BOOLEAN /\ desktopAlive \in BOOLEAN /\ launched \in BOOLEAN
    /\ exitCode \in {0, 1, 2} /\ requests \in 0..2

TargetPrecedence == phase # "input" /\ ~Conflict =>
    /\ (explicit \in Targets => target = explicit)
    /\ (explicit = "omitted" /\ appExe => target = "app")
    /\ (explicit = "omitted" /\ ~appExe /\ hint # "none" => target = "engine")
    /\ (explicit = "omitted" /\ ~appExe /\ hint = "none" => target = "app")
ConflictBlocks == phase # "input" /\ Conflict => rejected /\ exitCode = 2 /\ ~launched
BrokerRouting == launched /\ target = "app" /\ operation # "doctor" => commandKind = "agent_client"
EngineRouting == launched /\ target = "engine" /\ operation # "doctor" => commandKind = "serve"
EnvironmentBoundary == launched /\ operation # "doctor" =>
    /\ (target = "app" => environmentKind = "inherited")
    /\ (target = "engine" => environmentKind = "cpu2_offline")
DataBoundary == launched /\ operation # "doctor" =>
    /\ (target = "app" => dataKind = "desktop")
    /\ (target = "engine" => dataKind = "temporary")
DoctorReadOnly == operation = "doctor" => ~launched /\ resource = "none"
ExistingAppRequired == launched /\ target = "app" /\ operation # "doctor" =>
    IF appExe THEN explicitFile ELSE chosenPath \in available
CandidatePriority == phase \in {"active", "closing", "done"} /\ target = "app" /\
    operation # "doctor" /\ ~Conflict =>
      /\ (appExe /\ explicitFile => chosenPath = "explicit")
      /\ (~appExe /\ "release" \in available => chosenPath = "release")
      /\ (~appExe /\ "release" \notin available /\ "debug" \in available => chosenPath = "debug")
      /\ (~appExe /\ available = {"installed"} => chosenPath = "installed")
OwnedResourceCleanup == phase = "done" /\ operation # "doctor" => resource = "none"
DesktopSurvives == desktopAlive
EventuallyFinished == <> (phase = "done")

WitnessReached == phase = "done" /\
    CASE Witness = "app_session" -> target = "app" /\ operation = "session" /\ requests = 2 /\ desktopAlive
      [] Witness = "engine" -> target = "engine" /\ operation = "status" /\ launched /\ dataKind = "temporary"
      [] Witness = "conflict" -> Conflict /\ rejected /\ ~launched
      [] Witness = "startup_failure" -> target = "app" /\ launched /\ ~backendOK /\ exitCode = 1 /\ resource = "none"
      [] Witness = "installed" -> target = "app" /\ launched /\ chosenPath = "installed"
      [] Witness = "explicit_missing" -> target = "app" /\ appExe /\ ~explicitFile /\
                                         "release" \in available /\ operation = "status" /\ exitCode = 2 /\ ~launched
      [] OTHER -> FALSE
WitnessNotReached == ~WitnessReached
=============================================================================
