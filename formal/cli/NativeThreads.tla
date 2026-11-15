-------------------------- MODULE NativeThreads --------------------------
EXTENDS Integers

(* Thread-budget decision model, not model inference or an OS-thread-count claim.
   CLI owner: scripts/light_whisper_cli.py main -> positive_threads ->
   request_client -> owned_engine -> EngineClient(env) -> subprocess.Popen.
   SDK owner: qwen3_native.QwenNativeRuntime.__init__ ->
   r2t2_native.NativeRuntime.__init__ -> _BackendConfig -> session_create.
   Both server loaders omit threads; the SDK therefore owns env/default choice.
   BaseASRServer._setup_runtime_environment also writes OMP_NUM_THREADS; that
   unrelated variable must not change the old native default of four.

   Prepare represents synchronous CLI validation and child-environment copy.
   Select represents SDK selection and validation before DLL-directory/CDLL,
   registry/model/options allocation. Allocate represents the later ABI call.
   No await/callback exists between these decisions. Environment mutation by
   unrelated threads during construction is outside the contract. The app path
   ends at broker spawn: it cannot reconfigure an already-running desktop.

   Input classes retain lower/upper valid ABI boundaries, one interior value,
   absent/None, zero, negative, overflow, text, fraction and boolean partitions.
   Invalid classes differ by validation rule, not their exact spelling. max is
   2^31-1 because _BackendConfig.threads is ct.c_int. CPU/CUDA and both model
   kinds share this parameter; no backend selection transition is introduced.

   No scheduler/liveness claim: these finite, synchronous decisions have an
   enabled next action until Done; terminal stuttering preserves deadlock checks.
*)

CONSTANTS Mode, Witness

ValidCases == {"one", "two", "three", "four", "max"}
ExplicitCases == {"omitted", "none", "one", "two", "three", "max",
                  "zero", "negative", "overflow", "numeric_string", "float", "bool"}
EnvironmentCases == {"absent", "one", "two", "max", "zero", "negative",
                     "overflow", "text", "fraction", "empty"}
CLIValidCases == {"default", "one", "two", "max"}
CLICases == CLIValidCases \cup {"zero", "negative", "overflow", "text"}
Maximum == 2147483647

Value(input) ==
    CASE input = "one" -> 1
      [] input = "two" -> 2
      [] input = "three" -> 3
      [] input = "four" -> 4
      [] input = "max" -> Maximum
      [] input = "many" -> 19
      [] input = "negative" -> -1
      [] input = "overflow" -> Maximum + 1
      [] OTHER -> 0

VARIABLES phase, entry, argument, cliArgument, dedicated, omp, backend, family,
          transported, selectedCase, selectedSource, rejected, exitCode,
          spawned, allocated, nativeThreads

vars == <<phase, entry, argument, cliArgument, dedicated, omp, backend, family,
          transported, selectedCase, selectedSource, rejected, exitCode,
          spawned, allocated, nativeThreads>>

Implicit == argument \in {"omitted", "none"}
CLICase == IF cliArgument = "default" THEN "two" ELSE cliArgument
NativeEntry == entry = "sdk" \/ (entry = "engine" /\ cliArgument \in CLIValidCases)

Init ==
    /\ entry \in {"sdk", "engine", "app"}
    /\ argument \in ExplicitCases
    /\ cliArgument \in CLICases
    /\ (entry = "sdk" => cliArgument = "default")
    /\ (entry # "sdk" => argument = "omitted")
    /\ dedicated \in EnvironmentCases
    /\ omp \in {"absent", "many"}
    /\ backend \in {"cpu", "cuda"}
    /\ family \in {"r2t2", "qwen"}
    /\ phase = IF entry = "sdk" THEN "select" ELSE "prepare"
    /\ transported = IF entry = "sdk" THEN dedicated ELSE "unprepared"
    /\ selectedCase = "unselected"
    /\ selectedSource = "none"
    /\ rejected = FALSE
    /\ exitCode = 0
    /\ spawned = FALSE
    /\ allocated = FALSE
    /\ nativeThreads = 0

Prepare ==
    /\ phase = "prepare"
    /\ LET valid == cliArgument \in CLIValidCases \/ Mode = "SkipCLIValidation"
           environment == IF entry = "engine"
                          THEN IF Mode = "MissingCLIEnvironment" THEN dedicated ELSE CLICase
                          ELSE IF Mode = "OverrideAppEnvironment" THEN CLICase ELSE dedicated
       IN /\ rejected' = ~valid
          /\ exitCode' = IF valid THEN 0 ELSE 2
          /\ spawned' = valid
          /\ transported' = IF valid THEN environment ELSE "unprepared"
          /\ phase' = IF valid /\ entry = "engine" THEN "select" ELSE "done"
    /\ UNCHANGED <<entry, argument, cliArgument, dedicated, omp, backend, family,
                   selectedCase, selectedSource, allocated, nativeThreads>>

Select ==
    /\ phase = "select"
    /\ LET ignoreEnvironment == Mode = "IgnoreEnvironment" /\ Implicit
           environmentFirst == Mode = "EnvironmentBeforeExplicit" /\ transported # "absent"
           useExplicit == ~Implicit /\ ~environmentFirst
           useEnvironment == transported # "absent" /\ ~useExplicit /\ ~ignoreEnvironment
           candidate == IF useExplicit THEN argument
                        ELSE IF useEnvironment THEN transported
                        ELSE IF Mode = "ReadOMP" /\ omp = "many" THEN "many"
                        ELSE "four"
           source == IF useExplicit THEN "explicit"
                     ELSE IF useEnvironment THEN "environment" ELSE "default"
           valid == candidate \in ValidCases \/ Mode = "SkipValidation"
       IN /\ selectedCase' = candidate
          /\ selectedSource' = source
          /\ rejected' = ~valid
          /\ exitCode' = IF valid THEN 0 ELSE 2
          /\ phase' = IF valid THEN "validated" ELSE "done"
    /\ UNCHANGED <<entry, argument, cliArgument, dedicated, omp, backend, family,
                   transported, spawned, allocated, nativeThreads>>

Allocate ==
    /\ phase = "validated"
    /\ allocated' = TRUE
    /\ nativeThreads' = IF Mode = "ForceFourAtABI" THEN 4 ELSE Value(selectedCase)
    /\ phase' = "done"
    /\ UNCHANGED <<entry, argument, cliArgument, dedicated, omp, backend, family,
                   transported, selectedCase, selectedSource, rejected, exitCode, spawned>>

Done == /\ phase = "done" /\ UNCHANGED vars
Next == Prepare \/ Select \/ Allocate \/ Done
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in {"prepare", "select", "validated", "done"}
    /\ entry \in {"sdk", "engine", "app"}
    /\ argument \in ExplicitCases
    /\ cliArgument \in CLICases
    /\ dedicated \in EnvironmentCases
    /\ omp \in {"absent", "many"}
    /\ backend \in {"cpu", "cuda"}
    /\ family \in {"r2t2", "qwen"}
    /\ transported \in EnvironmentCases \cup {"unprepared", "one", "two", "max"}
    /\ selectedCase \in ExplicitCases \cup EnvironmentCases \cup {"unselected", "four", "many"}
    /\ selectedSource \in {"none", "explicit", "environment", "default"}
    /\ rejected \in BOOLEAN
    /\ exitCode \in {0, 2}
    /\ spawned \in BOOLEAN
    /\ allocated \in BOOLEAN
    /\ nativeThreads \in Int

CLIRejectsBeforeSpawn ==
    entry # "sdk" /\ cliArgument \notin CLIValidCases /\ phase = "done" =>
      rejected /\ exitCode = 2 /\ ~spawned /\ ~allocated /\ transported = "unprepared"

EngineEnvironment ==
    entry = "engine" /\ cliArgument \in CLIValidCases /\ phase # "prepare" =>
      transported = CLICase

AppEnvironment ==
    entry = "app" /\ cliArgument \in CLIValidCases /\ phase = "done" =>
      transported = dedicated /\ ~allocated

(* Independent input implications express precedence, not the Select algorithm. *)
SelectionPrecedence ==
    NativeEntry /\ phase \in {"validated", "done"} =>
      /\ (argument \in ValidCases =>
           selectedCase = argument /\ selectedSource = "explicit" /\ ~rejected)
      /\ (Implicit /\ transported \in ValidCases =>
           selectedCase = transported /\ selectedSource = "environment" /\ ~rejected)

OMPDefault ==
    NativeEntry /\ Implicit /\ transported = "absent" /\ phase \in {"validated", "done"} =>
      selectedCase = "four" /\ selectedSource = "default" /\ ~rejected

ValidationBeforeAllocation == allocated => selectedCase \in ValidCases

InvalidChoiceRejected ==
    NativeEntry /\ phase = "done" /\
      ((~Implicit /\ argument \notin ValidCases) \/
       (Implicit /\ transported # "absent" /\ transported \notin ValidCases)) =>
      rejected /\ exitCode = 2 /\ ~allocated

ValidChoiceAdmitted ==
    NativeEntry /\ phase = "done" /\
      (argument \in ValidCases \/
       (Implicit /\ (transported = "absent" \/ transported \in ValidCases))) =>
      ~rejected /\ exitCode = 0 /\ allocated

ABIExactness ==
    allocated => nativeThreads = Value(selectedCase) /\
                 nativeThreads >= 1 /\ nativeThreads <= Maximum

WitnessReached ==
    phase = "done" /\
      CASE Witness = "environment" -> entry = "sdk" /\ argument = "none" /\
              dedicated = "two" /\ backend = "cuda" /\ family = "qwen" /\
              allocated /\ nativeThreads = 2
        [] Witness = "explicit_invalid_environment" -> entry = "sdk" /\ argument = "three" /\
              dedicated = "text" /\ allocated /\ nativeThreads = 3
        [] Witness = "invalid_environment" -> entry = "sdk" /\ argument = "omitted" /\
              dedicated = "overflow" /\ rejected /\ ~allocated
        [] Witness = "engine_override" -> entry = "engine" /\ cliArgument = "two" /\
              dedicated = "max" /\ allocated /\ nativeThreads = 2
        [] Witness = "app_absence" -> entry = "app" /\ cliArgument = "two" /\
              dedicated = "absent" /\ transported = "absent" /\ spawned /\ ~allocated
        [] OTHER -> FALSE

WitnessNotReached == ~WitnessReached
=============================================================================
