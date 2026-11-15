----------------------------- MODULE AppQuery -----------------------------
EXTENDS Integers, Sequences, FiniteSets

(* Managed query contract; executable source identities live in each receipt.
   Production path: controller::query -> AppState native_asr_owner/recording
   guard -> bounded file read/owned copy -> send_command_to_server -> cleanup.
   Two callers share one ASR owner and actual native process. External owner 3
   represents an existing native recording/probe. Saved R2T2 and observed Qwen
   differ deliberately. Native response is healthy, application error, EOF or
   malformed stats/transcript; a failed operation never invents successful
   observations. transcript_absent represents missing/null text (Option::None)
   after native success=true. Explicit empty text is valid silence, distinct
   from absent text. Wrong-typed text uses the existing IPC-error partition.
   OwnerAbort means app-owned future cancellation. Transport disconnect and
   Wry lifecycle/watch wiring are explicitly outside this first query slice.
   Reap models asynchronous OS termination following CommandProcessGuard Drop.
   Fairness covers enabled local query/cleanup steps and OS reaping, not success
   of inference. Byte classes represent valid, missing, cloud and >64MiB input;
   no reader seam yet proves a concurrent file-growth schedule in native code.
*)
CONSTANTS Mode, Witness
VARIABLE s
vars == <<s>>
Clients == {1, 2}
Scenarios == {"recording", "processing", "owner", "status", "stats",
  "malformed", "application", "eof", "transcribe", "oversize", "cloud",
  "missing", "absent_status", "absent_stats", "transcript_absent", "silence"}
Phases == {"new", "admitted", "prepared", "ipc", "settling", "done"}
Working == {"admitted", "prepared", "ipc", "settling"}
Pairs(k) == CASE k \in {"recording", "processing", "owner"} ->
                   {<<"status", "transcribe">>, <<"stats", "engine_get">>}
             [] k \in {"status", "absent_status"} -> {<<"status", "engine_get">>}
             [] k \in {"stats", "malformed", "absent_stats"} -> {<<"stats", "engine_get">>}
             [] k \in {"application", "eof"} ->
                   {<<"status", "engine_get">>, <<"stats", "engine_get">>, <<"transcribe", "engine_get">>}
             [] k = "transcribe" -> {<<"transcribe", "status">>, <<"transcribe", "stats">>, <<"transcribe", "transcribe">>}
             [] OTHER -> {<<"transcribe", "engine_get">>}
InputInvalid == s.scenario \in {"oversize", "cloud", "missing"}
BackendBad == s.scenario \in {"application", "eof", "malformed", "transcript_absent"}
IsBlocked == (s.scenario = "recording" /\ Mode # "IgnoreRecording")
          \/ (s.scenario = "processing" /\ Mode # "IgnoreProcessing")
          \/ (s.owner # 0 /\ Mode # "IgnoreOwner")

Init == \E k \in Scenarios : \E pair \in Pairs(k) : s =
  [scenario |-> k, action |-> pair,
   phase |-> [c \in Clients |-> "new"], owner |-> IF k = "owner" THEN 3 ELSE 0,
   child |-> IF k \in {"absent_status", "absent_stats"} THEN "absent" ELSE "live",
   temp |-> {}, commands |-> <<>>, reply |-> [c \in Clients |-> "none"],
   observed |-> [c \in Clients |-> FALSE], runtime |-> [c \in Clients |-> "unknown"],
   count |-> [c \in Clients |-> -1], busy |-> {}, aborted |-> {}, abortIpc |-> {},
   admitted |-> {}, saved |-> "r2t2", invalidEffect |-> FALSE,
   transcript |-> [c \in Clients |-> "unknown"]]

Begin(c) ==
  /\ s.phase[c] = "new"
  /\ IF s.action[c] = "engine_get"
     THEN s' = [s EXCEPT !.phase[c] = "done", !.reply[c] = "saved"]
     ELSE IF IsBlocked
       THEN s' = [s EXCEPT !.phase[c] = "done", !.busy = @ \cup {c},
          !.reply[c] = IF s.action[c] = "status" THEN "busy_status" ELSE "busy_error",
          !.observed[c] = (Mode = "InventBusyRuntime"),
          !.runtime[c] = IF Mode = "InventBusyRuntime" THEN s.saved ELSE "unknown"]
       ELSE s' = [s EXCEPT !.phase[c] = "admitted", !.owner = c,
                            !.admitted = @ \cup {c}]

Prepare(c) ==
  /\ s.phase[c] = "admitted"
  /\ IF s.action[c] = "transcribe" /\ InputInvalid
          /\ ~(s.scenario = "oversize" /\ Mode = "IgnoreSize")
          /\ ~(s.scenario = "cloud" /\ Mode = "IgnoreCloud")
     THEN s' = [s EXCEPT !.phase[c] = "settling", !.reply[c] = "input_error"]
     ELSE IF s.child \in {"absent", "dead"}
       THEN s' = [s EXCEPT !.phase[c] = "settling",
                !.reply[c] = IF s.action[c] = "status" THEN "stopped" ELSE "backend_error"]
       ELSE s' = [s EXCEPT !.phase[c] = "prepared",
            !.temp = IF s.action[c] = "transcribe" THEN @ \cup {c} ELSE @,
            !.invalidEffect = @ \/ (s.action[c] = "transcribe" /\ InputInvalid)]

Issue(c) ==
  /\ s.phase[c] = "prepared"
  /\ s' = [s EXCEPT !.phase[c] = "ipc", !.commands = Append(@, c),
             !.owner = IF Mode = "ReleaseEarly" THEN 0 ELSE @]

Respond(c) ==
  /\ s.phase[c] = "ipc"
  /\ LET bad == BackendBad
         hidden == bad /\ (Mode = "HideBackendError"
                   \/ (Mode = "IgnoreTranscriptField" /\ s.scenario = "transcript_absent"))
         kind == IF bad /\ ~hidden THEN "backend_error"
                 ELSE CASE s.action[c] = "status" -> "observed_status"
                        [] s.action[c] = "stats" -> "observed_stats"
                        [] OTHER -> "transcribed"
     IN s' = [s EXCEPT !.phase[c] = "settling", !.reply[c] = kind,
        !.observed[c] = (kind = "observed_status"),
        !.runtime[c] = IF kind = "observed_status"
                        THEN IF Mode = "UseSavedRuntime" THEN s.saved ELSE "qwen"
                        ELSE "unknown",
        !.count[c] = IF kind = "observed_stats" THEN IF Mode = "InventStats" THEN 0 ELSE 7 ELSE -1,
        !.transcript[c] = IF kind # "transcribed" THEN "unknown"
                         ELSE IF s.scenario = "transcript_absent" THEN "missing"
                         ELSE IF s.scenario = "silence"
                           THEN IF Mode = "FillSilence" THEN "invented" ELSE "empty"
                         ELSE "text",
        !.child = IF s.scenario = "eof" THEN "stopping" ELSE @]

OwnerAbort(c) ==
  /\ s.phase[c] \in {"admitted", "prepared", "ipc"}
  /\ s' = [s EXCEPT !.phase[c] = "settling", !.reply[c] = "aborted",
       !.aborted = @ \cup {c},
       !.abortIpc = IF s.phase[c] = "ipc" THEN @ \cup {c} ELSE @,
       !.child = IF s.phase[c] = "ipc" /\ Mode # "KeepChildOnAbort" THEN "stopping" ELSE @]

Cleanup(c) ==
  /\ s.phase[c] = "settling"
  /\ s' = [s EXCEPT !.phase[c] = "done",
         !.temp = IF Mode = "LeakTemp" THEN @ ELSE @ \ {c},
         !.owner = IF Mode = "ForgetRelease" THEN @ ELSE IF @ = c THEN 0 ELSE @]

Reap == /\ s.child = "stopping" /\ Mode # "SkipReap"
        /\ s' = [s EXCEPT !.child = "dead"]
(* A terminal self-loop also permits a stuck OS reap. The progress property,
   rather than an unrelated deadlock, must expose the SkipReap control. *)
Terminal == /\ \A c \in Clients : s.phase[c] = "done" /\ UNCHANGED s
Step(c) == Begin(c) \/ Prepare(c) \/ Issue(c) \/ Respond(c) \/ OwnerAbort(c) \/ Cleanup(c)
Next == (\E c \in Clients : Step(c)) \/ Reap \/ Terminal
Spec == Init /\ [][Next]_vars /\ (\A c \in Clients : WF_vars(Step(c))) /\ WF_vars(Reap)

TypeOK == /\ s.phase \in [Clients -> Phases] /\ s.owner \in {0,1,2,3}
          /\ s.child \in {"live", "absent", "stopping", "dead"}
          /\ s.temp \subseteq Clients /\ s.busy \subseteq Clients
          /\ s.transcript \in [Clients -> {"unknown", "missing", "empty", "text", "invented"}]
PermitUntilSettled == \A c \in Clients : s.phase[c] \in Working => s.owner = c
RecordingGuard == s.scenario = "recording" => s.admitted = {}
ProcessingGuard == s.scenario = "processing" => s.admitted = {}
ExternalOwnerGuard == s.scenario = "owner" => s.admitted = {}
NoEffectsForInvalidInput == ~s.invalidEffect
NoTemporaryAfterReturn == \A c \in Clients : s.phase[c] = "done" => c \notin s.temp
NoOwnedLeaseAfterReturn == \A c \in Clients : s.phase[c] = "done" => s.owner # c
BusyRuntimeUnknown == \A c \in s.busy : ~s.observed[c] /\ s.runtime[c] = "unknown"
ActualRuntimeIdentity == \A c \in Clients : s.observed[c] => s.runtime[c] = "qwen"
ActualStats == \A c \in Clients : s.reply[c] = "observed_stats" => s.count[c] = 7
BackendErrorsExplicit == BackendBad => \A c \in Clients : s.reply[c] \notin {"observed_status", "observed_stats", "transcribed"}
SilenceIsEmpty == s.scenario = "silence" => \A c \in Clients :
                   s.reply[c] = "transcribed" => s.transcript[c] = "empty"
EngineGetNoIpc == \A n \in 1..Len(s.commands) : s.action[s.commands[n]] # "engine_get"
OwnerAbortStopsNative == s.abortIpc # {} => s.child \in {"stopping", "dead"}
AllQueriesSettle == <>(\A c \in Clients : s.phase[c] = "done")
OwnerAbortEventuallyReaps == [](s.abortIpc # {} => <>(s.child = "dead"))

Target == CASE Witness = "recording" -> s.scenario = "recording" /\ s.busy # {}
            [] Witness = "processing" -> s.scenario = "processing" /\ s.busy # {}
            [] Witness = "owner" -> s.scenario = "owner" /\ s.busy # {}
            [] Witness = "competing" -> s.scenario = "transcribe" /\ s.busy # {} /\ s.admitted # {}
            [] Witness = "temp" -> s.temp # {} /\ \E c \in Clients : s.phase[c] = "ipc"
            [] Witness = "abort" -> s.abortIpc # {} /\ s.child = "dead"
            [] Witness = "stats" -> \E c \in Clients : s.reply[c] = "observed_stats"
            [] Witness = "status" -> \E c \in Clients : s.reply[c] = "observed_status"
            [] Witness = "oversize" -> s.scenario = "oversize" /\ \E c \in Clients : s.reply[c] = "input_error"
            [] Witness = "cloud" -> s.scenario = "cloud" /\ \E c \in Clients : s.reply[c] = "input_error"
            [] Witness = "ipc_error" -> s.scenario = "eof" /\ \E c \in Clients : s.reply[c] = "backend_error"
            [] Witness = "saved" -> \E c \in Clients : s.reply[c] = "saved"
            [] Witness = "transcript_absent" -> s.scenario = "transcript_absent" /\ \E c \in Clients : s.reply[c] = "backend_error"
            [] Witness = "silence" -> s.scenario = "silence" /\ \E c \in Clients : s.reply[c] = "transcribed" /\ s.transcript[c] = "empty"
            [] OTHER -> FALSE
WitnessNotReached == ~Target
=============================================================================
