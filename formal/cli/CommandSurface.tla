------------------------- MODULE CommandSurface -------------------------
EXTENDS Integers, Sequences

(* Desired Python public command contract. Source binding: CommandSurface.md.
   Existing TargetRouting/EngineClient/Session models retain their separate
   detailed ownership/deadline contracts. This bounded model adds admission,
   wire payload shape and serial native-broker fatal response behavior.

   A session has one input command and either one queued stats or exit record.
   Replies are synchronous. A response arrives and cleanup completes unless
   the selected negative control removes that progress. No environment fairness
   guarantees a desktop connection: startup alone only acquires the broker.
   Reply filtering abstracts the production _receive loop: unmatched or invalid
   fatal noise is followed by a valid matching success response. Atomic dispatch
   is only Python's serial send boundary; native desktop execution is unmodeled.
   Two signed opaque IDs abstract arbitrary Python integers, including repeats.
   Strings abstract Unicode/empty/repeated hot words; executable tests retain
   their exact values. No symmetry, VIEW or state/action constraints are used.
*)

CONSTANTS Mode, Focus, Witness
LocalEngines == {"qwen3-asr-0.6b", "confucius4-r2t2"}
DesktopActions == {"models", "devices", "engine_get", "engine_set", "engine_start", "engine_restart"}
KnownActions == DesktopActions \cup {"status", "stats", "transcribe", "doctor", "unknown"}
Responses == {"ok", "false_missing", "false_flag", "fatal_false", "fatal_true", "unmatched_fatal", "invalid_fatal"}
Profiles == {"commands", "hot_words", "validation", "fatal", "doctor"}
Words == <<"unicode", "", "unicode">>

VARIABLES phase, profile, surface, requestedTarget, target, action, engine,
          fieldsValid, hasHotWords, response, externalId, nextId, tail,
          consumed, rejected, launched, resource, attempts, dispatches,
          wireAction, wireEngine, hotLocation, wireWords, replyIds,
          fatalObserved, exitCode

vars == <<phase, profile, surface, requestedTarget, target, action, engine,
          fieldsValid, hasHotWords, response, externalId, nextId, tail,
          consumed, rejected, launched, resource, attempts, dispatches,
          wireAction, wireEngine, hotLocation, wireWords, replyIds,
          fatalObserved, exitCode>>

ProfileInputs ==
    CASE profile = "commands" ->
           /\ action \in DesktopActions /\ fieldsValid /\ response = "ok" /\ ~hasHotWords
           /\ (action = "engine_set" => engine \in LocalEngines)
      [] profile = "hot_words" ->
           /\ action = "transcribe" /\ fieldsValid /\ response = "ok"
      [] profile = "validation" ->
           /\ action \in {"engine_set", "models", "unknown"}
           /\ response = "ok" /\ ~hasHotWords
      [] profile = "fatal" ->
           /\ action = "status" /\ fieldsValid /\ requestedTarget = "app"
           /\ ~hasHotWords /\ (surface = "session" => tail = "stats")
      [] profile = "doctor" ->
           /\ action = "doctor" /\ surface = "one_shot" /\ fieldsValid
           /\ response = "ok" /\ ~hasHotWords

Init ==
    /\ phase = "select"
    /\ profile \in Profiles /\ (Focus = "all" \/ profile = Focus)
    /\ surface \in {"one_shot", "session"}
    /\ requestedTarget \in {"app", "engine", "omitted"}
    /\ target = "none"
    /\ action \in KnownActions
    /\ engine \in LocalEngines \cup {"cloud", "missing"}
    /\ (action # "engine_set" => engine = "missing")
    /\ fieldsValid \in BOOLEAN /\ hasHotWords \in BOOLEAN
    /\ response \in Responses /\ externalId \in {-7, 80} /\ nextId \in {-7, 80}
    /\ tail \in {"stats", "exit"}
    /\ (surface = "one_shot" => tail = "exit")
    /\ ProfileInputs
    /\ consumed = 0 /\ rejected = FALSE /\ launched = FALSE
    /\ resource = "none" /\ attempts = 0 /\ dispatches = 0
    /\ wireAction = "none" /\ wireEngine = "missing"
    /\ hotLocation = "omitted" /\ wireWords = <<>> /\ replyIds = <<>>
    /\ fatalObserved = FALSE /\ exitCode = 0

ValidRecord == action # "unknown" /\ fieldsValid /\
               (action = "engine_set" => engine \in LocalEngines)
ExpectedReject == ~ValidRecord \/ (target = "engine" /\ action \in DesktopActions)
CurrentAction == IF consumed = 1 THEN action ELSE "stats"
CurrentId == IF consumed = 1 THEN externalId ELSE nextId
MatchingFatal == response \in {"fatal_false", "fatal_true"}
Recoverable == response \in {"false_missing", "false_flag"}
FilteredNoise == response \in {"unmatched_fatal", "invalid_fatal"}

Select ==
    /\ phase = "select"
    /\ target' = IF requestedTarget = "omitted"
                  THEN IF Mode = "DefaultEngine" THEN "engine" ELSE "app"
                  ELSE requestedTarget
    /\ phase' = "validate" /\ consumed' = 1
    /\ UNCHANGED <<profile, surface, requestedTarget, action, engine, fieldsValid,
                    hasHotWords, response, externalId, nextId, tail, rejected,
                    launched, resource, attempts, dispatches, wireAction,
                    wireEngine, hotLocation, wireWords, replyIds, fatalObserved, exitCode>>

Validate ==
    /\ phase = "validate"
    /\ LET badFields == ~ValidRecord /\ ~(Mode = "InvalidEngineAllowed" /\ action = "engine_set")
           badTarget == target = "engine" /\ action \in DesktopActions /\ Mode # "EngineAllowsDesktop"
           reject == badFields \/ badTarget \/ (Mode = "UnsupportedActions" /\ action \in DesktopActions)
           doctor == action = "doctor"
       IN /\ rejected' = reject
          /\ phase' = IF doctor THEN "done" ELSE IF reject
                      THEN IF surface = "session" THEN "tail" ELSE "done"
                      ELSE "dispatch"
          /\ replyIds' = IF reject THEN <<externalId>> ELSE replyIds
          /\ exitCode' = IF reject /\ surface = "one_shot" THEN 2 ELSE 0
          /\ launched' = (doctor /\ Mode = "DoctorLaunch")
          /\ resource' = IF doctor /\ Mode = "DoctorLaunch" THEN "broker" ELSE resource
    /\ UNCHANGED <<profile, surface, requestedTarget, target, action, engine,
                    fieldsValid, hasHotWords, response, externalId, nextId, tail,
                    consumed, attempts, dispatches, wireAction, wireEngine,
                    hotLocation, wireWords, fatalObserved>>

Dispatch ==
    /\ phase = "dispatch"
    /\ phase' = "response" /\ launched' = TRUE
    /\ resource' = IF target = "app" THEN "broker" ELSE "engine"
    /\ attempts' = IF resource = "none" THEN attempts + 1 ELSE attempts
    /\ dispatches' = dispatches + 1
    /\ wireAction' = IF Mode = "WrongWireAction" /\ CurrentAction \in DesktopActions
                     THEN "status" ELSE CurrentAction
    /\ wireEngine' = IF CurrentAction = "engine_set"
                     THEN IF Mode = "WrongEngineField" THEN "missing" ELSE engine
                     ELSE "missing"
    /\ hotLocation' = IF CurrentAction = "transcribe" /\ (hasHotWords \/ Mode = "EmitOmittedHotWords")
                      THEN IF Mode = "NestedHotWords" THEN "options" ELSE "top"
                      ELSE "omitted"
    /\ wireWords' = IF CurrentAction = "transcribe" /\ hasHotWords THEN Words ELSE <<>>
    /\ UNCHANGED <<profile, surface, requestedTarget, target, action, engine,
                    fieldsValid, hasHotWords, response, externalId, nextId, tail,
                    consumed, rejected, replyIds, fatalObserved, exitCode>>

Receive ==
    /\ phase = "response"
    /\ LET fatal == consumed = 1 /\ ((MatchingFatal /\ Mode # "FatalRecovered") \/
                                    (FilteredNoise /\ Mode = "AcceptFatalNoise"))
           terminal == fatal \/ (consumed = 1 /\ Recoverable /\ Mode = "NonFatalTerminal")
           last == surface = "one_shot" \/ consumed = 2
       IN /\ fatalObserved' = (fatalObserved \/ (consumed = 1 /\ MatchingFatal))
          /\ phase' = IF terminal \/ last THEN "closing" ELSE "tail"
          /\ replyIds' = Append(replyIds, IF Mode = "WrongReplyIdentity" THEN consumed ELSE CurrentId)
          /\ exitCode' = IF terminal \/ (last /\ consumed = 1 /\ Recoverable) THEN 1 ELSE 0
          /\ consumed' = IF fatal /\ Mode = "FatalReadsNext" THEN 2 ELSE consumed
          /\ attempts' = IF fatal /\ Mode = "FatalRetry" THEN attempts + 1 ELSE attempts
    /\ UNCHANGED <<profile, surface, requestedTarget, target, action, engine,
                    fieldsValid, hasHotWords, response, externalId, nextId, tail,
                    rejected, launched, resource, dispatches, wireAction,
                    wireEngine, hotLocation, wireWords>>

ReadTail ==
    /\ phase = "tail"
    /\ consumed' = 2
    /\ phase' = IF tail = "exit" THEN "closing" ELSE "dispatch"
    /\ replyIds' = IF tail = "exit" THEN Append(replyIds, nextId) ELSE replyIds
    /\ UNCHANGED <<profile, surface, requestedTarget, target, action, engine,
                    fieldsValid, hasHotWords, response, externalId, nextId, tail,
                    rejected, launched, resource, attempts, dispatches, wireAction,
                    wireEngine, hotLocation, wireWords, fatalObserved, exitCode>>

Close ==
    /\ phase = "closing" /\ Mode # "CleanupStuck"
    /\ phase' = "done"
    /\ resource' = IF Mode = "ForgetReap" THEN resource ELSE "none"
    /\ UNCHANGED <<profile, surface, requestedTarget, target, action, engine,
                    fieldsValid, hasHotWords, response, externalId, nextId, tail,
                    consumed, rejected, launched, attempts, dispatches, wireAction,
                    wireEngine, hotLocation, wireWords, replyIds, fatalObserved, exitCode>>

Done == /\ phase = "done" /\ UNCHANGED vars
Stall == /\ phase = "closing" /\ Mode = "CleanupStuck" /\ UNCHANGED vars
Advance == Select \/ Validate \/ Dispatch \/ Receive \/ ReadTail \/ Close
Spec == Init /\ [][Advance \/ Done \/ Stall]_vars /\ WF_vars(Advance)

TypeOK ==
    /\ phase \in {"select", "validate", "dispatch", "response", "tail", "closing", "done"}
    /\ profile \in Profiles /\ surface \in {"one_shot", "session"}
    /\ requestedTarget \in {"app", "engine", "omitted"} /\ target \in {"app", "engine", "none"}
    /\ action \in KnownActions /\ engine \in LocalEngines \cup {"cloud", "missing"}
    /\ fieldsValid \in BOOLEAN /\ hasHotWords \in BOOLEAN /\ response \in Responses
    /\ externalId \in {-7, 80} /\ nextId \in {-7, 80} /\ tail \in {"exit", "stats"}
    /\ consumed \in 0..2 /\ rejected \in BOOLEAN /\ launched \in BOOLEAN
    /\ resource \in {"none", "broker", "engine"} /\ attempts \in 0..2 /\ dispatches \in 0..2
    /\ wireAction \in KnownActions \cup {"none"} /\ wireEngine \in LocalEngines \cup {"cloud", "missing"}
    /\ hotLocation \in {"omitted", "top", "options"} /\ wireWords \in {<<>>, Words}
    /\ replyIds \in Seq({-7, 80, 1, 2}) /\ Len(replyIds) <= 2
    /\ fatalObserved \in BOOLEAN /\ exitCode \in {0, 1, 2}

DefaultApp == requestedTarget = "omitted" /\ phase # "select" => target = "app"
Admission == phase \notin {"select", "validate"} => rejected = ExpectedReject
RejectBeforeSpawn == ExpectedReject /\ consumed = 1 => ~launched /\ dispatches = 0
WireActionMatches == phase = "response" => wireAction = CurrentAction
EngineFieldMatches == phase = "response" /\ CurrentAction = "engine_set" => wireEngine = engine
HotWordsAtTop == phase = "response" /\ CurrentAction = "transcribe" /\ hasHotWords =>
                   hotLocation = "top" /\ wireWords = Words
OmittedHotWordsAbsent == phase = "response" /\ CurrentAction = "transcribe" /\ ~hasHotWords =>
                         hotLocation = "omitted"
FatalTerminates == fatalObserved => phase \in {"closing", "done"} /\ exitCode = 1
FatalDoesNotReadTail == fatalObserved => consumed = 1 /\ dispatches = 1 /\ Len(replyIds) = 1
NoAutomaticRetry == attempts <= 1
ReplyIdentity == Len(replyIds) > 0 =>
                   /\ replyIds[1] = externalId
                   /\ (Len(replyIds) = 2 => replyIds[2] = nextId)
NonFatalContinues == profile = "fatal" /\ surface = "session" /\ Recoverable /\ phase = "done" =>
                      consumed = 2 /\ dispatches = 2 /\ Len(replyIds) = 2 /\ exitCode = 0
FatalNoiseFiltered == profile = "fatal" /\ FilteredNoise /\ phase = "done" =>
                       ~fatalObserved /\ exitCode = 0 /\ (surface = "session" => dispatches = 2)
OwnedCleanup == phase = "done" => resource = "none"
DoctorReadOnly == action = "doctor" => ~launched /\ resource = "none" /\ dispatches = 0
EventuallyDone == <> (phase = "done")

WitnessReached == phase = "done" /\
    CASE Witness = "commands" -> action = "engine_restart" /\ surface = "session" /\ dispatches = 2
      [] Witness = "default_app" -> requestedTarget = "omitted" /\ action = "models" /\ ~rejected /\ target = "app"
      [] Witness = "engine_rejected" -> target = "engine" /\ action = "models" /\ rejected /\ ~launched /\ tail = "exit"
      [] Witness = "invalid_engine" -> action = "engine_set" /\ engine = "cloud" /\ rejected /\ ~launched /\ tail = "exit"
      [] Witness = "engine_field" -> action = "engine_set" /\ ~rejected /\ surface = "one_shot" /\ wireEngine \in LocalEngines
      [] Witness = "hot_words" -> action = "transcribe" /\ hasHotWords /\ hotLocation = "top" /\ wireWords = Words
      [] Witness = "omitted_words" -> action = "transcribe" /\ ~hasHotWords /\ hotLocation = "omitted"
      [] Witness = "fatal" -> surface = "session" /\ fatalObserved /\ exitCode = 1 /\ consumed = 1 /\ resource = "none"
      [] Witness = "fatal_success" -> surface = "session" /\ response = "fatal_true" /\ fatalObserved /\ exitCode = 1
      [] Witness = "identity" -> surface = "session" /\ externalId = -7 /\ nextId = -7 /\ replyIds = <<-7, -7>>
      [] Witness = "recoverable" -> surface = "session" /\ Recoverable /\ dispatches = 2 /\ exitCode = 0
      [] Witness = "noise" -> surface = "session" /\ FilteredNoise /\ dispatches = 2 /\ exitCode = 0
      [] Witness = "cleanup" -> launched /\ resource = "none" /\ attempts = 1
      [] Witness = "doctor" -> action = "doctor" /\ ~launched /\ dispatches = 0
      [] OTHER -> FALSE
WitnessNotReached == ~WitnessReached
=============================================================================
