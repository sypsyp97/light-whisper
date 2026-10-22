------------------------- MODULE ExactHeadSelection -------------------------
EXTENDS Naturals, FiniteSets, Sequences

CONSTANTS Wrong, MaxCalls, SeparateDevices, AllowRestart, Workload
VARIABLE s
vars == <<s>>

Contexts == {1, 2}
Keys == {<<64, 128>>, <<128, 64>>}
Profiles == {"eligible", "non2D", "noncontiguous", "weightType", "inputType",
             "precision", "bias"}
Choices == {"Serial", "Grouped"}
Outcomes == {"fast", "threshold", "slow", "mismatch", "partial",
             "zeroSerial", "zeroGrouped", "nonfinite"}
ActiveOutcomes == IF Workload = "lifecycle" THEN {"fast"} ELSE Outcomes
ActiveProfiles == IF Workload = "lifecycle" THEN {"eligible"} ELSE Profiles
Device(c) == IF SeparateDevices THEN c ELSE 1
Other(c) == IF c = 1 THEN 2 ELSE 1
NoPlan == [owner |-> 0, epoch |-> 0, device |-> 0, call |-> 0, key |-> <<0, 0>>,
  compared |-> 0, mismatches |-> 0, serialTicks |-> 0, groupedTicks |-> 0,
  finite |-> FALSE, choice |-> "none"]
NoOutput == "none"
EmptyRequest == [owner |-> 1, epoch |-> 1, device |-> Device(1), call |-> 0,
                 key |-> <<64, 128>>, profile |-> "eligible", capture |-> FALSE,
                 before |-> NoPlan]
ProtectedEffects == {"allocate", "compare", "create-events", "time", "synchronize"}

\* Each non-eligible profile changes exactly one explicit eligibility guard.
Descriptor(p) == [twoD |-> p # "non2D", contiguous |-> p # "noncontiguous",
  weights |-> IF p = "weightType" THEN "F32" ELSE "F16",
  input |-> IF p = "inputType" THEN "F16" ELSE "F32",
  precision |-> IF p = "precision" THEN "F32" ELSE "default",
  bias |-> p = "bias", columns |-> 4]
Eligible(p) == LET d == Descriptor(p) IN
  d.twoD /\ d.contiguous /\ d.weights = "F16" /\ d.input = "F32"
  /\ d.precision = "default" /\ ~d.bias /\ d.columns = 4

\* Tick values partition the >=5% boundary; they are not measured timings.
\* Nonfinite represents NaN/infinity; zeros exercise both positivity guards.
Observation(o, key) == [
  compared |-> IF o = "partial" THEN 0 ELSE 4 * key[2],
  mismatches |-> IF o = "mismatch" THEN 1 ELSE 0,
  serialTicks |-> IF o = "zeroSerial" THEN 0 ELSE 100,
  groupedTicks |-> CASE o = "threshold" -> 95 [] o = "slow" -> 96
                    [] o = "zeroGrouped" -> 0 [] OTHER -> 94,
  finite |-> o # "nonfinite"]
FullParity(p) == p.compared = 4 * p.key[2] /\ p.mismatches = 0
TimingWins(p) == p.finite /\ p.serialTicks > 0 /\ p.groupedTicks > 0
  /\ 100 * p.groupedTicks <= 95 * p.serialTicks
\* Record extension is explicit: provenance/observations are specification
\* history, not additional required production cache fields.
CompletedPlan(o) == LET v == Observation(o, s.request.key)
    p == [owner |-> s.request.owner, epoch |-> s.request.epoch,
      device |-> s.request.device, call |-> s.request.call, key |-> s.request.key,
      compared |-> v.compared, mismatches |-> v.mismatches,
      serialTicks |-> v.serialTicks, groupedTicks |-> v.groupedTicks,
      finite |-> v.finite]
    chosen == IF TimingWins(p) /\ (FullParity(p) \/ Wrong = "ignored-parity")
              THEN "Grouped" ELSE "Serial"
  IN [owner |-> p.owner, epoch |-> p.epoch, device |-> p.device, call |-> p.call,
      key |-> p.key, compared |-> p.compared, mismatches |-> p.mismatches,
      serialTicks |-> p.serialTicks, groupedTicks |-> p.groupedTicks,
      finite |-> p.finite, choice |-> chosen]

Output(producer, choice) == [owner |-> s.request.owner,
  epoch |-> s.request.epoch, call |-> s.request.call,
  device |-> s.request.device, key |-> s.request.key,
  producer |-> producer, choice |-> choice,
  scalarCalls |-> IF choice = "Serial" THEN 4 ELSE 0,
  groupedCalls |-> IF choice = "Grouped" THEN 1 ELSE 0]
Current == s.phase \in {"selected", "executed", "done"}
EpochEntries(history, epoch) == {i \in 1..Len(history): history[i].epoch = epoch}

Init == s = [phase |-> "idle", alive |-> [c \in Contexts |-> TRUE],
  epoch |-> [c \in Contexts |-> 1], calls |-> [c \in Contexts |-> 0],
  slot |-> [c \in Contexts |-> NoPlan],
  tunes |-> [c \in Contexts |-> <<>>], commits |-> [c \in Contexts |-> <<>>],
  request |-> EmptyRequest, route |-> "none", selected |-> "none",
  source |-> NoPlan, dst |-> NoOutput, effects |-> {}, outcome |-> "none",
  lastEvent |-> "init"]

Begin(c, key, profile, capture) ==
  /\ s.phase = "idle" /\ s.alive[c] /\ s.calls[c] < MaxCalls
  /\ s' = [s EXCEPT !.phase = "dispatch", !.calls[c] = @ + 1,
       !.request = [owner |-> c, epoch |-> s.epoch[c], device |-> Device(c),
         call |-> s.calls[c] + 1, key |-> key, profile |-> profile,
         capture |-> capture, before |-> s.slot[c]],
       !.lastEvent = "begin"]

Dispatch ==
  /\ s.phase = "dispatch"
  /\ LET c == s.request.owner
         local == s.slot[c]
         source == IF Wrong = "cross-context" /\ local = NoPlan
                   THEN s.slot[Other(c)] ELSE local
         hit == IF source = NoPlan THEN FALSE ELSE
           Eligible(s.request.profile) /\ Wrong # "mutable-plan"
           /\ (source.key = s.request.key \/ Wrong = "wrong-key")
         tune == Eligible(s.request.profile)
           /\ (local = NoPlan \/ Wrong = "mutable-plan")
           /\ (~s.request.capture \/ Wrong = "tune-in-capture")
     IN IF hit THEN
       s' = [s EXCEPT !.phase = "selected", !.route = "cache",
         !.selected = source.choice, !.source = source, !.lastEvent = "cache-hit"]
     ELSE IF tune THEN
       s' = [s EXCEPT !.phase = "calibrating", !.route = "calibration",
         !.tunes[c] = Append(@, [epoch |-> s.request.epoch,
           call |-> s.request.call, key |-> s.request.key,
           capture |-> s.request.capture]), !.lastEvent = "calibrate-begin"]
     ELSE s' = [s EXCEPT !.phase = "selected", !.route = "fallback",
       !.selected = "Serial", !.lastEvent = "serial-fallback"]

Calibrate(o) ==
  /\ s.phase = "calibrating"
  /\ LET c == s.request.owner
         plan == CompletedPlan(o)
     IN s' = [s EXCEPT !.phase = "selected", !.slot[c] = plan,
       !.commits[c] = Append(@, plan), !.selected = plan.choice,
       !.source = plan, !.outcome = o, !.effects = ProtectedEffects,
       !.dst = Output("calibration", "Grouped"), !.lastEvent = "calibrate-commit"]

ExecuteSelected ==
  /\ s.phase = "selected"
  /\ s' = [s EXCEPT !.phase = "executed",
       !.dst = Output("selected", s.selected), !.lastEvent = "execute-selected"]
Return ==
  /\ s.phase = "executed"
  /\ s' = [s EXCEPT !.phase = "done", !.lastEvent = "return"]
SkipSelected ==
  /\ Wrong = "skip-selected" /\ s.phase = "selected" /\ s.route = "calibration"
  /\ s' = [s EXCEPT !.phase = "done", !.lastEvent = "return-calibration-output"]
Finish ==
  /\ s.phase = "done"
  /\ s' = [s EXCEPT !.phase = "idle", !.request = EmptyRequest,
       !.route = "none", !.selected = "none", !.source = NoPlan,
       !.dst = NoOutput, !.effects = {}, !.outcome = "none", !.lastEvent = "finish"]

\* CUDA_CHECK -> [[noreturn]] ggml_cuda_error. No success-return or continuation.
ApiError ==
  /\ s.phase \in {"dispatch", "calibrating", "selected", "executed"}
  /\ s' = [s EXCEPT !.phase = "error", !.lastEvent = "cuda-api-error"]
Destroy(c) ==
  /\ AllowRestart /\ s.phase = "idle" /\ s.alive[c] /\ s.calls[c] > 0
  /\ s' = [s EXCEPT !.alive[c] = FALSE, !.slot[c] = NoPlan,
       !.lastEvent = "context-destroy"]
Recreate(c) ==
  /\ AllowRestart /\ s.phase = "idle" /\ ~s.alive[c] /\ s.epoch[c] = 1
  /\ s' = [s EXCEPT !.alive[c] = TRUE, !.epoch[c] = 2,
       !.slot[c] = NoPlan, !.lastEvent = "context-create"]

Next ==
  (\E c \in Contexts, key \in Keys, profile \in ActiveProfiles, capture \in BOOLEAN:
    Begin(c, key, profile, capture))
  \/ Dispatch \/ (\E o \in ActiveOutcomes: Calibrate(o))
  \/ ExecuteSelected \/ Return \/ SkipSelected \/ Finish \/ ApiError
  \/ (\E c \in Contexts: Destroy(c) \/ Recreate(c))
Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ s.phase \in {"idle", "dispatch", "calibrating", "selected", "executed", "done", "error"}
  /\ s.alive \in [Contexts -> BOOLEAN] /\ s.epoch \in [Contexts -> 1..2]
  /\ s.calls \in [Contexts -> 0..MaxCalls]
  /\ s.selected \in Choices \cup {"none"}
  /\ s.effects \subseteq ProtectedEffects
  /\ \A c \in Contexts: Len(s.tunes[c]) <= MaxCalls /\ Len(s.commits[c]) <= MaxCalls
SlotOwnership == \A c \in Contexts:
  s.slot[c] # NoPlan =>
    s.slot[c].owner = c /\ s.slot[c].epoch = s.epoch[c] /\ s.slot[c].device = Device(c)
OneCalibrationPerLifetime == \A c \in Contexts, e \in 1..2:
  Cardinality(EpochEntries(s.tunes[c], e)) <= 1
ImmutablePlan == \A c \in Contexts:
  /\ \A e \in 1..2: Cardinality(EpochEntries(s.commits[c], e)) <= 1
  /\ s.slot[c] # NoPlan =>
       \A i \in EpochEntries(s.commits[c], s.epoch[c]): s.slot[c] = s.commits[c][i]
GroupedRequiresEvidence == \A c \in Contexts: \A i \in 1..Len(s.commits[c]):
  LET plan == s.commits[c][i] IN
    plan.choice = "Grouped" => FullParity(plan) /\ TimingWins(plan)
CaptureSafe ==
  /\ \A c \in Contexts: \A i \in 1..Len(s.tunes[c]): ~s.tunes[c][i].capture
  /\ s.request.capture => s.effects \cap ProtectedEffects = {}
CachedKeyMatches == Current /\ s.route = "cache" => s.source.key = s.request.key
SelectedContextMatches == Current /\ s.route \in {"cache", "calibration"} =>
  s.source.owner = s.request.owner /\ s.source.epoch = s.request.epoch
  /\ s.source.device = s.request.device
CachedChoicePreserved == Current /\ s.route = "cache" => s.selected = s.source.choice
FallbackUsesSerial == Current /\
  (~Eligible(s.request.profile)
    \/ (s.request.capture /\ s.request.before = NoPlan)
    \/ (IF s.request.before = NoPlan THEN FALSE ELSE
          s.request.before.key # s.request.key))
    => s.route = "fallback" /\ s.selected = "Serial"
GroupedOnlyEligible == Current /\ s.selected = "Grouped" => Eligible(s.request.profile)
SelectedOutputExecuted == s.phase = "done" => s.dst = Output("selected", s.selected)
DestroyedContextClearsSlot == \A c \in Contexts: ~s.alive[c] => s.slot[c] = NoPlan

\* Reachability witnesses: expected violations, never safety claims.
NoGroupedCaptureHit == ~(s.phase = "done" /\ s.route = "cache"
  /\ s.request.capture /\ s.selected = "Grouped")
NoMismatchRestoration == ~(s.phase = "done" /\ s.route = "calibration"
  /\ s.outcome = "mismatch" /\ s.dst = Output("selected", "Serial"))
NoFreshContextRetune == ~\E c \in Contexts:
  EpochEntries(s.tunes[c], 1) # {} /\ EpochEntries(s.tunes[c], 2) # {}
=============================================================================
