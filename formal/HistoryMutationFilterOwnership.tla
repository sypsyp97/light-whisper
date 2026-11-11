-------------------- MODULE HistoryMutationFilterOwnership -------------------
EXTENDS Naturals, Sequences
CONSTANT Wrong
VARIABLE s, lastEvent
vars == <<s, lastEvent>>
Kinds == {"polish", "asr", "delete-removed", "delete-missing"}
Deletes == {"delete-removed", "delete-missing"}
Init ==
  /\ s = [filter |-> 0, pageFilter |-> 0, capturedFilter |-> 0,
    mutation |-> "unused", kind |-> "none", filterLoad |-> "unused",
    refresh |-> "unused", requestFilter |-> 0, working |-> FALSE]
  /\ lastEvent = "init"
BeginMutation(kind) ==
  /\ s.mutation = "unused" /\ s.filter = 0
  /\ s' = [s EXCEPT !.mutation = "pending", !.kind = kind,
    !.capturedFilter = s.filter, !.working = TRUE]
  /\ lastEvent' = "mutation:begin:" \o kind
ChangeFilter ==
  /\ s.mutation = "pending" /\ s.filter = 0
  /\ s' = [s EXCEPT !.filter = 1, !.filterLoad = "pending"]
  /\ lastEvent' = "filter:change:B"
CompleteFilter ==
  /\ s.filterLoad = "pending"
  /\ s' = [s EXCEPT !.filterLoad = "success", !.pageFilter = s.filter]
  /\ lastEvent' = "filter:B:success"
CompleteMutation(success) ==
  /\ s.mutation = "pending" /\ s.filterLoad = "success"
  /\ LET captured == Wrong = "legacy" \/ (Wrong = "delete-capture" /\ s.kind \in Deletes)
     IN s' = [s EXCEPT !.mutation = IF success THEN "success" ELSE "failure",
       !.refresh = IF success THEN "pending" ELSE "unused",
       !.requestFilter = IF captured THEN s.capturedFilter ELSE s.filter,
       !.working = success]
  /\ lastEvent' = "mutation:" \o (IF success THEN "success:" ELSE "failure:") \o s.kind
CompleteRefresh(success) ==
  /\ s.refresh = "pending"
  /\ s' = [s EXCEPT !.refresh = IF success THEN "success" ELSE "failure",
    !.pageFilter = IF success THEN s.requestFilter ELSE @, !.working = FALSE]
  /\ lastEvent' = "refresh:" \o (IF success THEN "success" ELSE "failure")
Next == (\E kind \in Kinds: BeginMutation(kind)) \/ ChangeFilter \/ CompleteFilter
  \/ (\E success \in BOOLEAN: CompleteMutation(success) \/ CompleteRefresh(success))
Spec == Init /\ [][Next]_vars
MutationRefreshUsesVisibleFilter == s.refresh # "unused" => s.requestFilter = s.filter
LoadedFilterKeepsItsRows == s.filterLoad = "success" => s.pageFilter = s.filter
CompletionSettlesWorking == s.refresh \in {"success", "failure"} => ~s.working
\* BeginMutation captures the UI handler under filter A. ChangeFilter and its
\* successful list result load B while that mutation remains pending.
\* CompleteMutation is reprocess's refresh or either delete branch's refresh,
\* after any deletion animation. loadRef.current(true) chooses the visible
\* callback; legacy load(true) chooses capturedFilter. CompleteRefresh accepts
\* that newly dispatched request, so request-id fencing cannot repair its input.
\* Failure outcomes are inputs. No transition assumes that B rows survive.
=============================================================================
