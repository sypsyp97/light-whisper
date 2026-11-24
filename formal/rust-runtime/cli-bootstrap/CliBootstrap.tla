--------------------------- MODULE CliBootstrap ----------------------------
EXTENDS Naturals, FiniteSets

CONSTANT Mode
Clients == {1, 2}
HostKinds == {"empty", "legacy", "staleNative", "matchingUnstamped", "matchingNative"}
Bundles == {"valid", "none", "badStream", "link", "badStamp"}
DiskKinds == {"empty", "damaged", "ready", "partial", "badPublished"}
Results == {"none", "host", "cache", "error", "missing"}

(* One digest and two processes. The immutable input is (host, bundle, cache).
   These classes cover each discovery/validation branch and all lock races
   on the clean/upgrade path. No actor modifies the installation archive. *)
Cases == {<<"empty", "valid", "empty">>, <<"legacy", "valid", "empty">>,
          <<"staleNative", "valid", "empty">>, <<"matchingNative", "valid", "empty">>,
          <<"matchingUnstamped", "valid", "empty">>, <<"legacy", "valid", "ready">>,
          <<"legacy", "valid", "damaged">>, <<"legacy", "none", "empty">>,
          <<"empty", "none", "empty">>, <<"staleNative", "none", "empty">>,
          <<"matchingUnstamped", "none", "empty">>, <<"matchingNative", "none", "empty">>,
          <<"legacy", "link", "empty">>, <<"legacy", "badStream", "empty">>,
          <<"legacy", "badStamp", "empty">>}

VARIABLE s
vars == <<s>>
HostAllowed == IF s.input[2] = "none" THEN s.input[1] \in {"matchingNative","staleNative"}
               ELSE s.input[1] = "matchingNative"
Init ==
    \E input \in Cases, doctors \in {{},{1},{1,2}} :
        s = [input |-> input, doctors |-> doctors, host |-> input[1], disk |-> input[3],
             pc |-> [i \in Clients |-> "inspect"], result |-> [i \in Clients |-> "none"],
             contenders |-> {}, stage |-> {}, wrote |-> {}, validated |-> {},
             waitingBehind |-> {}, rechecked |-> {}, published |-> {}, cleaned |-> {}, rejected |-> {}, sawStage |-> {}]

Done(i, result) == [s EXCEPT !.pc[i] = "done", !.result[i] = result,
                             !.contenders = @ \ {i}]
Inspect(i) ==
    /\ s.pc[i] = "inspect"
    /\ s' = IF HostAllowed \/ (Mode = "LegacyLaunch" /\ s.host = "legacy")
            THEN Done(i, "host")
            ELSE IF s.input[2] = "none" THEN Done(i, "missing")
            ELSE IF s.disk = "ready" \/ (Mode = "NoCacheStamp" /\ s.disk = "damaged")
            THEN Done(i, "cache")
            ELSE IF i \in s.doctors /\ Mode # "DoctorInstalls" THEN
                [Done(i, "missing") EXCEPT !.sawStage = IF s.stage # {} THEN @ \cup {i} ELSE @]
            ELSE [s EXCEPT !.pc[i] = "lock", !.wrote = @ \cup {i}]
Wait(i) ==
    /\ s.pc[i] = "lock" /\ s.contenders # {} /\ i \notin s.waitingBehind
    /\ s' = [s EXCEPT !.waitingBehind = @ \cup {i}]
Lock(i) ==
    /\ s.pc[i] = "lock" /\ (s.contenders = {} \/ Mode = "NoLock")
    /\ s' = [s EXCEPT !.pc[i] = "recheck", !.contenders = @ \cup {i}]
Recheck(i) ==
    /\ s.pc[i] = "recheck"
    /\ s' = CASE s.disk = "ready" ->
                    [Done(i, "cache") EXCEPT !.rechecked = @ \cup {i}]
              [] s.disk # "empty" -> Done(i, "error")
              [] OTHER -> [s EXCEPT !.pc[i] = "stage"]
Stage(i) ==
    /\ s.pc[i] = "stage"
    /\ s' = [s EXCEPT !.stage = @ \cup {i}, !.pc[i] = "entries"]
Entries(i) ==
    /\ s.pc[i] = "entries"
    /\ s' = [s EXCEPT !.pc[i] = IF s.input[2] = "link" /\ Mode # "AcceptLink" THEN "cleanup" ELSE "checksum",
                      !.rejected = IF s.input[2] = "link" /\ Mode # "AcceptLink" THEN @ \cup {i} ELSE @,
                      !.disk = IF Mode = "EarlyPublish" THEN "partial" ELSE @]
Checksum(i) ==
    /\ s.pc[i] = "checksum"
    /\ s' = [s EXCEPT !.pc[i] = IF s.input[2] = "badStream" /\ Mode # "SkipChecksum" THEN "cleanup" ELSE "stamp",
                      !.rejected = IF s.input[2] = "badStream" /\ Mode # "SkipChecksum" THEN @ \cup {i} ELSE @]
CheckStamp(i) ==
    /\ s.pc[i] = "stamp"
    /\ s' = IF s.input[2] = "badStamp" /\ Mode # "NoStamp"
            THEN [s EXCEPT !.pc[i] = "cleanup", !.rejected = @ \cup {i}]
            ELSE [s EXCEPT !.pc[i] = "rename",
                           !.validated = IF s.input[2] = "valid" THEN @ \cup {i} ELSE @]
Publish(i) ==
    /\ s.pc[i] = "rename" /\ s.disk \in {"empty","partial"}
    /\ s' = [s EXCEPT !.pc[i] = "return", !.disk = IF i \in s.validated THEN "ready" ELSE "badPublished",
                   !.published = @ \cup {i}, !.stage = @ \ {i},
                   !.host = IF Mode = "OverwriteHost" THEN "replaced" ELSE @]
IoFailure(i) ==
    /\ s.pc[i] \in {"entries","checksum","stamp","rename"}
    /\ s' = [s EXCEPT !.pc[i] = "cleanup"]
Cleanup(i) ==
    /\ s.pc[i] = "cleanup" /\ Mode # "StuckCleanup"
    /\ s' = [s EXCEPT !.pc[i] = "failReturn", !.stage = IF Mode = "LeakStage" THEN @ ELSE @ \ {i},
                                     !.cleaned = @ \cup {i}]

Return(i) ==
    /\ s.pc[i] \in {"return", "failReturn"}
    /\ s' = Done(i, IF s.pc[i] = "return" THEN "cache" ELSE "error")

Step(i) == Inspect(i) \/ Wait(i) \/ Lock(i) \/ Recheck(i) \/ Stage(i) \/ Entries(i) \/
           Checksum(i) \/ CheckStamp(i) \/ Publish(i) \/ IoFailure(i) \/ Cleanup(i) \/ Return(i)
Idle == (\A i \in Clients : s.pc[i] = "done")
        \/ (Mode = "StuckCleanup" /\ \E i \in Clients : s.pc[i] = "cleanup")
Next == (\E i \in Clients : Step(i)) \/ (Idle /\ UNCHANGED vars)
Spec == Init /\ [][Next]_vars /\ \A i \in Clients : WF_vars(Step(i))

TypeOK == /\ s.input \in Cases /\ s.host \in HostKinds \cup {"replaced"} /\ s.disk \in DiskKinds
          /\ s.pc \in [Clients -> {"inspect","lock","recheck","stage","entries","checksum","stamp","rename","cleanup","return","failReturn","done"}]
          /\ s.result \in [Clients -> Results]
          /\ \A x \in {s.doctors,s.contenders,s.stage,s.wrote,s.validated,s.waitingBehind,s.rechecked,s.published,s.cleaned,s.rejected,s.sawStage} : x \subseteq Clients
SelectionTrusted == \A i \in Clients :
                      /\ (s.result[i] = "host" => HostAllowed)
                      /\ (s.result[i] = "cache" => s.disk = "ready")
OnlyVerifiedPublication == s.disk \notin {"partial","badPublished"} /\ s.published \subseteq s.validated
ExclusiveStaging == Cardinality(s.stage) <= 1
HostPreserved == s.host = s.input[1]
DoctorReadOnly == s.doctors \cap s.wrote = {}
Settled == \A i \in Clients : s.pc[i] = "done" => i \notin s.contenders \cup s.stage
EventuallySettled == <>(\A i \in Clients : s.pc[i] = "done")

NoConcurrentReuse == ~(s.waitingBehind \cap s.rechecked # {} /\ s.published # {})
NoLegacyUpgrade == ~(s.input[1] = "legacy" /\ s.published # {} /\ s.host = "legacy")
NoCleanInstall == ~(s.input[1] = "empty" /\ s.published # {})
NoDoctorDuringStage == s.sawStage = {}
NoLinkCleanup == ~(s.input[2] = "link" /\ s.cleaned \cap s.rejected # {} /\ s.stage = {})
NoBadStampCleanup == ~(s.input[2] = "badStamp" /\ s.cleaned \cap s.rejected # {} /\ s.stage = {})
NoWithoutArchiveHost == ~(s.input[2] = "none" /\ \E i \in Clients : s.result[i] = "host")
=============================================================================
