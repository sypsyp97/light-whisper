-------------------------- MODULE Configuration --------------------------
EXTENDS Naturals

(* Desired CLI selection contract, not a proof of the baseline executable.
   Source binding: src-tauri/src/utils/paths.rs get_data_dir (9-25),
   get_engine_config_path/read_engine_config/configured_engine (62-79).
   CLI binding: scripts/light_whisper_cli.py main -> parser/doctor/engine_command.
   Baseline parser defaults to Qwen. Planned main resolver must read desktop
   config and validate before doctor/engine_command or EngineClient construction.
   LIGHT_WHISPER_DATA_DIR/APPDATA selects desktop config; --data-dir is solely
   the child sandbox. Unknown engine with a valid explicit override is allowed;
   malformed JSON and nonobject structure are rejected even with an override.

   Finite representatives partition absent file/key, two local engines, legacy
   alias, two cloud engines, unknown engine, malformed JSON and nonobject JSON.
   A single Resolve step models synchronous input evaluation and dispatch before
   process construction, not concurrent file mutation or engine lifecycle.
   No scheduler/progress claim: this bounded pure decision has no awaited steps.
   Terminal stuttering preserves TLC's deadlock check without adding resources.
*)

CONSTANTS Mode, Witness

ConfigCases == {"missing", "no_key", "qwen", "r2t2", "legacy", "glm", "alibaba",
                "unknown", "malformed", "nonobject"}
Local == {"qwen", "r2t2"}
Cloud == {"glm", "alibaba"}
StructuralErrors == {"malformed", "nonobject"}

VARIABLES phase, saved, argument, operation, configRoot, runtimeConfig,
          selected, engineSource, selectedRoot, exitCode, localSupported,
          spawned, diagnosticSafe

vars == <<phase, saved, argument, operation, configRoot, runtimeConfig,
          selected, engineSource, selectedRoot, exitCode, localSupported,
          spawned, diagnosticSafe>>

SavedChoice(value) ==
    CASE value \in {"missing", "no_key"} -> "qwen"
      [] value = "legacy" -> "r2t2"
      [] OTHER -> value

Init ==
    /\ phase = "pending"
    /\ saved \in ConfigCases
    /\ argument \in Local \cup {"omitted"}
    /\ operation \in {"doctor", "status", "transcribe"}
    /\ configRoot \in {"environment", "appdata"}
    /\ runtimeConfig \in Local
    /\ selected = "none"
    /\ engineSource = "none"
    /\ selectedRoot = "none"
    /\ exitCode = 0
    /\ localSupported = FALSE
    /\ spawned = FALSE
    /\ diagnosticSafe = TRUE

Resolve ==
    /\ phase = "pending"
    /\ LET effectiveSaved == IF Mode = "UseRuntimeDataConfig" THEN runtimeConfig ELSE saved
           badStructure == saved \in StructuralErrors /\ Mode # "IgnoreStructure"
           useArgument == argument # "omitted" /\ Mode # "IgnoreArgument"
           candidate == IF useArgument THEN argument
                        ELSE IF Mode = "IgnoreDesktopConfig" THEN "qwen"
                        ELSE SavedChoice(effectiveSaved)
           badEngine == ~useArgument /\ saved = "unknown" /\ Mode # "IgnoreUnknownGuard"
           blockedCloud == operation # "doctor" /\ candidate \in Cloud
                           /\ Mode # "AllowCloudSpawn"
           rejected == badStructure \/ badEngine \/ blockedCloud
       IN /\ selected' = IF badStructure \/ badEngine THEN "none" ELSE candidate
          /\ engineSource' = IF useArgument THEN "argument"
                             ELSE IF effectiveSaved \in {"missing", "no_key"}
                                  THEN "default" ELSE "desktop_config"
          /\ selectedRoot' = IF Mode = "UseRuntimeDataConfig" THEN "runtime" ELSE configRoot
          /\ exitCode' = IF rejected THEN 2 ELSE 0
          /\ localSupported' = (candidate \in Local /\ ~badStructure /\ ~badEngine
                                 /\ Mode # "MarkLocalUnsupported")
          /\ spawned' = ((operation # "doctor" \/ Mode = "SpawnDuringDoctor") /\ ~rejected)
          /\ diagnosticSafe' = ~(Mode = "LeakConfigPayload" /\ rejected)
    /\ phase' = "done"
    /\ UNCHANGED <<saved, argument, operation, configRoot, runtimeConfig>>

Done == /\ phase = "done" /\ UNCHANGED vars
Next == Resolve \/ Done
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"pending", "done"}
    /\ saved \in ConfigCases
    /\ argument \in Local \cup {"omitted"}
    /\ operation \in {"doctor", "status", "transcribe"}
    /\ configRoot \in {"environment", "appdata"}
    /\ runtimeConfig \in Local
    /\ selected \in ConfigCases \cup {"none"}
    /\ engineSource \in {"none", "argument", "default", "desktop_config"}
    /\ selectedRoot \in {"none", "environment", "appdata", "runtime"}
    /\ exitCode \in {0, 2}
    /\ localSupported \in BOOLEAN
    /\ spawned \in BOOLEAN
    /\ diagnosticSafe \in BOOLEAN

(* Oracle uses input classes/implications, independently of candidate calculation. *)
SelectionPrecedence ==
    phase = "done" /\ saved \notin StructuralErrors =>
      /\ (argument \in Local => selected = argument /\ engineSource = "argument")
      /\ (argument = "omitted" /\ saved = "qwen" =>
            selected = "qwen" /\ engineSource = "desktop_config")
      /\ (argument = "omitted" /\ saved \in {"r2t2", "legacy"} =>
            selected = "r2t2" /\ engineSource = "desktop_config")
      /\ (argument = "omitted" /\ saved \in Cloud =>
            selected = saved /\ engineSource = "desktop_config")
      /\ (argument = "omitted" /\ saved \in {"missing", "no_key"} =>
            selected = "qwen" /\ engineSource = "default")

InvalidConfigurationBlocks ==
    phase = "done" /\ (saved \in StructuralErrors \/
       (saved = "unknown" /\ argument = "omitted")) =>
      exitCode = 2 /\ ~spawned

OperationalLocalGuard ==
    phase = "done" /\ saved \in Cloud /\ argument = "omitted" =>
      /\ ~localSupported
      /\ (operation = "doctor" => exitCode = 0 /\ ~spawned)
      /\ (operation # "doctor" => exitCode = 2 /\ ~spawned)

LocalSelectionWorks ==
    phase = "done" /\ saved \notin StructuralErrors /\
      (argument \in Local \/ (argument = "omitted" /\
       saved \in {"missing", "no_key", "qwen", "r2t2", "legacy"})) =>
      exitCode = 0 /\ localSupported /\ (spawned = (operation # "doctor"))

DesktopDirectoryOnly == phase = "done" => selectedRoot = configRoot
SafeDiagnostics == phase = "done" /\ exitCode = 2 => diagnosticSafe

WitnessReached ==
    phase = "done" /\
      CASE Witness = "legacy" -> saved = "legacy" /\ argument = "omitted" /\ selected = "r2t2"
        [] Witness = "cloud_blocked" -> saved \in Cloud /\ argument = "omitted" /\
                                        operation = "status" /\ exitCode = 2 /\ ~spawned
        [] Witness = "cloud_doctor" -> saved \in Cloud /\ argument = "omitted" /\
                                       operation = "doctor" /\ exitCode = 0 /\ ~localSupported
        [] Witness = "malformed_override" -> saved = "malformed" /\ argument \in Local /\ exitCode = 2
        [] Witness = "unknown_override" -> saved = "unknown" /\ argument \in Local /\
                                           operation = "status" /\ exitCode = 0 /\ spawned
        [] Witness = "directory_separation" -> configRoot = "environment" /\ saved = "r2t2" /\
                                               runtimeConfig = "qwen" /\ argument = "omitted" /\
                                               selectedRoot = "environment" /\ selected = "r2t2"
        [] OTHER -> FALSE

WitnessNotReached == ~WitnessReached
=============================================================================
