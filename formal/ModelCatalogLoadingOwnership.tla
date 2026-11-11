--------------------- MODULE ModelCatalogLoadingOwnership --------------------
EXTENDS Naturals, Sequences
CONSTANT Wrong
VARIABLE s, lastEvent
vars == <<s, lastEvent>>
Ids == {1, 2}
Reasons == {"context", "no-auth", "separate-off"}
Owns(i) == s.owner = i /\ s.requestEpoch[i] = s.epoch
Init ==
  /\ s = [epoch |-> 1, owner |-> 0, loading |-> FALSE, enabled |-> TRUE,
    retired |-> FALSE, started |-> 0, displayEpoch |-> 0, errorEpoch |-> 0,
    requestState |-> [i \in Ids |-> "unused"], requestEpoch |-> [i \in Ids |-> 0]]
  /\ lastEvent = "init"
Begin(i) ==
  /\ i = s.started + 1 /\ i \in Ids /\ s.enabled /\ s.owner = 0
  /\ (i = 2 => s.retired)
  /\ s' = [s EXCEPT !.started = i, !.owner = i, !.loading = TRUE,
    !.requestState[i] = "pending", !.requestEpoch[i] = s.epoch, !.errorEpoch = 0]
  /\ lastEvent' = IF i = 1 THEN "catalog:1:begin" ELSE "catalog:2:begin"
Retire(reason) ==
  /\ s.started = 1 /\ s.requestState[1] = "pending" /\ ~s.retired
  /\ s' = [s EXCEPT !.epoch = @ + 1, !.owner = 0, !.retired = TRUE,
    !.enabled = reason = "context", !.displayEpoch = 0, !.errorEpoch = 0,
    !.loading = IF Wrong = "legacy-retirement" THEN @ ELSE FALSE]
  /\ lastEvent' = "catalog:retire:" \o reason
RestoreAuth ==
  /\ s.retired /\ ~s.enabled
  /\ s' = [s EXCEPT !.enabled = TRUE]
  /\ lastEvent' = "catalog:restore-eligibility"
Complete(i, success) ==
  /\ s.requestState[i] = "pending"
  /\ LET publish == Owns(i) \/ Wrong = "publication"
         finish == Owns(i) \/ Wrong = "finally"
     IN s' = [s EXCEPT !.requestState[i] = IF success THEN "success" ELSE "failure",
       !.displayEpoch = IF publish /\ success THEN s.requestEpoch[i] ELSE @,
       !.errorEpoch = IF publish /\ ~success THEN s.requestEpoch[i] ELSE @,
       !.loading = IF finish THEN FALSE ELSE @,
       !.owner = IF Owns(i) THEN 0 ELSE @]
  /\ lastEvent' = (IF i = 1 THEN "catalog:1:" ELSE "catalog:2:")
    \o (IF success THEN "success" ELSE "failure")
Next == RestoreAuth \/ (\E reason \in Reasons: Retire(reason))
  \/ (\E i \in Ids: Begin(i) \/ (\E success \in BOOLEAN: Complete(i, success)))
Spec == Init /\ [][Next]_vars
RetiredLoadingSettles == s.owner = 0 => ~s.loading
OwnedPendingKeepsLoading == s.owner \in Ids => s.loading
IneligibleCatalogIsSettled == ~s.enabled => ~s.loading
OnlyCurrentCatalogPublishes == s.displayEpoch \in {0, s.epoch} /\ s.errorEpoch \in {0, s.epoch}
CurrentCompletionSettles == s.requestState[2] \in {"success", "failure"} => ~s.loading
\* Begin is useModelCatalog.refresh or the Selection effect's delayed request.
\* Retire(context) maps to the hook context cleanup/replacement; no-auth maps
\* to ScreenVision reset and Selection's credential-less effect; separate-off
\* maps to Selection's closed independent-model branch. Retirement invalidates
\* ownership while leaving the old promise pending. It must settle its spinner.
\* RestoreAuth represents a later credential/mode change, then Begin(2) starts
\* a new spinner. Complete(1) can occur before or after that replacement begins.
\* Both success/catch publication and finally execute as explicit completion
\* effects. legacy-retirement drops retirement's settlement; finally removes
\* only completion ownership; publication removes only the publication fence.
=============================================================================
