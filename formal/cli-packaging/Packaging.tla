---------------------------- MODULE Packaging ----------------------------
EXTENDS Naturals, TLC

CONSTANTS SkipVerify, SkipInputGuard, OverwriteEarly, DeleteUnrelated,
          SkipCleanup, AllowEmptyRelease, OverwriteDebug, SkipMetadataGuard, Fair, Witness

VARIABLES phase, inputs, snapshot, staged, stagedInputs, verified,
          published, publishedVerified, publishedOriginal, keepUnrelated,
          mode, initialArtifact, artifact, selectorDone, selectorSuccess, selectorError

vars == <<phase, inputs, snapshot, staged, stagedInputs, verified,
          published, publishedVerified, publishedOriginal, keepUnrelated,
          mode, initialArtifact, artifact, selectorDone, selectorSuccess, selectorError>>

\* Fifth selector input is only embedded-NUL path conversion, not general I/O failure.
Init ==
    /\ phase = "idle"
    /\ inputs = 0 /\ snapshot = 0
    /\ staged = "none" /\ stagedInputs = 0 /\ verified = FALSE
    /\ published = "old" /\ publishedVerified = FALSE /\ publishedOriginal = FALSE
    /\ keepUnrelated = TRUE
    /\ mode \in {"debug", "release"}
    /\ initialArtifact \in {"missing", "empty", "full", "directory", "metadata-nul-invalid-input"}
    /\ artifact = initialArtifact
    /\ selectorDone = FALSE /\ selectorSuccess = FALSE
    /\ selectorError = [stage |-> "none", cause |-> "none"]

Start ==
    /\ phase = "idle"
    /\ phase' = "building" /\ snapshot' = inputs /\ staged' = "partial"
    /\ published' = IF OverwriteEarly THEN "new" ELSE published
    /\ UNCHANGED <<inputs, stagedInputs, verified, publishedVerified,
                    publishedOriginal, keepUnrelated, mode, initialArtifact,
                    artifact, selectorDone, selectorSuccess, selectorError>>

ChangeInputs ==
    /\ phase \in {"building", "built", "verified"} /\ inputs = 0
    /\ inputs' = 1
    /\ UNCHANGED <<phase, snapshot, staged, stagedInputs, verified, published,
                    publishedVerified, publishedOriginal, keepUnrelated,
                    mode, initialArtifact, artifact, selectorDone, selectorSuccess, selectorError>>

Finish ==
    /\ phase = "building"
    /\ phase' = "built" /\ staged' \in {"good", "bad"} /\ stagedInputs' = inputs
    /\ UNCHANGED <<inputs, snapshot, verified, published, publishedVerified,
                    publishedOriginal, keepUnrelated, mode, initialArtifact,
                    artifact, selectorDone, selectorSuccess, selectorError>>

BuildFailure ==
    /\ phase = "building" /\ phase' = "failed"
    /\ UNCHANGED <<inputs, snapshot, staged, stagedInputs, verified, published,
                    publishedVerified, publishedOriginal, keepUnrelated, mode,
                    initialArtifact, artifact, selectorDone, selectorSuccess, selectorError>>

Verify ==
    /\ phase = "built"
    /\ verified' = (staged = "good" /\ stagedInputs = inputs)
    /\ phase' = IF (staged = "good" /\ stagedInputs = inputs) \/ SkipVerify
                 THEN "verified" ELSE "failed"
    /\ UNCHANGED <<inputs, snapshot, staged, stagedInputs, published,
                    publishedVerified, publishedOriginal, keepUnrelated, mode,
                    initialArtifact, artifact, selectorDone, selectorSuccess, selectorError>>

Publish ==
    /\ phase = "verified"
    /\ (inputs = snapshot) \/ SkipInputGuard
    /\ published' = "new"
    /\ publishedVerified' = verified /\ (stagedInputs = inputs)
    /\ publishedOriginal' = (inputs = snapshot)
    /\ staged' = "none" /\ phase' = "committed"
    /\ UNCHANGED <<inputs, snapshot, stagedInputs, verified, keepUnrelated, mode,
                    initialArtifact, artifact, selectorDone, selectorSuccess, selectorError>>

InputFailure ==
    /\ phase = "verified" /\ inputs # snapshot /\ ~SkipInputGuard
    /\ phase' = "failed"
    /\ UNCHANGED <<inputs, snapshot, staged, stagedInputs, verified, published,
                    publishedVerified, publishedOriginal, keepUnrelated, mode,
                    initialArtifact, artifact, selectorDone, selectorSuccess, selectorError>>

ReplaceFailure ==
    /\ phase = "verified" /\ phase' = "failed"
    /\ UNCHANGED <<inputs, snapshot, staged, stagedInputs, verified, published,
                    publishedVerified, publishedOriginal, keepUnrelated, mode,
                    initialArtifact, artifact, selectorDone, selectorSuccess, selectorError>>

Cleanup ==
    /\ phase \in {"failed", "committed"}
    /\ phase' = "done"
    /\ staged' = IF SkipCleanup THEN staged ELSE "none"
    /\ keepUnrelated' = IF DeleteUnrelated THEN FALSE ELSE keepUnrelated
    /\ UNCHANGED <<inputs, snapshot, stagedInputs, verified, published,
                    publishedVerified, publishedOriginal, mode, initialArtifact,
                    artifact, selectorDone, selectorSuccess, selectorError>>

\* NUL conversion fails before WinAPI in metadata and, after guard bypass, debug create.
Select ==
    /\ ~selectorDone
    /\ selectorDone' = TRUE
    /\ selectorSuccess' =
        IF artifact = "metadata-nul-invalid-input" THEN FALSE
        ELSE IF mode = "debug" THEN artifact # "directory"
             ELSE artifact = "full" \/ (AllowEmptyRelease /\ artifact = "empty")
    /\ artifact' =
        IF mode = "debug" /\ (artifact = "missing" \/
                                   (OverwriteDebug /\ artifact = "full"))
        THEN "empty" ELSE artifact
    /\ selectorError' =
        IF artifact = "metadata-nul-invalid-input" THEN
            IF ~SkipMetadataGuard THEN [stage |-> "metadata", cause |-> "invalid-input"]
            ELSE IF mode = "debug" THEN [stage |-> "create", cause |-> "invalid-input"]
                 ELSE [stage |-> "selection", cause |-> "missing-or-empty"]
        ELSE IF artifact = "directory" THEN [stage |-> "metadata", cause |-> "not-file"]
        ELSE IF selectorSuccess' THEN [stage |-> "none", cause |-> "none"]
        ELSE [stage |-> "selection", cause |-> "missing-or-empty"]
    /\ UNCHANGED <<phase, inputs, snapshot, staged, stagedInputs, verified,
                    published, publishedVerified, publishedOriginal, keepUnrelated,
                    mode, initialArtifact>>

Step == Start \/ Finish \/ BuildFailure \/ Verify \/ Publish \/ InputFailure \/
        ReplaceFailure \/ Cleanup \/ Select
Terminal == /\ phase = "done" /\ selectorDone /\ UNCHANGED vars
Next == Step \/ ChangeInputs \/ Terminal
Spec == Init /\ [][Next]_vars /\ (IF Fair THEN WF_vars(Step) ELSE TRUE)

TypeOK ==
    /\ phase \in {"idle", "building", "built", "verified", "failed", "committed", "done"}
    /\ inputs \in {0, 1} /\ snapshot \in {0, 1} /\ stagedInputs \in {0, 1}
    /\ staged \in {"none", "partial", "good", "bad"}
    /\ published \in {"old", "new"}
    /\ <<verified, publishedVerified, publishedOriginal, keepUnrelated,
          selectorDone, selectorSuccess>> \in BOOLEAN \X BOOLEAN \X BOOLEAN \X BOOLEAN \X BOOLEAN \X BOOLEAN
    /\ mode \in {"debug", "release"}
    /\ initialArtifact \in {"missing", "empty", "full", "directory", "metadata-nul-invalid-input"}
    /\ artifact \in {"missing", "empty", "full", "directory", "metadata-nul-invalid-input"}
    /\ selectorError \in [stage: {"none", "metadata", "create", "selection"},
                            cause: {"none", "invalid-input", "not-file", "missing-or-empty"}]

OnlyVerifiedPublication == published = "new" => publishedVerified
OriginalInputsRequired == published = "new" => publishedOriginal
UnrelatedSurvives == keepUnrelated
SettledOwnStageReleased == phase = "done" => staged = "none"
ReleaseRequiresNonempty == selectorDone /\ mode = "release" /\ selectorSuccess => artifact = "full"
ExistingArtifactPreserved == selectorDone /\ initialArtifact # "missing" => artifact = initialArtifact
SelectionTruth == selectorDone =>
    (selectorSuccess = (IF mode = "debug" THEN initialArtifact \in {"missing", "empty", "full"}
                        ELSE initialArtifact = "full"))
MetadataFailurePreserved == selectorDone /\ initialArtifact = "metadata-nul-invalid-input" =>
    /\ ~selectorSuccess
    /\ selectorError = [stage |-> "metadata", cause |-> "invalid-input"]
    /\ artifact = initialArtifact
    /\ keepUnrelated
EventuallySettled == <> (phase = "done" /\ selectorDone)

WitnessNotReached ==
    CASE Witness = "commit" -> ~(phase = "done" /\ published = "new")
      [] Witness = "rollback" -> ~(phase = "done" /\ published = "old" /\ staged = "none")
      [] Witness = "stale" -> ~(phase = "done" /\ inputs # snapshot /\ published = "old")
      [] Witness = "debug" -> ~(selectorDone /\ mode = "debug" /\
                                 initialArtifact = "missing" /\ artifact = "empty" /\ selectorSuccess)
      [] Witness = "release" -> ~(selectorDone /\ mode = "release" /\
                                   initialArtifact = "empty" /\ ~selectorSuccess)
      [] Witness = "metadata" -> ~(
            /\ selectorDone
            /\ initialArtifact = "metadata-nul-invalid-input"
            /\ ~selectorSuccess
            /\ selectorError = [stage |-> "metadata", cause |-> "invalid-input"]
            /\ artifact = initialArtifact
            /\ keepUnrelated)
      [] OTHER -> TRUE
=============================================================================
