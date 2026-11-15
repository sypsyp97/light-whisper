---------------------------- MODULE Bootstrap ----------------------------
EXTENDS Naturals

(* Pure Python bootstrap/selection contract, not a frozen binary or installer proof.
   Source binding: scripts/light_whisper_cli.py ROOT initialization;
   app_path/app_command/doctor; engine_command/doctor; resolve_engine_selection
   and owned_engine/request_client. Desktop models_dir precedence comes from
   src-tauri/src/utils/paths.rs read_models_dir/get_default_models_dir/
   get_effective_models_dir (434-475). Baseline070261bb lacks frozen bootstrap,
   installed-runtime fallback, cache selection and app marker validation.
   resourceRoot maps to doctor resource_root, never a frozen repository label;
   Python tests require repository=null when frozen and the actual root in source mode.

   Five disjoint finite input partitions prevent irrelevant Cartesian products.
   Inputs are one synchronous config/environment/filesystem observation. No live
   file mutation, loader lifetime, arbitrary bytes, PyInstaller packaging, native
   broker, installer wiring, downloads or model usability is proved. Is-file and
   ASCII-marker-presence are predicates; Python tests cover directory/missing,
   contiguous marker data and a marker after a large binary prefix.
   Dev release represents the existing release/debug priority, separately checked
   by TargetRouting. Paths here are semantic resolved classes; Python tests check
   actual expanduser/resolve and HF_HOME presence including the empty string.
   No progress claim: each partition is a single finite synchronous decision.
*)

CONSTANTS Mode, Witness
VARIABLES phase, scenario, frozen, explicit, sourcePython, sourceScript,
          installedEngine, neighbor, devRelease, installedApp, marked,
          custom, hfHome, target, resourceRoot, versionRoot, appChoice,
          canLaunch, protocolSupported, runtimeChoice, wrapper,
          cacheChoice, cacheSource, childCache, normalized, inputText, decodedText

inputs == <<scenario, frozen, explicit, sourcePython, sourceScript,
            installedEngine, neighbor, devRelease, installedApp, marked,
            custom, hfHome, target, inputText>>
vars == <<phase, scenario, frozen, explicit, sourcePython, sourceScript,
          installedEngine, neighbor, devRelease, installedApp, marked,
          custom, hfHome, target, resourceRoot, versionRoot, appChoice,
          canLaunch, protocolSupported, runtimeChoice, wrapper,
          cacheChoice, cacheSource, childCache, normalized, inputText, decodedText>>

Init ==
    /\ phase = "pending"
    /\ scenario \in {"resource", "app", "runtime", "cache", "encoding"}
    /\ frozen \in (IF scenario \in {"resource", "app"} THEN BOOLEAN ELSE {FALSE})
    /\ explicit \in (CASE scenario = "app" -> {"none", "marked", "unmarked", "missing"}
                      [] scenario = "runtime" -> {"none", "python_file", "python_missing", "exe_file", "exe_missing"}
                      [] OTHER -> {"none"})
    /\ sourcePython \in (IF scenario = "runtime" THEN BOOLEAN ELSE {FALSE})
    /\ sourceScript \in (IF scenario = "runtime" THEN BOOLEAN ELSE {FALSE})
    /\ installedEngine \in (IF scenario = "runtime" THEN BOOLEAN ELSE {FALSE})
    /\ neighbor \in (IF scenario = "app" THEN BOOLEAN ELSE {FALSE})
    /\ devRelease \in (IF scenario = "app" THEN BOOLEAN ELSE {FALSE})
    /\ installedApp \in (IF scenario = "app" THEN BOOLEAN ELSE {FALSE})
    /\ marked \in (IF scenario = "app" THEN BOOLEAN ELSE {FALSE})
    /\ custom \in (IF scenario = "cache" THEN {"valid", "missing", "empty", "nonstring"} ELSE {"missing"})
    /\ hfHome \in (IF scenario = "cache" THEN {"missing", "empty", "set"} ELSE {"missing"})
    /\ target \in (IF scenario = "cache" THEN {"app", "engine"} ELSE {"app"})
    /\ inputText \in (IF scenario = "encoding" THEN {"ascii", "unicode"} ELSE {"ascii"})
    /\ resourceRoot = "none" /\ versionRoot = "none" /\ appChoice = "none"
    /\ canLaunch = FALSE /\ protocolSupported = FALSE
    /\ runtimeChoice = "none" /\ wrapper = FALSE
    /\ cacheChoice = "none" /\ cacheSource = "none" /\ childCache = "none"
    /\ normalized = TRUE
    /\ decodedText = "none"

ResourceResolve ==
    /\ phase = "pending" /\ scenario = "resource"
    /\ resourceRoot' = IF frozen /\ Mode # "IgnoreFrozen" THEN "assets" ELSE "source"
    /\ versionRoot' = IF frozen /\ Mode \notin {"IgnoreFrozen", "WrongMetadata"} THEN "assets" ELSE "source"
    /\ phase' = "done" /\ UNCHANGED inputs
    /\ UNCHANGED <<appChoice, canLaunch, protocolSupported, runtimeChoice, wrapper,
                    cacheChoice, cacheSource, childCache, normalized, decodedText>>

AutoApp == IF neighbor /\ ((frozen /\ Mode # "IgnoreNeighbor") \/ Mode = "SourceUsesNeighbor") THEN "neighbor"
           ELSE IF devRelease THEN "dev" ELSE IF installedApp THEN "installed" ELSE "none"
FirstApp == IF explicit = "missing" THEN IF Mode = "FallbackExplicitApp" THEN AutoApp ELSE "none"
            ELSE IF explicit # "none" THEN "explicit" ELSE AutoApp
IsMarked(choice) == choice # "none" /\
    (explicit = "marked" \/ (explicit = "none" /\ marked))

AppResolve ==
    /\ phase = "pending" /\ scenario = "app"
    /\ LET first == FirstApp
           choice == IF Mode = "SkipUnmarked" /\ ~IsMarked(first) /\ installedApp
                     THEN "installed" ELSE first
           supported == IsMarked(choice) \/ (Mode = "SkipUnmarked" /\ choice = "installed")
       IN /\ appChoice' = choice
          /\ canLaunch' = (choice # "none" /\ (supported \/ Mode = "IgnoreMarker"))
          /\ protocolSupported' = (supported \/ Mode = "ClaimCapability")
    /\ phase' = "done" /\ UNCHANGED inputs
    /\ UNCHANGED <<resourceRoot, versionRoot, runtimeChoice, wrapper,
                    cacheChoice, cacheSource, childCache, normalized, decodedText>>

DefaultRuntime ==
    IF sourcePython /\ (sourceScript \/ Mode = "PartialSource") THEN "source_python"
    ELSE IF installedEngine /\ Mode # "IgnoreInstalled" THEN "installed_engine" ELSE "missing"

RuntimeResolve ==
    /\ phase = "pending" /\ scenario = "runtime"
    /\ LET choice == IF explicit = "none" \/ Mode = "IgnoreExplicitRuntime" THEN DefaultRuntime ELSE explicit
       IN /\ runtimeChoice' = choice
          /\ canLaunch' = (choice = "exe_file" \/ choice = "installed_engine" \/
                           (choice = "python_file" /\ sourceScript) \/
                           (choice = "source_python" /\ sourcePython /\ sourceScript) \/
                           Mode = "AllowMissingRuntime")
          /\ wrapper' = (choice \in {"source_python", "python_file", "python_missing"} \/
                          (choice = "installed_engine" /\ Mode = "WrapInstalled"))
    /\ phase' = "done" /\ UNCHANGED inputs
    /\ UNCHANGED <<resourceRoot, versionRoot, appChoice, protocolSupported,
                    cacheChoice, cacheSource, childCache, normalized, decodedText>>

CacheResolve ==
    /\ phase = "pending" /\ scenario = "cache"
    /\ LET useCustom == (custom = "valid" /\ Mode # "IgnoreCustom") \/
                        (custom = "empty" /\ Mode = "AcceptEmptyCustom") \/
                        (custom = "nonstring" /\ Mode = "AcceptNonstringCustom")
           choice == IF useCustom THEN "custom"
                     ELSE IF hfHome = "set" THEN "hf_set_hub"
                     ELSE IF hfHome = "empty" /\ Mode # "IgnoreEmptyHF" THEN "cwd_hub"
                     ELSE "home_cache"
       IN /\ cacheChoice' = choice
          /\ cacheSource' = IF Mode = "WrongCacheSource" THEN "default"
                             ELSE IF useCustom THEN "desktop_config"
                             ELSE IF choice \in {"hf_set_hub", "cwd_hub"} THEN "HF_HOME" ELSE "default"
          /\ childCache' = IF target = "app" /\ Mode # "RewriteAppCache" THEN "inherited"
                           ELSE IF target = "engine" /\ Mode = "KeepInheritedEngineCache" THEN "inherited"
                           ELSE choice
          /\ normalized' = (Mode # "UnresolvedCache")
    /\ phase' = "done" /\ UNCHANGED inputs
    /\ UNCHANGED <<resourceRoot, versionRoot, appChoice, canLaunch, protocolSupported, runtimeChoice, wrapper, decodedText>>

EncodingResolve ==
    /\ phase = "pending" /\ scenario = "encoding"
    /\ decodedText' = IF inputText = "unicode" /\ Mode = "LegacyInputEncoding" THEN "corrupted" ELSE inputText
    /\ phase' = "done" /\ UNCHANGED inputs
    /\ UNCHANGED <<resourceRoot, versionRoot, appChoice, canLaunch, protocolSupported,
                    runtimeChoice, wrapper, cacheChoice, cacheSource, childCache, normalized>>

Done == /\ phase = "done" /\ UNCHANGED vars
Next == ResourceResolve \/ AppResolve \/ RuntimeResolve \/ CacheResolve \/ EncodingResolve \/ Done
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"pending", "done"}
    /\ scenario \in {"resource", "app", "runtime", "cache", "encoding"}
    /\ explicit \in {"none", "marked", "unmarked", "missing", "python_file", "python_missing", "exe_file", "exe_missing"}
    /\ frozen \in BOOLEAN /\ sourcePython \in BOOLEAN /\ sourceScript \in BOOLEAN
    /\ installedEngine \in BOOLEAN /\ neighbor \in BOOLEAN /\ devRelease \in BOOLEAN
    /\ installedApp \in BOOLEAN /\ marked \in BOOLEAN /\ canLaunch \in BOOLEAN
    /\ protocolSupported \in BOOLEAN /\ wrapper \in BOOLEAN /\ normalized \in BOOLEAN
    /\ custom \in {"valid", "missing", "empty", "nonstring"}
    /\ hfHome \in {"missing", "empty", "set"} /\ target \in {"app", "engine"}
    /\ resourceRoot \in {"none", "assets", "source"} /\ versionRoot \in {"none", "assets", "source"}
    /\ appChoice \in {"none", "neighbor", "dev", "installed", "explicit"}
    /\ runtimeChoice \in {"none", "source_python", "installed_engine", "missing", "python_file", "python_missing", "exe_file", "exe_missing"}
    /\ cacheChoice \in {"none", "custom", "hf_set_hub", "cwd_hub", "home_cache"}
    /\ cacheSource \in {"none", "desktop_config", "HF_HOME", "default"}
    /\ childCache \in {"none", "custom", "hf_set_hub", "cwd_hub", "home_cache", "inherited"}
    /\ inputText \in {"ascii", "unicode"}
    /\ decodedText \in {"none", "ascii", "unicode", "corrupted"}

FrozenResources == phase = "done" /\ scenario = "resource" =>
    resourceRoot = (IF frozen THEN "assets" ELSE "source")
ResourceMetadata == phase = "done" /\ scenario = "resource" =>
    versionRoot = (IF frozen THEN "assets" ELSE "source")
AppPrecedence == phase = "done" /\ scenario = "app" =>
    /\ (explicit = "missing" => appChoice = "none")
    /\ (explicit \in {"marked", "unmarked"} => appChoice = "explicit")
    /\ (explicit = "none" /\ frozen /\ neighbor => appChoice = "neighbor")
    /\ (explicit = "none" /\ (~frozen \/ ~neighbor) /\ devRelease => appChoice = "dev")
    /\ (explicit = "none" /\ (~frozen \/ ~neighbor) /\ ~devRelease /\ installedApp => appChoice = "installed")
    /\ (explicit = "none" /\ (~frozen \/ ~neighbor) /\ ~devRelease /\ ~installedApp => appChoice = "none")
MarkerRequired == phase = "done" /\ scenario = "app" /\ canLaunch =>
    appChoice # "none" /\ (explicit = "marked" \/ (explicit = "none" /\ marked))
CapabilityTruth == phase = "done" /\ scenario = "app" =>
    protocolSupported = (appChoice # "none" /\ (explicit = "marked" \/ (explicit = "none" /\ marked)))
RuntimePrecedence == phase = "done" /\ scenario = "runtime" =>
    /\ (explicit # "none" => runtimeChoice = explicit)
    /\ (explicit = "none" /\ sourcePython /\ sourceScript => runtimeChoice = "source_python")
    /\ (explicit = "none" /\ ~(sourcePython /\ sourceScript) /\ installedEngine => runtimeChoice = "installed_engine")
    /\ (explicit = "none" /\ ~(sourcePython /\ sourceScript) /\ ~installedEngine => runtimeChoice = "missing")
RuntimeLaunchGuard == phase = "done" /\ scenario = "runtime" =>
    canLaunch = (explicit = "exe_file" \/ (explicit = "python_file" /\ sourceScript) \/
                 (explicit = "none" /\ ((sourcePython /\ sourceScript) \/ installedEngine)))
RuntimeWrapper == phase = "done" /\ scenario = "runtime" =>
    wrapper = (runtimeChoice \in {"source_python", "python_file", "python_missing"})
CachePrecedence == phase = "done" /\ scenario = "cache" =>
    /\ (custom = "valid" => cacheChoice = "custom")
    /\ (custom # "valid" /\ hfHome = "set" => cacheChoice = "hf_set_hub")
    /\ (custom # "valid" /\ hfHome = "empty" => cacheChoice = "cwd_hub")
    /\ (custom # "valid" /\ hfHome = "missing" => cacheChoice = "home_cache")
CacheMetadata == phase = "done" /\ scenario = "cache" =>
    cacheSource = (IF custom = "valid" THEN "desktop_config"
                   ELSE IF hfHome # "missing" THEN "HF_HOME" ELSE "default")
CacheEnvironment == phase = "done" /\ scenario = "cache" =>
    /\ (target = "app" => childCache = "inherited")
    /\ (target = "engine" => childCache = cacheChoice)
ResolvedCache == phase = "done" /\ scenario = "cache" => normalized
UTF8InputPreserved == phase = "done" /\ scenario = "encoding" => decodedText = inputText

WitnessReached == phase = "done" /\
    CASE Witness = "frozen" -> scenario = "resource" /\ frozen /\ resourceRoot = "assets" /\ versionRoot = "assets"
      [] Witness = "neighbor" -> scenario = "app" /\ frozen /\ explicit = "none" /\ neighbor /\ devRelease /\ installedApp /\ appChoice = "neighbor" /\ canLaunch
      [] Witness = "unsupported" -> scenario = "app" /\ explicit = "none" /\ devRelease /\ installedApp /\ ~marked /\ appChoice = "dev" /\ ~canLaunch
      [] Witness = "installed_app" -> scenario = "app" /\ frozen /\ explicit = "none" /\ ~neighbor /\ ~devRelease /\ installedApp /\ appChoice = "installed" /\ canLaunch
      [] Witness = "installed_engine" -> scenario = "runtime" /\ explicit = "none" /\ sourcePython /\ ~sourceScript /\ installedEngine /\ runtimeChoice = "installed_engine" /\ ~wrapper /\ canLaunch
      [] Witness = "invalid_explicit" -> scenario = "runtime" /\ explicit = "exe_missing" /\ installedEngine /\ ~canLaunch
      [] Witness = "empty_hf" -> scenario = "cache" /\ custom = "empty" /\ hfHome = "empty" /\ cacheChoice = "cwd_hub" /\ cacheSource = "HF_HOME"
      [] Witness = "app_cache" -> scenario = "cache" /\ custom = "valid" /\ target = "app" /\ childCache = "inherited"
      [] Witness = "utf8" -> scenario = "encoding" /\ inputText = "unicode" /\ decodedText = "unicode"
      [] OTHER -> FALSE
WitnessNotReached == ~WitnessReached
=============================================================================
