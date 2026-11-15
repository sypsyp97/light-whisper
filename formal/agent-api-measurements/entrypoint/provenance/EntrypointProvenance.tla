--------------------- MODULE EntrypointProvenance ---------------------
EXTENDS Naturals, TLC
CONSTANT Mode
ASSUME Mode \in {"correct", "bad-head-admission", "fixed-authoring-head", "skip-drift-check"}

ValidHeads == {"valid-a", "valid-b"}
Heads == ValidHeads \cup {"invalid"}
NoHead == "none"
Ops == {"none", "apply", "verify", "restore"}
HeadValid(head) == head \in ValidHeads

(* Each helper invocation is synchronous and exclusive. Environment edits occur
   only between invocations. identityValid abstracts the complete stage-specific
   21-pin, exact variant/observer/backup, path and ownership conjunction.
   artifactValid is verify_launch's additional app/fixture admission conjunction.
   writes counts helper write operations, not bytes or environment metadata edits.
   launchAdmitted means verify_launch returned admission, not process execution. *)
VARIABLES phase, ownerHead, receiptHead, identityValid, artifactValid, overlay,
          appliedHead, recordedHead, writes, launchAdmitted,
          ownerDrifted, receiptDrifted, last
vars == <<phase, ownerHead, receiptHead, identityValid, artifactValid, overlay,
          appliedHead, recordedHead, writes, launchAdmitted,
          ownerDrifted, receiptDrifted, last>>

Snapshot(operation, admitted) ==
    [op |-> operation, owner |-> ownerHead, receipt |-> receiptHead,
     identity |-> identityValid, artifact |-> artifactValid,
     before |-> writes, accepted |-> admitted]

Init == /\ phase = "new"
        /\ ownerHead \in Heads
        /\ identityValid \in BOOLEAN /\ artifactValid \in BOOLEAN
        /\ receiptHead = NoHead /\ appliedHead = NoHead /\ recordedHead = NoHead
        /\ overlay = FALSE /\ writes = 0 /\ launchAdmitted = FALSE
        /\ ownerDrifted = FALSE /\ receiptDrifted = FALSE
        /\ last = [op |-> "none", owner |-> NoHead, receipt |-> NoHead,
                    identity |-> FALSE, artifact |-> FALSE,
                    before |-> 0, accepted |-> FALSE]

(* _mirror -> apply_overlay; the fixed-head negative changes only receipt data. *)
Apply == /\ phase = "new"
         /\ LET admitted == identityValid /\
                    (HeadValid(ownerHead) \/ Mode = "bad-head-admission")
                stamped == IF Mode = "fixed-authoring-head" THEN "valid-a" ELSE ownerHead
            IN /\ last' = Snapshot("apply", admitted)
               /\ phase' = IF admitted THEN "applied" ELSE "done"
               /\ overlay' = admitted
               /\ receiptHead' = IF admitted THEN stamped ELSE NoHead
               /\ recordedHead' = IF admitted THEN stamped ELSE NoHead
               /\ appliedHead' = IF admitted THEN ownerHead ELSE NoHead
               /\ writes' = IF admitted THEN 1 ELSE 0
         /\ UNCHANGED <<ownerHead, identityValid, artifactValid, launchAdmitted,
                         ownerDrifted, receiptDrifted>>

(* External edits, bounded to at most one owner edit, one receipt edit and one
   identity loss. Their order is unrestricted before the later helper call. *)
DriftOwner == /\ phase = "applied" /\ ~ownerDrifted
              /\ \E head \in Heads \ {ownerHead}: ownerHead' = head
              /\ ownerDrifted' = TRUE
              /\ UNCHANGED <<phase, receiptHead, identityValid, artifactValid, overlay,
                              appliedHead, recordedHead, writes, launchAdmitted,
                              receiptDrifted, last>>
DriftReceipt == /\ phase = "applied" /\ ~receiptDrifted
                /\ \E head \in Heads \ {receiptHead}: receiptHead' = head
                /\ receiptDrifted' = TRUE
                /\ UNCHANGED <<phase, ownerHead, identityValid, artifactValid, overlay,
                                appliedHead, recordedHead, writes, launchAdmitted,
                                ownerDrifted, last>>
DriftIdentity == /\ phase = "applied" /\ identityValid
                 /\ identityValid' = FALSE
                 /\ UNCHANGED <<phase, ownerHead, receiptHead, artifactValid, overlay,
                                 appliedHead, recordedHead, writes, launchAdmitted,
                                 ownerDrifted, receiptDrifted, last>>

(* _applied repeats _mirror and compares the current owner to the receipt before
   either public caller proceeds. The skip-drift negative removes only equality. *)
Invoke(operation) ==
    /\ phase = "applied"
    /\ LET admitted == identityValid /\ HeadValid(ownerHead)
                          /\ (receiptHead = ownerHead \/ Mode = "skip-drift-check")
                          /\ (operation = "restore" \/ artifactValid)
       IN /\ last' = Snapshot(operation, admitted)
          /\ phase' = "done"
          /\ overlay' = IF admitted /\ operation = "restore" THEN FALSE ELSE overlay
          /\ writes' = writes + IF admitted /\ operation = "restore" THEN 1 ELSE 0
          /\ launchAdmitted' = (admitted /\ operation = "verify")
    /\ UNCHANGED <<ownerHead, receiptHead, identityValid, artifactValid,
                    appliedHead, recordedHead, ownerDrifted, receiptDrifted>>
Verify == Invoke("verify")
Restore == Invoke("restore")
Terminal == /\ phase = "done" /\ UNCHANGED vars
Next == Apply \/ DriftOwner \/ DriftReceipt \/ DriftIdentity \/ Verify \/ Restore \/ Terminal
Spec == Init /\ [][Next]_vars

TypeOK == /\ phase \in {"new", "applied", "done"}
          /\ ownerHead \in Heads /\ receiptHead \in Heads \cup {NoHead}
          /\ appliedHead \in Heads \cup {NoHead} /\ recordedHead \in Heads \cup {NoHead}
          /\ identityValid \in BOOLEAN /\ artifactValid \in BOOLEAN /\ overlay \in BOOLEAN
          /\ launchAdmitted \in BOOLEAN /\ ownerDrifted \in BOOLEAN /\ receiptDrifted \in BOOLEAN
          /\ writes \in 0..2
          /\ last \in [op : Ops, owner : Heads \cup {NoHead}, receipt : Heads \cup {NoHead},
                       identity : BOOLEAN, artifact : BOOLEAN, before : 0..2, accepted : BOOLEAN]
UseGuard(call) == call.identity /\ HeadValid(call.owner) /\ call.receipt = call.owner
CallGuard(call) == IF call.op = "apply" THEN call.identity /\ HeadValid(call.owner)
                   ELSE call.op \in {"verify", "restore"} /\ UseGuard(call)
                          /\ (call.op = "restore" \/ call.artifact)

ValidHeadAdmission == (last.op = "apply" /\ last.accepted) => HeadValid(last.owner)
RecordedOwnerProvenance == appliedHead # NoHead => recordedHead = appliedHead
ContentIdentityGate == last.accepted => last.identity
LaunchAdmissionGate == launchAdmitted =>
    last.op = "verify" /\ last.accepted /\ UseGuard(last) /\ last.artifact /\ overlay
RestorationGate == (last.op = "restore" /\ last.accepted) => UseGuard(last) /\ ~overlay
RefusalHasNoEffects == (last.op # "none" /\ ~CallGuard(last)) =>
    writes = last.before /\ ~launchAdmitted
VerificationIsReadOnly == last.op = "verify" => writes = last.before
OverlayLifecycle == /\ (phase = "new" => ~overlay)
                    /\ (phase = "applied" => overlay)
                    /\ (last.op = "apply" /\ ~last.accepted =>
                          ~overlay /\ receiptHead = NoHead /\ recordedHead = NoHead)

(* Expected invariant failures are reachability witnesses, never safety kills. *)
NoValidNewHeadApply == ~(last.op = "apply" /\ last.accepted /\ last.owner = "valid-b"
                          /\ recordedHead = "valid-b")
NoBadHeadRefusal == ~(last.op = "apply" /\ last.owner = "invalid" /\ last.identity /\ ~last.accepted
                       /\ writes = 0 /\ receiptHead = NoHead)
CleanDriftRefusal(operation) ==
    /\ last.op = operation /\ ~last.accepted /\ last.identity
    /\ HeadValid(last.owner) /\ HeadValid(last.receipt) /\ last.owner # last.receipt
    /\ (operation = "restore" \/ last.artifact)
    /\ writes = last.before /\ ~launchAdmitted
NoOwnerDriftLaunchRefusal == ~(CleanDriftRefusal("verify") /\ ownerDrifted /\ ~receiptDrifted)
NoOwnerDriftRestoreRefusal == ~(CleanDriftRefusal("restore") /\ ownerDrifted /\ ~receiptDrifted)
NoReceiptDriftLaunchRefusal == ~(CleanDriftRefusal("verify") /\ receiptDrifted /\ ~ownerDrifted)
NoReceiptDriftRestoreRefusal == ~(CleanDriftRefusal("restore") /\ receiptDrifted /\ ~ownerDrifted)
=============================================================================
