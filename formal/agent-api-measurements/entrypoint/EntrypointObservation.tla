--------------------- MODULE EntrypointObservation ---------------------
EXTENDS TLC
CONSTANT Variant
VARIABLES input, identityValid, phase, route, observation, launched, overlay
vars == <<input, identityValid, phase, route, observation, launched, overlay>>

(* argv[0] is the owned executable, unequal to either agent flag.
   Only explicit empty data is modeled; absent data is never launched.
   Desktop uses a nonempty owned directory. No scheduler/process model here. *)
Init == /\ input \in {"client-empty", "desktop", "minimized"}
        /\ identityValid \in BOOLEAN
        /\ phase = "new" /\ route = "none" /\ observation = "none"
        /\ launched = FALSE /\ overlay = FALSE

Admit == /\ phase = "new"
         /\ phase' = IF identityValid THEN "admitted" ELSE "refused"
         /\ UNCHANGED <<input, identityValid, route, observation, launched, overlay>>

Overlay == /\ phase = "admitted"
           /\ phase' = "prepared" /\ overlay' = TRUE
           /\ UNCHANGED <<input, identityValid, route, observation, launched>>

(* lib.rs:16 guard; lib.rs:22 client-first return; lib.rs:29 host decision.
   any(arg != host) is TRUE because argv[0] is an executable path. *)
Dispatch == /\ phase = "prepared"
            /\ launched' = TRUE /\ phase' = "routed"
            /\ route' = IF input = "client-empty"
                         THEN IF Variant = "initial-host-inverse" THEN "exit2" ELSE "client"
                         ELSE IF Variant = "final-host-inverse" THEN "host" ELSE "gui-boundary"
            /\ UNCHANGED <<input, identityValid, observation, overlay>>

Observe == /\ phase = "routed"
           /\ phase' = "observed"
           /\ observation' = CASE route = "client" -> "client-ready-local-exit"
                                  [] route = "host" -> "windowless-host-ready"
                                  [] route = "exit2" -> "exit2"
                                  [] OTHER -> "test-marker-exit86"
           /\ UNCHANGED <<input, identityValid, route, launched, overlay>>

Restore == /\ phase = "observed" /\ phase' = "restored" /\ overlay' = FALSE
           /\ UNCHANGED <<input, identityValid, route, observation, launched>>
Terminal == /\ phase \in {"restored", "refused"} /\ UNCHANGED vars
Next == Admit \/ Overlay \/ Dispatch \/ Observe \/ Restore \/ Terminal
Spec == Init /\ [][Next]_vars

IdentityGate == launched => identityValid
BeforeGui == (route = "gui-boundary" /\ phase \in {"routed", "observed"}) => overlay
NoAbsentCase == input \in {"client-empty", "desktop", "minimized"}
Restored == phase = "restored" => ~overlay
ExpectedObservation == phase \in {"observed", "restored"} =>
    observation = IF input = "client-empty"
                  THEN "client-ready-local-exit" ELSE "test-marker-exit86"
=============================================================================
