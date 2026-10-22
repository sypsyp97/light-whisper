------------------------ MODULE HistoryFilterOwnership -----------------------
EXTENDS Naturals, Sequences
CONSTANT Wrong
VARIABLE s, lastEvent
vars == <<s, lastEvent>>
Init ==
  /\ s = [filter |-> 0, pageFilter |-> 0, old |-> "absent", reset |-> "absent",
    page |-> "absent", owner |-> 0, pendingReset |-> FALSE,
    loading |-> FALSE, error |-> FALSE, hasMore |-> TRUE,
    items |-> <<1>>, offset |-> 0, requestFilter |-> 0, requestKind |-> "none",
    obsoleteOK |-> TRUE]
  /\ lastEvent = "init"
\* This is the production button guard. The failed-reset negative removes the
\* error guard; no transition is enabled solely because a fault was selected.
PaginationAvailable == s.hasMore /\ ~s.loading /\
  (Wrong = "failed-reset-pagination" \/ ~s.error)
BeginOld ==
  /\ PaginationAvailable /\ s.filter = 0 /\ s.old = "absent"
  /\ s' = [s EXCEPT !.old = "pending", !.owner = 1,
    !.offset = Len(s.items), !.requestFilter = s.filter, !.requestKind = "old",
    !.error = FALSE]
  /\ lastEvent' = "history:old:begin"
ChangeFilter ==
  /\ s.filter = 0
  /\ s' = [s EXCEPT !.filter = 1, !.reset = "queued", !.pendingReset = TRUE,
    !.loading = IF Wrong = "pagination-before-reset" THEN @ ELSE TRUE,
    !.owner = IF Wrong = "late-retirement" THEN @ ELSE 0]
  /\ lastEvent' = "history:filter:change"
DispatchReset ==
  /\ s.reset = "queued"
  /\ s' = [s EXCEPT !.reset = "pending", !.owner = 2, !.loading = TRUE,
    !.error = FALSE, !.offset = 0, !.requestFilter = s.filter,
    !.requestKind = "reset"]
  /\ lastEvent' = "history:reset:dispatch"
CompleteOld(success) ==
  /\ s.old = "pending"
  /\ LET owns == s.owner = 1 IN
     s' = [s EXCEPT !.old = "complete",
       !.items = IF owns /\ success THEN Append(@, 2) ELSE @,
       !.error = IF owns THEN ~success ELSE @,
       !.loading = IF owns THEN FALSE ELSE @,
       !.obsoleteOK = @ /\ ~(s.filter = 1 /\ owns)]
  /\ lastEvent' = "history:old:" \o (IF success THEN "success" ELSE "failure")
CompleteReset(success) ==
  /\ s.reset \in {"pending", "retry"} /\ s.owner \in {2, 3}
  /\ s' = [s EXCEPT !.reset = IF success THEN "success" ELSE "failed",
    !.pendingReset = ~success, !.loading = FALSE, !.error = ~success,
    !.items = IF success THEN <<3, 4>> ELSE @,
    !.pageFilter = IF success THEN s.filter ELSE @,
    !.hasMore = IF success THEN TRUE ELSE @]
  /\ lastEvent' = "history:reset:" \o (IF success THEN "success" ELSE "failure")
Retry ==
  /\ s.reset = "failed"
  /\ s' = [s EXCEPT !.reset = "retry", !.owner = 3,
    !.loading = TRUE, !.error = FALSE, !.offset = 0,
    !.requestFilter = s.filter, !.requestKind = "retry"]
  /\ lastEvent' = "history:reset:retry"
Paginate ==
  /\ PaginationAvailable /\ s.filter = 1 /\ s.page = "absent"
  /\ s' = [s EXCEPT !.page = "pending", !.owner = 4, !.error = FALSE,
    !.offset = Len(s.items), !.requestFilter = s.filter, !.requestKind = "page"]
  /\ lastEvent' = "history:pagination:begin"
CompletePage(success) ==
  /\ s.page = "pending" /\ s.owner = 4
  /\ s' = [s EXCEPT !.page = IF success THEN "success" ELSE "failed",
    !.items = IF success THEN Append(@, 5) ELSE @,
    !.hasMore = IF success THEN FALSE ELSE @,
    !.error = ~success, !.loading = FALSE]
  /\ lastEvent' = "history:pagination:" \o (IF success THEN "success" ELSE "failure")
Next == BeginOld \/ ChangeFilter \/ DispatchReset \/ Retry \/ Paginate
  \/ (\E success \in BOOLEAN: CompleteOld(success) \/ CompleteReset(success) \/ CompletePage(success))
Spec == Init /\ [][Next]_vars
ObsoleteCannotPublishOrEndReset == s.obsoleteOK
PendingResetBlocksPagination == s.pendingReset => ~PaginationAvailable
FailedResetRetainsBoundary == s.reset = "failed" => s.pendingReset /\ s.error /\ ~s.loading
ResetUsesZeroOffset == s.requestKind \in {"reset", "retry"} => s.offset = 0
PaginationUsesCurrentPage == s.page = "pending" =>
  s.pageFilter = s.filter /\ s.requestFilter = s.filter /\ s.offset = 2 /\ Len(s.items) = 2
ReplacementSuccessReplacesRows == s.reset = "success" =>
  /\ s.pageFilter = s.filter /\ ~s.pendingReset
  /\ SubSeq(s.items, 1, 2) = <<3, 4>>
NormalPaginationAppends == s.page = "success" => s.items = <<3, 4, 5>> /\ ~s.hasMore
\* One old epoch with seed row 1 and one optional old page row 2; one filter
\* change; reset/retry replace with rows 3,4; one normal page appends row 5.
\* Request identities are old=1/reset=2/retry=3/page=4. Repeated retries fold
\* into identity 3; all success/failure outcomes are environmental inputs.
\* pendingReset is specification intent, not a production availability guard.
\* ChangeFilter is the effect boundary; DispatchReset abstracts debounce.
\* Actual replay separately checks the exact 179/180 ms boundary.
=============================================================================
