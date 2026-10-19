------------------------ MODULE LlmRequestParameters ------------------------
EXTENDS Naturals
VARIABLE q
Routes == {"OpenRouter", "CustomOpenRouter", "OtherOpenAI", "Anthropic"}
Modes == {"Default", "Off", "Light", "Balanced", "Deep"}
IsOpenRouter(route) == route \in {"OpenRouter", "CustomOpenRouter"}
RouterControl(mode) == CASE mode = "Off" -> "Disabled"
  [] mode = "Light" -> "Low" [] mode = "Balanced" -> "Medium"
  [] mode = "Deep" -> "High" [] OTHER -> "Omitted"
DefaultLimit(route) == IF route = "Anthropic" THEN 16384 ELSE 0
Init == q = [phase |-> "Idle", route |-> "OtherOpenAI", mode |-> "Default",
  control |-> "Omitted", limit |-> 0, retries |-> 0, reasoningRejected |-> FALSE]
Build(route, mode) ==
  /\ q.phase = "Idle" /\ route \in Routes /\ mode \in Modes
  /\ q' = [q EXCEPT !.phase = "Pending", !.route = route, !.mode = mode,
    !.control = IF IsOpenRouter(route) THEN RouterControl(mode) ELSE "Native",
    !.limit = DefaultLimit(route)]
RejectReasoning ==
  /\ q.phase = "Pending" /\ IsOpenRouter(q.route) /\ q.mode = "Off"
  /\ q' = [q EXCEPT !.phase = "Retry", !.control = "Omitted",
    !.retries = 1, !.reasoningRejected = TRUE]
RejectLimit ==
  /\ q.phase = "Pending" /\ q.route = "Anthropic"
  /\ q' = [q EXCEPT !.phase = "Retry", !.limit = 4096, !.retries = 1]
Accept ==
  /\ q.phase \in {"Pending", "Retry"}
  /\ q' = [q EXCEPT !.phase = "Done"]
Fail ==
  /\ q.phase \in {"Pending", "Retry"}
  /\ q' = [q EXCEPT !.phase = "Error"]
Next == (\E route \in Routes, mode \in Modes: Build(route, mode))
  \/ RejectReasoning \/ RejectLimit \/ Accept \/ Fail
Spec == Init /\ [][Next]_q /\ WF_q(Accept \/ Fail)
OpenRouterModesMatch == q.phase # "Idle" /\ IsOpenRouter(q.route) =>
  q.control = IF q.reasoningRejected THEN "Omitted" ELSE RouterControl(q.mode)
OptionalOutputLimitOmitted == q.route # "Anthropic" => q.limit = 0
NativeOutputLimitRequired == q.route = "Anthropic" => q.limit \in {4096, 16384}
CompatibilityRetryBounded == q.retries <= 1
RequestsSettle == q.phase \in {"Pending", "Retry"} ~> q.phase \in {"Done", "Error"}
=============================================================================
