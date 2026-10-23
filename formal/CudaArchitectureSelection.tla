----------------------- MODULE CudaArchitectureSelection -----------------------
EXTENDS Naturals, FiniteSets, Sequences

CONSTANTS Wrong, MaxTokens
VARIABLE s
vars == <<s>>

\* First CMake reports are inputs, including future and suffixed targets.
Tokens == {"80-virtual", "80-real", "80", "90-virtual", "90-real", "90",
           "90a-virtual", "90a-real", "100f-real", "130a-virtual", "89-real"}
Upstreams == UNION {[1..n -> Tokens] : n \in 1..MaxTokens}
Members(xs) == {xs[i] : i \in 1..Len(xs)}
NativeToken(arch) == IF arch = "80" THEN "80-real" ELSE "90-real"
VirtualToken(arch) == IF arch = "80" THEN "80-virtual" ELSE "90-virtual"
HasNative(xs, arch) == arch \in Members(xs) \/ NativeToken(arch) \in Members(xs)
NeedsNative(xs, arch) == VirtualToken(arch) \in Members(xs) /\ ~HasNative(xs, arch)
Additions(xs) ==
  (IF NeedsNative(xs, "80") THEN <<"80-real">> ELSE <<>>) \o
  (IF NeedsNative(xs, "90") THEN <<"90-real">> ELSE <<>>)
PlannedPhases == {"planned", "reported", "verified", "built", "packaged", "rejected"}
BuildPhases == {"built", "packaged"}
RepairNeeded == s.mode = "default" /\ s.plan # s.upstream
ValidReport == s.mode = "cpu" \/
  (s.report # <<>> /\ Members(s.report) \subseteq Tokens)

Init == s = [phase |-> "start", mode |-> "none", upstream |-> <<>>,
             plan |-> <<>>, report |-> <<>>, configurations |-> 0,
             manifest |-> [published |-> FALSE, architectures |-> <<>>]]

\* build_r2t2_runtime.main: initial configure, then resolved_cuda_architectures.
FirstConfigure(mode, upstream) ==
  /\ s.phase = "start"
  /\ s' = [s EXCEPT !.phase = "configured", !.mode = mode,
       !.upstream = upstream, !.report = upstream, !.configurations = 1]

\* portable_cuda_architectures: retain the exact first sequence, append only.
Plan ==
  /\ s.phase = "configured"
  /\ LET additions == IF s.mode = "default" THEN Additions(s.upstream) ELSE <<>>
         base == IF s.mode = "default" /\ Wrong = "drop-ptx" THEN
                   IF s.upstream[1] \in {"80-virtual", "90-virtual"}
                   THEN Tail(s.upstream) ELSE s.upstream
                 ELSE s.upstream
         chosen == IF s.mode = "default" /\ Wrong = "omit-native"
                   THEN s.upstream ELSE base \o additions
     IN s' = [s EXCEPT !.phase = "planned", !.plan = chosen]

\* A second CMake configure may report the plan, stale cache, changed or invalid output.
Reconfigure ==
  /\ s.phase = "planned" /\ RepairNeeded
  /\ \E report \in {s.plan, s.upstream, <<"89-real">>, <<"invalid">>}:
       s' = [s EXCEPT !.phase = "reported", !.report = report,
         !.configurations = 2]

\* Parse the actual final report; a requested -D value is not evidence of success.
Verify ==
  /\ s.phase = "reported" \/ (s.phase = "planned" /\ ~RepairNeeded)
  /\ LET accepted == ValidReport /\
           (~RepairNeeded \/ s.report = s.plan \/
             (Wrong = "accept-stale" /\ s.report = s.upstream))
     IN s' = [s EXCEPT !.phase = IF accepted THEN "verified" ELSE "rejected"]

Build ==
  /\ s.phase = "verified"
  /\ s' = [s EXCEPT !.phase = "built"]

Package ==
  /\ s.phase = "built"
  /\ s' = [s EXCEPT !.phase = "packaged",
       !.manifest = [published |-> TRUE, architectures |-> s.report]]

Next ==
  \/ \E mode \in {"default", "explicit"}, upstream \in Upstreams:
       FirstConfigure(mode, upstream)
  \/ FirstConfigure("cpu", <<>>)
  \/ Plan
  \/ Reconfigure
  \/ Verify
  \/ Build
  \/ Package
  \/ s.phase \in {"packaged", "rejected"} /\ UNCHANGED vars

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ s.phase \in {"start", "configured"} \cup PlannedPhases
  /\ s.mode \in {"none", "default", "explicit", "cpu"}
  /\ s.upstream \in Seq(Tokens) /\ s.plan \in Seq(Tokens)
  /\ s.report \in Seq(Tokens \cup {"invalid"})
  /\ s.configurations \in 0..2
  /\ s.manifest.published \in BOOLEAN /\ s.manifest.architectures \in Seq(Tokens)

\* Equality of the original prefix preserves duplicates, order and unknown tokens.
UpstreamPreserved == s.mode = "default" /\ s.phase \in PlannedPhases =>
  Len(s.plan) >= Len(s.upstream) /\ SubSeq(s.plan, 1, Len(s.upstream)) = s.upstream

SelectedGenericHasNative == s.mode = "default" /\ s.phase \in PlannedPhases =>
  \A arch \in {"80", "90"}: VirtualToken(arch) \in Members(s.upstream) =>
    HasNative(s.plan, arch)

OnlyRequiredNativeAdded == s.mode = "default" /\ s.phase \in PlannedPhases =>
  SubSeq(s.plan, Len(s.upstream) + 1, Len(s.plan)) = Additions(s.upstream)

ExplicitAndCpuUnchanged ==
  /\ (s.mode \in {"explicit", "cpu"} /\ s.phase \in PlannedPhases =>
        s.plan = s.upstream /\ s.configurations = 1)
  /\ (s.mode = "cpu" => s.upstream = <<>> /\ s.report = <<>>)

NoUnnecessaryReconfigure == s.phase \in PlannedPhases /\ ~RepairNeeded =>
  s.configurations = 1

FinalMismatchBlocksBuild == s.phase \in BuildPhases =>
  ValidReport /\ (RepairNeeded => s.configurations = 2 /\ s.report = s.plan)

ManifestMatchesActual ==
  /\ (s.phase = "packaged" => s.manifest.published /\ s.manifest.architectures = s.report)
  /\ (s.phase # "packaged" => ~s.manifest.published /\ s.manifest.architectures = <<>>)

=============================================================================
