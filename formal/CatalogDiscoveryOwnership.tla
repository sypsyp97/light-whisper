---------------------- MODULE CatalogDiscoveryOwnership ----------------------
EXTENDS Naturals, FiniteSets, Sequences
CONSTANT Wrong
VARIABLE s, lastEvent
vars == <<s, lastEvent>>
Roles == {"polish", "assistant"}
Other(role) == IF role = "polish" THEN "assistant" ELSE "polish"
Text(number) == IF number = 1 THEN "1" ELSE "2"
Init ==
  /\ s \in [shared : BOOLEAN,
    issued : [Roles -> {0}], owner : [Roles -> {0}], intended : [Roles -> {0}],
    pending : [Roles -> { {} }], catalog : [Roles -> {0}], wanted : [Roles -> {0}],
    error : [Roles -> {FALSE}], wantedError : [Roles -> {FALSE}],
    loading : [Roles -> {FALSE}], wantedLoading : [Roles -> {FALSE}],
    invalidated : [Roles -> {FALSE}]]
  /\ lastEvent = "init"
Begin(role) ==
  /\ s.issued[role] < 2 /\ (~s.shared \/ role = "polish")
  /\ LET id == s.issued[role] + 1 IN
     s' = [s EXCEPT !.issued[role] = id, !.owner[role] = id,
       !.intended[role] = id, !.pending[role] = @ \cup {id},
       !.loading[role] = TRUE, !.wantedLoading[role] = TRUE,
       !.error[role] = FALSE, !.wantedError[role] = FALSE]
  /\ lastEvent' = "catalog:" \o role \o ":begin:" \o Text(s.issued[role] + 1)
Complete(role, id, success) ==
  /\ id \in s.pending[role]
  /\ LET owns == s.owner[role] = id
         intended == s.intended[role] = id
         base == [s EXCEPT !.pending[role] = @ \ {id},
           !.catalog[role] = IF owns /\ success THEN id ELSE @,
           !.wanted[role] = IF intended /\ success THEN id ELSE @,
           !.error[role] = IF owns THEN ~success ELSE @,
           !.wantedError[role] = IF intended THEN ~success ELSE @,
           !.loading[role] = IF owns THEN FALSE ELSE @,
           !.wantedLoading[role] = IF intended THEN FALSE ELSE @]
         published == IF role = "polish" /\ owns /\ success /\ s.shared
           THEN [base EXCEPT !.catalog["assistant"] = id,
             !.wanted["assistant"] = id, !.error["assistant"] = FALSE,
             !.wantedError["assistant"] = FALSE, !.loading["assistant"] = FALSE,
             !.wantedLoading["assistant"] = FALSE]
           ELSE base
     IN s' = IF Wrong = "polish-retires-independent" /\ role = "polish"
           /\ owns /\ success /\ ~s.shared /\ s.owner["assistant"] # 0
       THEN [published EXCEPT !.owner["assistant"] = 0] ELSE published
  /\ lastEvent' = "catalog:" \o role \o ":" \o
       (IF success THEN "success:" ELSE "failure:") \o Text(id)
Invalidate(role) ==
  /\ ~s.invalidated[role]
  /\ (~s.shared \/ role = "polish")
  /\ LET cleared == [s EXCEPT !.owner[role] = 0, !.intended[role] = 0,
         !.catalog[role] = 0, !.wanted[role] = 0,
         !.error[role] = FALSE, !.wantedError[role] = FALSE,
         !.loading[role] = FALSE, !.wantedLoading[role] = FALSE,
         !.invalidated[role] = TRUE]
         projected == IF s.shared /\ role = "polish"
           THEN [cleared EXCEPT !.catalog["assistant"] = 0,
             !.wanted["assistant"] = 0] ELSE cleared
     IN s' = IF Wrong = "global-invalidation"
       THEN [projected EXCEPT !.owner[Other(role)] = 0] ELSE projected
  /\ lastEvent' = "catalog:" \o role \o ":invalidate"
Next == (\E role \in Roles: Begin(role) \/ Invalidate(role))
  \/ (\E role \in Roles, id \in 1..2, success \in BOOLEAN: Complete(role, id, success))
Spec == Init /\ [][Next]_vars
LatestValidCompletionPublishes == s.catalog = s.wanted /\ s.error = s.wantedError
LoadingOwnershipPreserved == s.loading = s.wantedLoading
SelectedInvalidationClears == \A role \in Roles:
  s.invalidated[role] /\ s.intended[role] = 0 =>
    /\ s.catalog[role] = 0 /\ ~s.error[role] /\ ~s.loading[role]
SharedProjectionPreserved == s.shared => s.catalog["assistant"] = s.catalog["polish"]
\* Two roles, two issued requests each, one selective invalidation per role.
\* Native request completion is success/failure input. intended/wanted are
\* specification history: selected invalidation alone retires an intention.
\* Auth/context/unmount remain the separate AsyncSettings obligations.
\* Queued callback cancellation is implementation-test coverage only: this
\* companion has no scheduler/automatic-dispatch state or cancellation proof.
=============================================================================
