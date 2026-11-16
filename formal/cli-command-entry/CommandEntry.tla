---------------------------- MODULE CommandEntry ----------------------------
EXTENDS Naturals, Sequences
CONSTANT Fault
VARIABLES phase, remaining, operation, outcome, reparse, alias, ownCount,
          foreign, installed, removed, touchedForeign, allowUninstall, savedCommand
vars == <<phase, remaining, operation, outcome, reparse, alias, ownCount,
          foreign, installed, removed, touchedForeign, allowUninstall, savedCommand>>
ForeignTokens == <<"foreign", "", "%EXPAND%">>
Active == {"admit", "validate", "copy", "copied", "unpublish", "cleanup"}

Init == /\ phase = "idle" /\ remaining = 2
        /\ operation = "install" /\ outcome = "ok" /\ reparse \in BOOLEAN
        /\ alias = FALSE /\ ownCount = 0 /\ foreign = ForeignTokens
        /\ installed = FALSE /\ removed = FALSE /\ touchedForeign = FALSE
        /\ allowUninstall = TRUE /\ savedCommand = <<FALSE, 0>>

Begin == /\ phase = "idle" /\ remaining > 0
         /\ operation' \in {"install", "uninstall"}
         /\ outcome' \in {"ok", "missing", "copy", "write"}
         /\ allowUninstall' \in BOOLEAN /\ savedCommand' = <<alias, ownCount>>
         /\ remaining' = remaining - 1
         /\ phase' = IF operation' = "uninstall" /\ Fault # "late-admission"
                        THEN "admit" ELSE "validate"
         /\ UNCHANGED <<reparse, alias, ownCount, foreign, installed, removed, touchedForeign>>

Admit == /\ phase = "admit"
         /\ phase' = IF ~allowUninstall THEN "denied"
                        ELSE IF Fault = "late-admission" THEN "done" ELSE "validate"
         /\ UNCHANGED <<remaining, operation, outcome, reparse, alias, ownCount, foreign,
                        installed, removed, touchedForeign, allowUninstall, savedCommand>>

Validate == /\ phase = "validate"
            /\ IF reparse /\ Fault # "reparse"
                  THEN phase' = "error" /\ UNCHANGED ownCount
                  ELSE IF operation = "install"
                         THEN IF outcome = "missing"
                                THEN phase' = "error" /\ UNCHANGED ownCount
                                ELSE /\ phase' = "copy"
                                     /\ ownCount' = IF Fault = "early" THEN 1 ELSE ownCount
                         ELSE phase' = "unpublish" /\ UNCHANGED ownCount
            /\ UNCHANGED <<remaining, operation, outcome, reparse, alias, foreign,
                           installed, removed, touchedForeign, allowUninstall, savedCommand>>

Copy == /\ phase = "copy"
        /\ IF outcome = "copy"
              THEN /\ phase' = "error" /\ UNCHANGED <<alias, touchedForeign>>
              ELSE /\ phase' = "copied" /\ alias' = TRUE
                   /\ touchedForeign' = (touchedForeign \/ reparse)
        /\ UNCHANGED <<remaining, operation, outcome, reparse, ownCount, foreign, installed,
                       removed, allowUninstall, savedCommand>>

Publish == /\ phase = "copied" /\ Fault # "stall"
           /\ IF outcome = "write"
                 THEN /\ phase' = "error" /\ UNCHANGED <<ownCount, foreign, installed>>
                 ELSE /\ phase' = "done" /\ installed' = TRUE
                      /\ ownCount' = IF Fault = "duplicate" THEN ownCount + 1 ELSE 1
                      /\ foreign' = IF Fault = "foreign" THEN Tail(foreign) ELSE foreign
           /\ UNCHANGED <<remaining, operation, outcome, reparse, alias, removed, touchedForeign,
                          allowUninstall, savedCommand>>

Unpublish == /\ phase = "unpublish"
             /\ IF outcome = "write"
                   THEN /\ phase' = "error" /\ UNCHANGED <<ownCount, foreign>>
                   ELSE /\ phase' = "cleanup" /\ ownCount' = 0
                        /\ foreign' = IF Fault = "foreign" THEN Tail(foreign) ELSE foreign
             /\ UNCHANGED <<remaining, operation, outcome, reparse, alias, installed,
                            removed, touchedForeign, allowUninstall, savedCommand>>

Cleanup == /\ phase = "cleanup"
           /\ phase' = IF Fault = "late-admission" THEN "admit" ELSE "done"
           /\ removed' = TRUE
           /\ alias' = IF Fault = "cleanup" THEN alias ELSE FALSE
           /\ touchedForeign' = (touchedForeign \/ reparse)
           /\ UNCHANGED <<remaining, operation, outcome, reparse, ownCount, foreign, installed,
                          allowUninstall, savedCommand>>

Again == /\ phase \in {"done", "error", "denied"} /\ remaining > 0 /\ phase' = "idle"
         /\ UNCHANGED <<remaining, operation, outcome, reparse, alias, ownCount, foreign,
                        installed, removed, touchedForeign, allowUninstall, savedCommand>>
Terminal == /\ phase \in {"done", "error", "denied"} /\ remaining = 0 /\ UNCHANGED vars
Stall == /\ Fault = "stall" /\ phase = "copied" /\ UNCHANGED vars
Next == Begin \/ Admit \/ Validate \/ Copy \/ Publish \/ Unpublish \/ Cleanup \/ Again \/ Terminal \/ Stall
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK == /\ phase \in {"idle", "done", "error", "denied"} \cup Active /\ remaining \in 0..2
          /\ operation \in {"install", "uninstall"} /\ outcome \in {"ok", "missing", "copy", "write"}
          /\ reparse \in BOOLEAN /\ alias \in BOOLEAN /\ ownCount \in 0..2
          /\ foreign \in Seq({"foreign", "", "%EXPAND%"})
          /\ installed \in BOOLEAN /\ removed \in BOOLEAN /\ touchedForeign \in BOOLEAN
          /\ allowUninstall \in BOOLEAN /\ savedCommand \in (BOOLEAN \X (0..2))
PublishedAfterCopy == ownCount > 0 => alias
PathIntegrity == foreign = ForeignTokens /\ ownCount <= 1
CleanupComplete == phase = "done" /\ operation = "uninstall" => ~alias /\ ownCount = 0
ForeignResourceUntouched == ~touchedForeign
DeniedPreservesCommand == phase = "denied" => <<alias, ownCount>> = savedCommand
OperationSettles == phase \in Active ~> phase \in {"done", "error", "denied"}
LifecycleNotReached == ~(installed /\ removed /\ phase = "done" /\ remaining = 0
                         /\ ownCount = 0 /\ ~alias)
DeniedNotReached == ~(phase = "denied" /\ savedCommand = <<TRUE, 1>> /\ alias /\ ownCount = 1)
=============================================================================
