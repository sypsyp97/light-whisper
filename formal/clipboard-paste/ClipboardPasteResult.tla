----------------------- MODULE ClipboardPasteResult -----------------------
CONSTANT RestoreOverridesSend
VARIABLES phase, send, cleanup, result
vars == <<phase, send, cleanup, result>>

\* One clipboard-mode dispatch, starting at the Ctrl+V attempt in
\* paste_text_to_target. OS input acceptance is the delivery boundary.
Init == /\ phase = "send" /\ send = "pending"
        /\ cleanup = "pending" /\ result = "pending"

Send == /\ phase = "send"
        /\ send' \in {"accepted", "failed"}
        /\ phase' = "guard" /\ UNCHANGED <<cleanup, result>>

\* read_text + should_restore_clipboard_after_paste determines ownership.
Guard == /\ phase = "guard"
         /\ \/ /\ phase' = "restore" /\ UNCHANGED cleanup
            \/ /\ phase' = "return" /\ cleanup' = "skipped"
         /\ UNCHANGED <<send, result>>

Restore == /\ phase = "restore" /\ phase' = "return"
           /\ cleanup' \in {"restored", "failed"} /\ UNCHANGED <<send, result>>

\* drain_pending_pastes queues recovery for an Err from this branch.
Return == /\ phase = "return" /\ phase' = "done"
          /\ result' = IF send = "failed" \/ (RestoreOverridesSend /\ cleanup = "failed")
                       THEN "recovery" ELSE "success"
          /\ UNCHANGED <<send, cleanup>>

Next == Send \/ Guard \/ Restore \/ Return \/ (phase = "done" /\ UNCHANGED vars)
Spec == Init /\ [][Next]_vars
DispatchAgreesWithSend == phase = "done" => ((result = "success") <=> (send = "accepted"))
=============================================================================
