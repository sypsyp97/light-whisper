------------------------ MODULE CodexCatalogVersion ------------------------
EXTENDS Naturals
CONSTANT Fault
VARIABLES cached, age, pending, owner, staleRead, floorSeen, failedFresh
vars == <<cached, age, pending, owner, staleRead, floorSeen, failedFresh>>
Readers == {1, 2}
TTL == 2
Init == /\ cached = 1 /\ age = TTL /\ pending = {} /\ owner = 0
        /\ staleRead = FALSE /\ floorSeen = 1 /\ failedFresh = FALSE
Ask(i) == /\ i \notin pending
          /\ pending' = pending \cup {i}
          /\ UNCHANGED <<cached, age, owner, staleRead, floorSeen, failedFresh>>
Serve(i) == /\ i \in pending /\ owner = 0 /\ age < TTL
            /\ pending' = pending \ {i}
            /\ UNCHANGED <<cached, age, owner, staleRead, floorSeen, failedFresh>>
Acquire(i) == /\ i \in pending /\ owner = 0
              /\ (age = TTL \/ Fault = "bypass")
              /\ owner' = i /\ staleRead' = (age < TTL)
              /\ UNCHANGED <<cached, age, pending, floorSeen, failedFresh>>
\* r = 0 abstracts transport, HTTP, JSON and validation failures.
Finish(r) == /\ owner # 0
             /\ cached' = IF r >= cached THEN r
                           ELSE IF Fault = "downgrade" THEN 1 ELSE cached
             /\ floorSeen' = IF r > floorSeen THEN r ELSE floorSeen
             /\ age' = IF r = 0 /\ Fault = "noFailureTTL" THEN TTL ELSE 0
             /\ failedFresh' = (r = 0 /\ age' = TTL)
             /\ pending' = pending \ {owner} /\ owner' = 0
             /\ UNCHANGED staleRead
Tick == /\ age < TTL /\ age' = age + 1
        /\ UNCHANGED <<cached, pending, owner, staleRead, floorSeen, failedFresh>>
Next == (\E i \in Readers : Ask(i) \/ Serve(i) \/ Acquire(i))
        \/ (\E r \in 0..3 : Finish(r)) \/ Tick
Spec == Init /\ [][Next]_vars
Monotone == cached = floorSeen
ReuseBeforeExpiry == ~staleRead
ThrottleFailures == ~failedFresh
=============================================================================
