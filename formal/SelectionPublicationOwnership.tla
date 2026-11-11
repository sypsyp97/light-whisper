------------------- MODULE SelectionPublicationOwnership -------------------
EXTENDS Naturals, FiniteSets
CONSTANT Wrong
VARIABLE s
vars == <<s>>
Tasks == 1..2
Phases == {"idle", "capturing", "ready", "screenshots", "window", "finishing", "done"}

Init == s = [enabled |-> TRUE, epoch |-> 0, disablePhase |-> "idle",
  phase |-> [task \in Tasks |-> "idle"],
  capturedEpoch |-> [task \in Tasks |-> 0],
  captureHasScreenshots |-> [task \in Tasks |-> FALSE],
  publicationLock |-> 0, screenshotOwner |-> 0, visible |-> FALSE,
  badPublication |-> FALSE, staleDropped |-> FALSE,
  publishedCurrent |-> FALSE, publishedAfterReenable |-> FALSE,
  retiredPending |-> FALSE]

\* Existing worker checks config.enabled before SELECTION_SETTLE_MS, clipboard
\* grab and screenshot join. Capture completion may occur after disable.
BeginWorker(task, screenshots) ==
  /\ s.enabled /\ s.phase[task] = "idle"
  /\ s' = [s EXCEPT !.phase[task] = "capturing",
       !.capturedEpoch[task] = s.epoch, !.captureHasScreenshots[task] = screenshots]
CaptureCompletes(task) ==
  /\ s.phase[task] = "capturing"
  /\ s' = [s EXCEPT !.phase[task] = "ready"]

Intended(task) == s.enabled /\ s.capturedEpoch[task] = s.epoch
Eligible(task) == CASE Wrong = "legacy" -> TRUE
  [] Wrong = "reenable" -> s.enabled
  [] OTHER -> Intended(task)
\* Correct publication locks from eligibility through BOTH effects. The
\* early-unlock mutant keeps the check but releases before screenshot/show.
CheckPublication(task) ==
  /\ s.phase[task] = "ready" /\ s.publicationLock = 0
  /\ s' = [s EXCEPT !.phase[task] = IF Eligible(task) THEN "screenshots" ELSE "done",
       !.publicationLock = IF Eligible(task) /\ Wrong \notin {"legacy", "early-unlock"}
         THEN task ELSE 0,
       !.staleDropped = @ \/ (~Eligible(task) /\ ~Intended(task))]
PublishScreenshots(task) ==
  /\ s.phase[task] = "screenshots"
  /\ s' = [s EXCEPT !.phase[task] = "window",
       !.screenshotOwner = IF s.captureHasScreenshots[task] THEN task ELSE 0,
       !.badPublication = @ \/ ~Intended(task)]
ShowWindow(task) ==
  /\ s.phase[task] = "window"
  /\ s' = [s EXCEPT !.phase[task] = "finishing", !.visible = TRUE,
       !.badPublication = @ \/ ~Intended(task),
       !.publishedCurrent = @ \/ Intended(task),
       !.publishedAfterReenable = @ \/ (Intended(task) /\ s.epoch = 1)]
FinishPublication(task) ==
  /\ s.phase[task] = "finishing"
  /\ s' = [s EXCEPT !.phase[task] = "done", !.publicationLock = 0]

\* Real command ordering: retire listener, cancel/hide, then return. A show
\* owning the publication lock may finish before retirement obtains that lock.
Retire ==
  /\ s.enabled /\ s.disablePhase = "idle" /\ s.publicationLock = 0
  /\ s' = [s EXCEPT !.enabled = FALSE, !.epoch = 1, !.disablePhase = "retired",
       !.retiredPending = \E task \in Tasks: s.phase[task] \notin {"idle", "done"}]
CompleteDisable ==
  /\ s.disablePhase = "retired"
  /\ s' = [s EXCEPT !.disablePhase = "complete", !.screenshotOwner = 0, !.visible = FALSE]
Reenable ==
  /\ ~s.enabled /\ s.disablePhase = "complete"
  /\ s' = [s EXCEPT !.enabled = TRUE]

Next == Retire \/ CompleteDisable \/ Reenable
  \/ (\E task \in Tasks, screenshots \in BOOLEAN: BeginWorker(task, screenshots))
  \/ (\E task \in Tasks: CaptureCompletes(task) \/ CheckPublication(task)
       \/ PublishScreenshots(task) \/ ShowWindow(task) \/ FinishPublication(task))
Spec == Init /\ [][Next]_vars
TypeOK ==
  /\ s.enabled \in BOOLEAN /\ s.epoch \in 0..1
  /\ s.disablePhase \in {"idle", "retired", "complete"}
  /\ s.phase \in [Tasks -> Phases]
  /\ s.capturedEpoch \in [Tasks -> 0..1]
  /\ s.captureHasScreenshots \in [Tasks -> BOOLEAN]
  /\ s.publicationLock \in 0..2 /\ s.screenshotOwner \in 0..2
  /\ s.visible \in BOOLEAN /\ s.badPublication \in BOOLEAN
PublicationOwned == ~s.badPublication
DisableCompletionClearsEffects == ~s.enabled /\ s.disablePhase = "complete" =>
  s.screenshotOwner = 0 /\ ~s.visible
BoundaryCoverageNotReached == ~(s.retiredPending /\ s.staleDropped /\
  s.publishedCurrent /\ s.publishedAfterReenable)
\* Two workers, one disable/reenable cycle. Capture success/screenshots are
\* environmental inputs; scheduler termination, native windows, clipboard,
\* screenshot encoding and rendering are outside this bounded safety model.
\* Legacy maps the baseline's unconditional show_selection after capture.
\* Early-unlock maps a check-then-show repair; reenable omits epoch comparison.
\* Runtime tests cover the real owner callback; source binding is a separate
\* review obligation. This model is not a Windows runtime regression proof.
=============================================================================
