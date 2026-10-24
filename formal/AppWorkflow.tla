---------------------------- MODULE AppWorkflow ----------------------------
EXTENDS Naturals

CONSTANT Workload
ASSUME Workload \in {"Speech", "CloudSpeech", "Selection", "Management",
                    "Mixed", "EngineSpeech", "AssistantWeb", "Subtitles", "History", "GpuIdle"}

Modes == {"Dictation", "Translation", "Edit", "Assistant"}
PolishModes == {"Off", "On", "Auto"}
SelectionActions == {"Translate", "Explain", "Optimize"}
Windows == {"Unknown", "A", "B"}

AllowSpeech == Workload \in {"Speech", "CloudSpeech", "Mixed", "EngineSpeech",
                            "AssistantWeb", "Subtitles", "GpuIdle"}
AllowSelection == Workload \in {"Selection", "Mixed"}
AllowSettings == Workload \in {"Speech", "CloudSpeech", "Management", "Mixed",
                              "EngineSpeech", "AssistantWeb"}
AllowManagement == Workload \in {"Management", "EngineSpeech"}

MixedPolicy == [history |-> TRUE, audio |-> TRUE, screen |-> TRUE,
                polish |-> "Auto", input |-> "Clipboard", web |-> FALSE]
WebPolicy == [history |-> FALSE, audio |-> FALSE, screen |-> TRUE,
              polish |-> "Off", input |-> "SendInput", web |-> TRUE]

Policies == {
  [history |-> FALSE, audio |-> FALSE, screen |-> FALSE,
   polish |-> "Off", input |-> "SendInput", web |-> FALSE],
  [history |-> TRUE, audio |-> FALSE, screen |-> FALSE,
   polish |-> "On", input |-> "SendInput", web |-> FALSE],
  [history |-> TRUE, audio |-> TRUE, screen |-> TRUE,
   polish |-> "Auto", input |-> "Clipboard", web |-> FALSE],
  [history |-> FALSE, audio |-> TRUE, screen |-> TRUE,
   polish |-> "Auto", input |-> "Clipboard", web |-> FALSE],
  WebPolicy
}

VARIABLE app

OutputReadyForArchive ==
  Workload = "History"
    \/ (app.terminalPublished
        /\ (app.mode = "Assistant" \/ app.result = "Error" \/ app.paste # "None"))

Init == app = [
  engine |-> IF Workload \in {"CloudSpeech", "AssistantWeb"}
            THEN "Cloud" ELSE "Local",
  runtime |-> IF Workload \in {"CloudSpeech", "AssistantWeb"}
             THEN "NeedKey" ELSE "Missing",
  localInstalled |-> FALSE,
  asrKey |-> FALSE, llmKey |-> FALSE,
  download |-> "Idle", downloadEpoch |-> 0, configEpoch |-> 0,
  engineSwitches |-> 0, engineRequest |-> "None", requestEpoch |-> 0,
  rec |-> "Idle", mode |-> IF Workload = "History" THEN "Dictation" ELSE "None",
  target |-> "Unknown",
  foreground |-> "A", foregroundChanges |-> 0,
  asr |-> "None", result |-> IF Workload = "History" THEN "Raw" ELSE "None",
  historyOn |-> FALSE, audioOn |-> FALSE, screenOn |-> FALSE,
  webOn |-> FALSE, webSearch |-> "None", webAllowed |-> FALSE,
  polishMode |-> "Off", inputMethod |-> "SendInput",
  historyAllowed |-> Workload = "History",
  audioAllowed |-> Workload = "History",
  screenAllowed |-> FALSE, screenUsed |-> FALSE,
  screenshotWindow |-> "Unknown", screenPrep |-> "None",
  screenSent |-> FALSE, dispatchWindow |-> "Unknown",
  historyStored |-> Workload = "History",
  historyFinalized |-> Workload = "History", audioWrite |-> "None",
  audioStored |-> Workload = "History",
  audioFileExists |-> Workload = "History", audioLease |-> FALSE,
  reprocess |-> "Idle", historyQueried |-> FALSE, historyExported |-> FALSE,
  displayed |-> FALSE, terminalPublished |-> Workload = "History",
  paste |-> "None", clipboard |-> "Original",
  clipboardSnapshot |-> "Original", pasteExternalChanged |-> FALSE,
  externalPreservedAtFinish |-> TRUE,
  selVersion |-> 0, selPhase |-> "Idle", selAction |-> "None",
  selWindow |-> "Unknown", selRequestVersion |-> 0,
  selResultVersion |-> 0, selectionReplaced |-> FALSE,
  replacementWindow |-> "Unknown", sourceMatches |-> FALSE,
  replacementSourceMatched |-> FALSE,
  selectionSearched |-> FALSE, historyReprocessed |-> FALSE,
  chat |-> "Idle", settingsVersion |-> 0, savedVersion |-> 0,
  updateCheck |-> "Unchecked",
  subtitle |-> "Idle", subtitleSession |-> 0, subtitleFinalSeen |-> FALSE,
  staleSubtitleIgnored |-> FALSE
  , gpuEnabled |-> FALSE, gpuLoaded |-> FALSE, gpuAge |-> 0,
  gpuSuspended |-> FALSE, gpuUnsafeUnload |-> FALSE
]

\* Settings and credentials are user actions. The profile component model
\* checks that a queued save cannot persist an older settings version.
Configure(policy) ==
  /\ AllowSettings
  /\ app.settingsVersion = 0
  /\ policy \in Policies
  /\ (Workload \notin {"Mixed", "EngineSpeech"} \/ policy = MixedPolicy)
  /\ (Workload # "AssistantWeb" \/ policy = WebPolicy)
  /\ app' = [app EXCEPT
       !.historyOn = policy.history, !.audioOn = policy.audio,
       !.screenOn = policy.screen, !.polishMode = policy.polish,
       !.webOn = policy.web, !.inputMethod = policy.input,
       !.settingsVersion = 1]

SaveSettings ==
  /\ app.settingsVersion > app.savedVersion
  /\ app' = [app EXCEPT !.savedVersion = app.settingsVersion]

SetLlmCredential ==
  /\ ~app.llmKey
  /\ app' = [app EXCEPT !.llmKey = TRUE]

RequestEngineSwitch ==
  /\ AllowManagement
  /\ app.engineSwitches = 0 /\ app.engineRequest = "None"
  /\ app' = [app EXCEPT !.engineRequest = "Pending",
       !.requestEpoch = app.configEpoch]

SwitchEngine ==
  /\ AllowManagement
  /\ app.engineSwitches = 0 /\ app.engineRequest = "Pending"
  /\ app.download \notin {"Running", "Cancelling"}
  /\ app.rec \notin {"Starting", "Recording", "Asr", "PostAsr", "Preparing", "Llm"}
  /\ app' = [app EXCEPT
       !.engine = "Cloud", !.runtime = "NeedKey",
       !.configEpoch = @ + 1, !.engineSwitches = 1,
       !.engineRequest = "Done"]

RejectEngineSwitch ==
  /\ app.engineRequest = "Pending"
  /\ (app.download \in {"Running", "Cancelling"}
      \/ app.rec \in {"Starting", "Recording", "Asr", "PostAsr", "Preparing", "Llm"})
  /\ app' = [app EXCEPT !.engineRequest = "Blocked"]

SetAsrCredential ==
  /\ app.engine = "Cloud"
  /\ app.runtime = "NeedKey"
  /\ app' = [app EXCEPT !.asrKey = TRUE, !.runtime = "Ready"]

StartDownload ==
  /\ Workload \in {"Speech", "Management", "Mixed", "EngineSpeech",
                  "Subtitles", "GpuIdle"}
  /\ app.engine = "Local"
  /\ app.runtime = "Missing"
  /\ app.download = "Idle"
  /\ app' = [app EXCEPT
       !.download = "Running", !.downloadEpoch = app.configEpoch]

CancelDownload ==
  /\ app.download = "Running"
  /\ app' = [app EXCEPT !.download = "Cancelling"]

DownloadSuccess ==
  /\ app.download = "Running"
  /\ app' = [app EXCEPT
       !.download = "Done", !.localInstalled = TRUE, !.runtime = "Ready",
       !.gpuLoaded = Workload = "GpuIdle"]

DownloadExit ==
  /\ app.download \in {"Running", "Cancelling"}
  /\ app' = [app EXCEPT !.download = "Idle"]

MoveForeground(w) ==
  /\ Workload \in {"Speech", "CloudSpeech", "Selection", "Mixed",
                  "AssistantWeb"}
  /\ app.foregroundChanges = 0
  /\ w \in Windows
  /\ w # app.foreground
  /\ app' = [app EXCEPT !.foreground = w, !.foregroundChanges = 1]

StartRecording(mode) ==
  /\ AllowSpeech
  /\ mode \in Modes
  /\ (Workload \notin {"Mixed", "EngineSpeech", "Subtitles", "GpuIdle"}
      \/ mode = "Dictation")
  /\ (Workload # "AssistantWeb" \/ mode = "Assistant")
  /\ app.runtime = "Ready"
  /\ app.rec = "Idle"
  /\ app' = [app EXCEPT
       !.rec = "Starting", !.mode = mode, !.target = app.foreground,
       !.subtitleSession = IF Workload = "Subtitles" THEN 1 ELSE 0]

CaptureReady ==
  /\ app.rec = "Starting"
  /\ app' = [app EXCEPT !.rec = "Recording",
       !.gpuLoaded = IF Workload = "GpuIdle" THEN TRUE ELSE @,
       !.gpuSuspended = FALSE]

CancelStarting ==
  /\ app.rec = "Starting"
  /\ app' = [app EXCEPT !.rec = "Cancelled"]

StopRecording ==
  /\ app.rec = "Recording"
  /\ app' = [app EXCEPT
       !.rec = "Asr",
       !.historyAllowed = app.historyOn /\ app.target # "Unknown",
       !.audioAllowed = app.historyOn /\ app.audioOn
                        /\ app.target # "Unknown",
       !.audioWrite = IF app.historyOn /\ app.audioOn /\ app.target # "Unknown"
                     THEN "Pending" ELSE "None"]

AsrText ==
  /\ app.rec = "Asr"
  /\ app' = [app EXCEPT !.rec = "PostAsr", !.asr = "Text", !.gpuAge = 0]

AsrFailure ==
  /\ app.rec = "Asr"
  /\ app' = [app EXCEPT !.rec = "Failed", !.asr = "Error",
       !.result = "Error", !.gpuAge = 0]

NoSpeech ==
  /\ app.rec = "Asr"
  /\ app' = [app EXCEPT !.rec = "Failed", !.asr = "Empty",
       !.result = "Error", !.gpuAge = 0]

\* Auto polish can either skip or run after the Jev decision. Explicit
\* translation, edit, and assistant requests always enter their LLM path.
SkipPolish ==
  /\ app.rec = "PostAsr"
  /\ app.mode = "Dictation"
  /\ app.polishMode \in {"Off", "Auto"}
  /\ app' = [app EXCEPT !.rec = "Finished", !.result = "Raw"]

StartLlm ==
  /\ app.rec = "PostAsr"
  /\ (app.mode # "Dictation" \/ app.polishMode \in {"On", "Auto"})
  /\ app' = [app EXCEPT
       !.rec = IF app.mode = "Assistant" THEN "Preparing" ELSE "Llm",
       !.screenAllowed = app.screenOn /\ app.target # "Unknown"
                          /\ app.target = app.foreground,
       !.screenPrep = IF app.mode = "Assistant" THEN "Pending" ELSE "None",
       !.webAllowed = app.webOn /\ app.mode = "Assistant"]

UseScreenContext ==
  /\ app.rec \in {"Preparing", "Llm"}
  /\ app.screenAllowed
  /\ app.target = app.foreground
  /\ ~app.screenUsed
  /\ app' = [app EXCEPT !.screenUsed = TRUE,
       !.screenshotWindow = app.foreground,
       !.screenPrep = IF app.rec = "Preparing" THEN "Ready" ELSE @]

FinishScreenWithoutContext ==
  /\ app.rec = "Preparing" /\ app.screenPrep = "Pending"
  /\ app' = [app EXCEPT !.screenPrep = "Unavailable"]

\* External third-party search and fresh screen preparation join before
\* dispatch; ModelNative search runs inside the LLM request instead.
\* The post-join foreground read is a snapshot, not an OS focus lock.
DispatchAssistant ==
  /\ app.rec = "Preparing"
  /\ app.screenPrep \in {"Ready", "Unavailable"}
  /\ ~app.webAllowed \/ app.webSearch \in {"Sources", "Unavailable"}
  /\ app' = [app EXCEPT !.rec = "Llm",
       !.dispatchWindow = app.foreground,
       !.screenSent = app.screenUsed /\ app.target = app.foreground,
       !.screenUsed = app.screenUsed /\ app.target = app.foreground]

LlmSuccess ==
  /\ app.rec = "Llm"
  /\ app.llmKey
  /\ app.webSearch # "Running"
  /\ app' = [app EXCEPT !.rec = "Finished",
       !.result = IF app.mode = "Assistant" THEN "Assistant" ELSE "Processed"]

LlmFailure ==
  /\ app.rec = "Llm"
  /\ app.webSearch # "Running"
  /\ IF app.mode \in {"Assistant", "Edit"}
        THEN app' = [app EXCEPT !.rec = "Failed", !.result = "Error"]
        ELSE app' = [app EXCEPT !.rec = "Finished", !.result = "Raw"]

PublishFailure ==
  /\ app.rec = "Failed"
  /\ ~app.terminalPublished
  /\ app' = [app EXCEPT !.terminalPublished = TRUE]

BeginHistoryAudio ==
  /\ app.audioAllowed
  /\ OutputReadyForArchive
  /\ app.audioWrite = "Pending"
  /\ app' = [app EXCEPT !.audioWrite = "Running"]

SaveHistory ==
  /\ app.rec \in {"Finished", "Failed"}
  /\ app.historyAllowed
  /\ OutputReadyForArchive
  /\ app.audioWrite \in {"None", "Done"}
  /\ ~app.historyStored
  /\ app' = [app EXCEPT !.historyStored = TRUE, !.historyFinalized = TRUE]

HistoryWriteFailure ==
  /\ app.rec \in {"Finished", "Failed"}
  /\ app.historyAllowed /\ ~app.historyFinalized
  /\ OutputReadyForArchive
  /\ app.audioWrite \in {"None", "Done"}
  /\ app' = [app EXCEPT !.historyFinalized = TRUE]

SaveAudio ==
  /\ app.rec \in {"Finished", "Failed"}
  /\ app.audioAllowed
  /\ app.audioWrite = "Running"
  /\ ~app.audioStored
  /\ app' = [app EXCEPT !.audioStored = TRUE,
       !.audioFileExists = TRUE, !.audioWrite = "Done"]

AudioWriteFailure ==
  /\ app.audioWrite = "Running"
  /\ app' = [app EXCEPT !.audioWrite = "Done"]

\* The assistant prepares external search only when its effective profile allows it.
\* Both success and failure return to the normal answer path.
StartWebSearch ==
  /\ app.rec = "Preparing" /\ app.mode = "Assistant"
  /\ app.webAllowed /\ app.webSearch = "None"
  /\ app' = [app EXCEPT !.webSearch = "Running"]

FinishWebSearch(outcome) ==
  /\ app.webSearch = "Running"
  /\ outcome \in {"Sources", "Unavailable"}
  /\ app' = [app EXCEPT !.webSearch = outcome]

DisplayResult ==
  /\ app.rec = "Finished"
  /\ app.result \in {"Raw", "Processed", "Assistant"}
  /\ ~app.displayed
  /\ app' = [app EXCEPT !.displayed = TRUE, !.terminalPublished = TRUE]

QueuePaste ==
  /\ app.rec = "Finished"
  /\ app.mode # "Assistant"
  /\ app.result \in {"Raw", "Processed"}
  /\ app.terminalPublished
  /\ app.paste = "None"
  /\ app' = [app EXCEPT !.paste = "Queued"]

BeginPaste ==
  /\ app.paste = "Queued"
  /\ app.rec \notin {"Starting", "Recording"}
  /\ app' = [app EXCEPT !.paste = "InProgress",
       !.clipboardSnapshot = app.clipboard,
       !.clipboard = IF app.inputMethod = "Clipboard" THEN "App"
                     ELSE app.clipboard]

ExternalClipboardChange ==
  /\ app.clipboard = "App"
  /\ app' = [app EXCEPT !.clipboard = "External",
       !.pasteExternalChanged = app.paste = "InProgress"]

FinishPaste ==
  /\ app.paste = "InProgress"
  /\ app' = [app EXCEPT
       !.paste = "Done",
       !.clipboard = IF app.inputMethod = "Clipboard" /\ app.clipboard = "App"
                     THEN app.clipboardSnapshot ELSE app.clipboard,
       !.externalPreservedAtFinish = ~app.pasteExternalChanged
                                      \/ app.clipboard = "External"]

DetectSelection ==
  /\ AllowSelection
  /\ app.selVersion < IF Workload = "Mixed" THEN 1 ELSE 2
  /\ app.foreground # "Unknown"
  /\ app' = [app EXCEPT
       !.selVersion = @ + 1, !.selPhase = "Selected",
       !.selWindow = app.foreground, !.selAction = "None",
       !.selResultVersion = 0, !.selectionReplaced = FALSE,
       !.sourceMatches = TRUE, !.replacementSourceMatched = FALSE]

ExternalSelectionChange ==
  /\ AllowSelection /\ app.selPhase # "Idle"
  /\ app.sourceMatches
  /\ app' = [app EXCEPT !.sourceMatches = FALSE]

StartSelectionAction(action) ==
  /\ action \in SelectionActions
  /\ (Workload # "Mixed" \/ action = "Optimize")
  /\ app.selPhase = "Selected"
  /\ app' = [app EXCEPT !.selPhase = "Working", !.selAction = action,
       !.selRequestVersion = app.selVersion]

SelectionSuccess ==
  /\ app.selPhase = "Working"
  /\ app.selRequestVersion = app.selVersion
  /\ app.llmKey
  /\ app' = [app EXCEPT !.selPhase = "Result",
       !.selResultVersion = app.selVersion]

CancelSelection ==
  /\ app.selPhase = "Working"
  /\ app' = [app EXCEPT !.selPhase = "Selected"]

ReplaceSelection ==
  /\ app.selPhase = "Result"
  /\ app.selAction = "Optimize"
  /\ app.selResultVersion = app.selVersion
  /\ app.selWindow = app.foreground
  /\ app.sourceMatches
  /\ ~app.selectionReplaced
  /\ app' = [app EXCEPT !.selectionReplaced = TRUE,
       !.replacementWindow = app.foreground,
       !.replacementSourceMatched = app.sourceMatches]

CopySelection ==
  /\ app.selPhase \in {"Selected", "Result"}
  /\ app.clipboard = "Original"
  /\ app' = [app EXCEPT !.clipboard = "App"]

SearchSelection ==
  /\ app.selPhase \in {"Selected", "Result"}
  /\ ~app.selectionSearched
  /\ app' = [app EXCEPT !.selectionSearched = TRUE]

StartAssistantChat ==
  /\ app.mode = "Assistant"
  /\ app.rec = "Finished"
  /\ app.displayed
  /\ app.chat = "Idle"
  /\ app' = [app EXCEPT !.chat = "Working"]

AssistantChatResult ==
  /\ app.chat = "Working"
  /\ app.llmKey
  /\ app' = [app EXCEPT !.chat = "Result"]

CancelAssistantChat ==
  /\ app.chat = "Working"
  /\ app' = [app EXCEPT !.chat = "Cancelled"]

DeleteHistory ==
  /\ app.historyStored
  /\ app' = [app EXCEPT !.historyStored = FALSE,
       !.audioStored = FALSE,
       !.audioFileExists = app.audioLease]

QueryHistory ==
  /\ Workload = "History"
  /\ app.historyStored /\ ~app.historyQueried
  /\ app' = [app EXCEPT !.historyQueried = TRUE]

ExportHistory ==
  /\ Workload = "History"
  /\ app.historyQueried /\ ~app.historyExported
  /\ app' = [app EXCEPT !.historyExported = TRUE]

StartAudioReprocess ==
  /\ Workload = "History"
  /\ app.historyStored /\ app.audioStored /\ app.audioFileExists
  /\ app.mode = "Dictation" /\ app.reprocess = "Idle"
  /\ app' = [app EXCEPT !.reprocess = "Running", !.audioLease = TRUE]

FinishAudioReprocess(success) ==
  /\ app.reprocess = "Running"
  /\ success \in BOOLEAN
  /\ app' = [app EXCEPT !.reprocess = "Done",
       !.historyReprocessed = success,
       !.historyStored = IF success THEN TRUE ELSE @,
       !.audioStored = IF success THEN TRUE ELSE @]

ReleaseAudioLease ==
  /\ app.reprocess = "Done" /\ app.audioLease
  /\ app' = [app EXCEPT !.audioLease = FALSE,
       !.audioFileExists = app.audioStored]

\* The subtitle overlay ignores an event from an earlier recording and never
\* replaces a final result with a late interim result from the same session.
ShowSubtitle ==
  /\ Workload = "Subtitles" /\ app.rec = "Recording"
  /\ app.subtitle = "Idle"
  /\ app' = [app EXCEPT !.subtitle = "Visible"]

InterimSubtitle(session) ==
  /\ Workload = "Subtitles"
  /\ session \in 0..1
  /\ app.subtitle \in {"Visible", "Interim", "Final"}
  /\ app' = IF session = app.subtitleSession /\ app.subtitle # "Final"
             THEN [app EXCEPT !.subtitle = "Interim"]
             ELSE [app EXCEPT !.staleSubtitleIgnored = TRUE]

FinalSubtitle ==
  /\ Workload = "Subtitles" /\ app.rec = "Finished"
  /\ app.subtitle \in {"Visible", "Interim"}
  /\ app' = [app EXCEPT !.subtitle = "Final", !.subtitleFinalSeen = TRUE]

ReprocessHistory ==
  /\ Workload \in {"Speech", "CloudSpeech"}
  /\ app.historyStored
  /\ app.mode = "Dictation"
  /\ app.result \in {"Raw", "Processed"}
  /\ ~app.historyReprocessed
  /\ app' = [app EXCEPT !.historyReprocessed = TRUE]

CheckUpdate(status) ==
  /\ AllowManagement
  /\ app.updateCheck = "Unchecked"
  /\ status \in {"Current", "Available", "Error"}
  /\ app' = [app EXCEPT !.updateCheck = status]

OpenRelease ==
  /\ app.updateCheck = "Available"
  /\ app' = [app EXCEPT !.updateCheck = "Opened"]

EnableGpuIdle ==
  /\ Workload = "GpuIdle" /\ ~app.gpuEnabled
  /\ app' = [app EXCEPT !.gpuEnabled = TRUE, !.gpuAge = 0]
DisableGpuIdle ==
  /\ Workload = "GpuIdle" /\ app.gpuEnabled
  /\ app' = [app EXCEPT !.gpuEnabled = FALSE, !.gpuAge = 0]
TickGpuIdle ==
  /\ Workload = "GpuIdle" /\ app.gpuEnabled /\ app.gpuLoaded /\ app.gpuAge < 2
  /\ app.rec \notin {"Starting", "Recording", "Asr"}
  /\ app' = [app EXCEPT !.gpuAge = @ + 1]
UnloadGpuIdle ==
  /\ Workload = "GpuIdle" /\ app.gpuEnabled /\ app.gpuLoaded /\ app.gpuAge = 2
  /\ app.rec \notin {"Starting", "Recording", "Asr"}
  /\ app' = [app EXCEPT !.gpuLoaded = FALSE, !.gpuSuspended = TRUE,
       !.gpuUnsafeUnload = app.rec \in {"Starting", "Recording", "Asr"}]

Next ==
  \/ \E policy \in Policies: Configure(policy)
  \/ SaveSettings \/ SetLlmCredential \/ RequestEngineSwitch
  \/ SwitchEngine \/ RejectEngineSwitch
  \/ SetAsrCredential \/ StartDownload \/ CancelDownload
  \/ DownloadSuccess \/ DownloadExit
  \/ \E w \in Windows: MoveForeground(w)
  \/ \E mode \in Modes: StartRecording(mode)
  \/ CaptureReady \/ CancelStarting \/ StopRecording
  \/ AsrText \/ AsrFailure \/ NoSpeech \/ SkipPolish
  \/ StartLlm \/ UseScreenContext \/ FinishScreenWithoutContext
  \/ DispatchAssistant \/ LlmSuccess \/ LlmFailure
  \/ StartWebSearch
  \/ \E outcome \in {"Sources", "Unavailable"}: FinishWebSearch(outcome)
  \/ PublishFailure \/ BeginHistoryAudio
  \/ SaveHistory \/ HistoryWriteFailure \/ SaveAudio \/ AudioWriteFailure
  \/ DisplayResult \/ QueuePaste
  \/ BeginPaste \/ ExternalClipboardChange \/ FinishPaste
  \/ DetectSelection \/ ExternalSelectionChange
  \/ \E action \in SelectionActions: StartSelectionAction(action)
  \/ SelectionSuccess \/ CancelSelection \/ ReplaceSelection
  \/ CopySelection \/ SearchSelection
  \/ StartAssistantChat \/ AssistantChatResult \/ CancelAssistantChat
  \/ DeleteHistory \/ ReprocessHistory \/ QueryHistory \/ ExportHistory
  \/ StartAudioReprocess
  \/ \E success \in BOOLEAN: FinishAudioReprocess(success)
  \/ ReleaseAudioLease
  \/ ShowSubtitle
  \/ \E session \in 0..1: InterimSubtitle(session)
  \/ FinalSubtitle
  \/ \E status \in {"Current", "Available", "Error"}: CheckUpdate(status)
  \/ OpenRelease
  \/ EnableGpuIdle \/ DisableGpuIdle \/ TickGpuIdle \/ UnloadGpuIdle

Spec == Init /\ [][Next]_app

TypeOK ==
  /\ app.engine \in {"Local", "Cloud"}
  /\ app.runtime \in {"Missing", "NeedKey", "Ready"}
  /\ app.download \in {"Idle", "Running", "Cancelling", "Done"}
  /\ app.engineRequest \in {"None", "Pending", "Blocked", "Done"}
  /\ app.rec \in {"Idle", "Starting", "Recording", "Asr", "PostAsr",
                  "Preparing", "Llm", "Finished", "Failed", "Cancelled"}
  /\ app.mode \in Modes \cup {"None"}
  /\ app.target \in Windows
  /\ app.foreground \in Windows
  /\ app.foregroundChanges \in 0..1
  /\ app.screenshotWindow \in Windows
  /\ app.dispatchWindow \in Windows
  /\ app.screenPrep \in {"None", "Pending", "Ready", "Unavailable"}
  /\ app.audioWrite \in {"None", "Pending", "Running", "Done"}
  /\ app.terminalPublished \in BOOLEAN
  /\ app.asr \in {"None", "Text", "Empty", "Error"}
  /\ app.result \in {"None", "Raw", "Processed", "Assistant", "Error"}
  /\ app.webSearch \in {"None", "Running", "Sources", "Unavailable"}
  /\ app.polishMode \in PolishModes
  /\ app.inputMethod \in {"SendInput", "Clipboard"}
  /\ app.paste \in {"None", "Queued", "InProgress", "Done"}
  /\ app.clipboard \in {"Original", "App", "External"}
  /\ app.clipboardSnapshot \in {"Original", "App", "External"}
  /\ app.selVersion \in 0..2
  /\ app.selPhase \in {"Idle", "Selected", "Working", "Result"}
  /\ app.selAction \in SelectionActions \cup {"None"}
  /\ app.selWindow \in Windows
  /\ app.replacementWindow \in Windows
  /\ app.selRequestVersion \in 0..2
  /\ app.selResultVersion \in 0..2
  /\ app.chat \in {"Idle", "Working", "Result", "Cancelled"}
  /\ app.reprocess \in {"Idle", "Running", "Done"}
  /\ app.subtitle \in {"Idle", "Visible", "Interim", "Final"}
  /\ app.subtitleSession \in 0..1
  /\ app.settingsVersion \in 0..1
  /\ app.savedVersion \in 0..1
  /\ app.updateCheck \in {"Unchecked", "Current", "Available", "Error", "Opened"}
  /\ app.gpuAge \in 0..2

HistoryConsent == ~app.historyStored \/ app.historyAllowed
AudioConsent == ~app.audioStored \/ app.audioAllowed
ScreenConsent == ~app.screenUsed \/ app.screenAllowed
ScreenStaysOnCapturedWindow ==
  ~app.screenUsed \/ app.screenshotWindow = app.target
FreshScreenIsCurrentAtDispatch ==
  ~app.screenSent \/ app.screenshotWindow = app.dispatchWindow
AssistantPreparationSettlesBeforeDispatch ==
  app.mode # "Assistant" \/ app.rec # "Llm"
    \/ (app.screenPrep \in {"Ready", "Unavailable"}
        /\ (~app.webAllowed \/ app.webSearch \in {"Sources", "Unavailable"}))
HistorySettlesBeforePublication ==
  \* Deliberately false reachability witness: terminal output may precede archive work.
  ~(app.displayed \/ app.paste # "None" \/ app.subtitleFinalSeen)
    \/ ~app.historyAllowed \/ (app.historyFinalized /\ app.audioWrite # "Running")
ArchiveFollowsTerminalOutput ==
  ~(app.historyFinalized \/ app.audioWrite \in {"Running", "Done"})
    \/ OutputReadyForArchive
HistoryRequiresCompletedPaste ==
  \* Deliberately false reachability witness: queued paste cannot hold up text storage.
  ~app.historyFinalized \/ app.paste # "Queued"
DownloadPinsConfig ==
  app.download \notin {"Running", "Cancelling"}
    \/ app.downloadEpoch = app.configEpoch
AssistantNeverPastes == app.mode # "Assistant" \/ app.paste = "None"
PasteHasText == app.paste = "None" \/ app.result \in {"Raw", "Processed"}
SelectionResultIsCurrent ==
  app.selPhase # "Result" \/ app.selResultVersion = app.selVersion
SelectionReplaceIsAuthorized ==
  ~app.selectionReplaced
    \/ (app.selAction = "Optimize" /\ app.selResultVersion = app.selVersion
        /\ app.replacementWindow = app.selWindow
        /\ app.replacementSourceMatched)
ExternalClipboardIsPreserved == app.externalPreservedAtFinish
SavedSettingsNotFuture == app.savedVersion <= app.settingsVersion
ReprocessOnlyDictation == ~app.historyReprocessed \/ app.mode = "Dictation"
WebSearchConsent == app.webSearch = "None" \/ app.webAllowed
AudioLeaseKeepsFile == ~app.audioLease \/ app.audioFileExists
StoredAudioKeepsFile == ~app.audioStored \/ app.audioFileExists
ReprocessNeedsLease == app.reprocess # "Running" \/ app.audioLease
SubtitleMatchesSession ==
  app.subtitle = "Idle" \/ app.subtitleSession = 1
SubtitleFinalStaysFinal ==
  ~app.subtitleFinalSeen \/ app.subtitle = "Final"
RejectedSwitchKeepsConfig ==
  app.engineRequest # "Blocked" \/ app.configEpoch = app.requestEpoch
SuccessfulSwitchHasRequest ==
  app.engineSwitches = 0 \/ app.engineRequest = "Done"

GpuUnloadIsSafe == ~app.gpuUnsafeUnload
GpuSuspensionKeepsReady == ~app.gpuSuspended \/ app.runtime = "Ready"
GpuStreamHasRuntime == Workload # "GpuIdle" \/ app.rec # "Recording" \/ app.gpuLoaded

Safety ==
  /\ HistoryConsent /\ AudioConsent /\ ScreenConsent
  /\ ScreenStaysOnCapturedWindow /\ DownloadPinsConfig
  /\ FreshScreenIsCurrentAtDispatch /\ AssistantPreparationSettlesBeforeDispatch
  /\ ArchiveFollowsTerminalOutput
  /\ AssistantNeverPastes /\ PasteHasText
  /\ SelectionResultIsCurrent /\ SelectionReplaceIsAuthorized
  /\ ExternalClipboardIsPreserved /\ SavedSettingsNotFuture
  /\ ReprocessOnlyDictation /\ WebSearchConsent
  /\ AudioLeaseKeepsFile /\ StoredAudioKeepsFile /\ ReprocessNeedsLease
  /\ SubtitleMatchesSession /\ SubtitleFinalStaysFinal
  /\ RejectedSwitchKeepsConfig /\ SuccessfulSwitchHasRequest
  /\ GpuUnloadIsSafe /\ GpuSuspensionKeepsReady /\ GpuStreamHasRuntime
=============================================================================
