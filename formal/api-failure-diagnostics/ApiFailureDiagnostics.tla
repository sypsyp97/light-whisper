----------------------- MODULE ApiFailureDiagnostics -----------------------
EXTENDS Naturals, Sequences

CONSTANT Mode
VARIABLES operation, outcome, phase, result, warnings
Vars == <<operation, outcome, phase, result, warnings>>

\* discovery_at exits at send, HTTP status, reading bytes, decoding, trust, or success.
Discovery == {"timeout", "dns", "tls", "connection_refused", "connect", "transport",
              "401", "503", "response_read", "invalid_metadata", "untrusted_metadata", "ok"}
\* evaluate_with_warning exits before build, while executing, validating, or at deadline.
Decision == {"invalid_request", "empty", "missing_key", "request_build", "transport",
             "401", "503", "response_read", "invalid_json", "invalid_answers", "timeout", "ok"}
Success == {"ok", "empty"}
Statuses == {"401", "503"}
Providers == {"TypeSafe", "OpenRouter", "Vercel", "Liquid", "Custom"}
Operations == {"chatgpt_discovery"} \cup Providers
Descriptor == IF operation = "chatgpt_discovery" THEN "chatgpt_discovery" ELSE "decision_model"
Private == "raw-input"
Init ==
    /\ operation \in Operations
    /\ outcome \in IF operation = "chatgpt_discovery" THEN Discovery ELSE Decision
    /\ phase = "calling" /\ result = "unset" /\ warnings = <<>>

\* One resolved async request. The HTTP/network/auth decision itself is an input.
\* Reasons stand for published fixed strings, including authored Chinese decision errors.
Record ==
    [owner |-> Descriptor, provider |-> IF operation \in Providers THEN operation ELSE "none",
     level |-> "warn", reason |-> IF Mode = "wrong-reason" THEN "unknown"
                                  ELSE IF outcome \in Statuses THEN "http_status" ELSE outcome,
     status |-> IF outcome \in Statuses THEN outcome ELSE "none",
     extra |-> IF Mode = "leak" THEN Private ELSE "none"]
Finish ==
    /\ phase = "calling" /\ phase' = "finished"
    /\ result' = IF Mode = "change-result" THEN "replacement" ELSE outcome
    /\ warnings' = IF Mode = "silent" \/ (outcome \in Success /\ Mode # "log-success") THEN <<>>
                   ELSE IF Mode = "duplicate" THEN <<Record, Record>> ELSE <<Record>>
    /\ UNCHANGED <<operation, outcome>>
Done == phase = "finished" /\ UNCHANGED Vars
\* Check only return-side effects after the awaited operation resolves.
Spec == Init /\ [][Finish \/ Done]_Vars

OneWarning == phase = "finished" /\ outcome \notin Success => Len(warnings) = 1
Quiet == outcome \in Success => warnings = <<>>
PreserveResult == phase = "finished" => result = outcome
PrivateFieldsExcluded == \A i \in 1..Len(warnings):
    Private \notin {warnings[i][key] : key \in DOMAIN warnings[i]}
CorrectReason == \A i \in 1..Len(warnings):
    /\ warnings[i].level = "warn"
    /\ warnings[i].owner = Descriptor
    /\ warnings[i].provider = IF operation \in Providers THEN operation ELSE "none"
    /\ IF outcome \in Statuses THEN warnings[i].reason = "http_status" /\ warnings[i].status = outcome
       ELSE warnings[i].reason = outcome /\ warnings[i].status = "none"
=============================================================================
