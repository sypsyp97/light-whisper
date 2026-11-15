---------------------------- MODULE Protocol ----------------------------
EXTENDS Naturals, Sequences
CONSTANTS Mode, Target
VARIABLES scenario, phase, connected, dispatched, replyKind, replyId,
          leaked, accepted, done
vars == <<scenario, phase, connected, dispatched, replyKind, replyId,
          leaked, accepted, done>>

Scenarios == {"ok", "endpoint-acl", "endpoint-schema", "endpoint-size",
  "header", "auth", "inner", "frame-size", "frame-truncated", "backend-false",
  "handler-panic", "payload-shape", "payload-secret", "response-size",
  "stale-protocol", "stale-instance", "stale-id", "untyped-success", "controller-id",
  "request-nonce", "response-mac", "response-domain", "response-nonce"}
EndpointOK == scenario \notin {"endpoint-acl", "endpoint-schema", "endpoint-size"}
HeaderOK == scenario # "header"
RequestMACOK == scenario # "auth"
RequestNonceOK == scenario # "request-nonce"
InnerOK == scenario # "inner"
FrameOK == scenario \notin {"frame-size", "frame-truncated"}
ControllerOK == scenario \notin {"payload-shape", "payload-secret", "response-size"}
ClientReplyOK == scenario \notin {"stale-protocol", "stale-instance", "stale-id", "untyped-success"}
ClientMACOK == scenario \notin {"response-mac", "response-domain"}
ClientNonceOK == scenario # "response-nonce"

Init == /\ scenario \in Scenarios
        /\ phase = "discover" /\ connected = FALSE /\ dispatched = FALSE
        /\ replyKind = "none" /\ replyId = 0 /\ leaked = FALSE
        /\ accepted = FALSE /\ done = FALSE
Discover == /\ phase = "discover"
            /\ IF EndpointOK \/ Mode = "endpoint"
                  THEN /\ connected' = TRUE /\ phase' = "ingress"
                  ELSE /\ connected' = FALSE /\ phase' = "terminal"
            /\ done' = (phase' = "terminal")
            /\ UNCHANGED <<scenario, dispatched, replyKind, replyId, leaked, accepted>>
Ingress == /\ phase = "ingress"
           /\ IF (FrameOK \/ Mode = "frame")
                 /\ (HeaderOK \/ Mode = "auth")
                 /\ (RequestMACOK \/ Mode = "request-mac")
                 /\ (RequestNonceOK \/ Mode = "request-nonce")
                 /\ (InnerOK \/ Mode = "inner")
                 THEN /\ dispatched' = TRUE /\ phase' = "handler"
                      /\ replyKind' = "none"
                 ELSE /\ dispatched' = FALSE /\ phase' = "terminal"
                      /\ replyKind' = IF ~HeaderOK THEN "authentication_failed" ELSE "invalid_request"
           /\ done' = (phase' = "terminal")
           /\ UNCHANGED <<scenario, connected, replyId, leaked, accepted>>
Handler == /\ phase = "handler"
           /\ phase' = "client-read"
           /\ replyKind' = IF scenario = "handler-panic" /\ Mode # "panic" THEN "operation_failed"
                ELSE IF ~ControllerOK /\ Mode # "payload" THEN "invalid_response"
                ELSE IF scenario = "backend-false" THEN "backend_failure" ELSE "success"
           /\ replyId' = IF scenario = "controller-id" /\ Mode = "server-id" THEN 2 ELSE 1
           /\ leaked' = ((scenario = "payload-secret" /\ Mode = "payload")
                        \/ (scenario = "handler-panic" /\ Mode = "panic"))
           /\ UNCHANGED <<scenario, connected, dispatched, accepted, done>>
ClientRead == /\ phase = "client-read"
              /\ accepted' = ((ClientReplyOK \/ Mode = "correlation")
                             /\ (ClientMACOK \/ Mode = "response-mac")
                             /\ (ClientNonceOK \/ Mode = "response-nonce"))
              /\ phase' = "terminal" /\ done' = TRUE
              /\ UNCHANGED <<scenario, connected, dispatched, replyKind, replyId, leaked>>
Terminal == /\ phase = "terminal" /\ UNCHANGED vars
Next == Discover \/ Ingress \/ Handler \/ ClientRead \/ Terminal
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK == /\ scenario \in Scenarios
          /\ phase \in {"discover", "ingress", "handler", "client-read", "terminal"}
          /\ connected \in BOOLEAN /\ dispatched \in BOOLEAN /\ accepted \in BOOLEAN
          /\ leaked \in BOOLEAN /\ done \in BOOLEAN /\ replyId \in 0..2
          /\ replyKind \in {"none", "authentication_failed", "invalid_request", "operation_failed",
                            "invalid_response", "backend_failure", "success"}
PrivateEndpoint == connected => EndpointOK
AuthenticatedDispatch == dispatched => HeaderOK
VerifiedRequestMAC == dispatched => RequestMACOK
RequestNonceValid == dispatched => RequestNonceOK
ValidatedDispatch == dispatched => InnerOK
BoundedCompleteFrame == dispatched => FrameOK
SanitizedReply == ~leaked /\ ((phase \in {"client-read", "terminal"} /\ dispatched /\ ~ControllerOK)
                             => replyKind = "invalid_response")
ServerOwnsIdentity == dispatched /\ phase \in {"client-read", "terminal"} => replyId = 1
CorrelatedClient == accepted => ClientReplyOK
VerifiedResponseMAC == accepted => ClientMACOK
ResponseNonceValid == accepted => ClientNonceOK
EventuallySettled == <>done
WitnessNotReached == ~CASE Target = "valid" -> accepted /\ scenario = "ok"
                       [] Target = "backend-failure" -> accepted /\ replyKind = "backend_failure"
                       [] Target = "rejected-auth" -> done /\ scenario = "auth" /\ ~dispatched
                       [] Target = "sanitized" -> done /\ scenario = "payload-secret" /\ ~leaked
                       [] OTHER -> FALSE
=============================================================================
