--------------------------- MODULE CatalogBodyRetry ---------------------------
EXTENDS Naturals, Sequences

Outcomes == {"send", "http", "payload", "timeout", "interrupted", "eof", "invalid"}
Incomplete == {"timeout", "interrupted", "eof"}

VARIABLES enabled, cloneable, attempts, done
vars == <<enabled, cloneable, attempts, done>>

Init == /\ enabled \in BOOLEAN
        /\ cloneable \in BOOLEAN
        /\ attempts = <<>>
        /\ done = FALSE

\* Each read returns a send error, intact HTTP error response, payload,
\* transport/body timeout, interrupted body, EOF JSON, or invalid JSON.
Read == /\ ~done
        /\ \E outcome \in Outcomes :
              /\ attempts' = Append(attempts, outcome)
              /\ done' = ~(enabled /\ cloneable /\ Len(attempts) = 0 /\ outcome \in Incomplete)
        /\ UNCHANGED <<enabled, cloneable>>

Spec == Init /\ [][Read]_vars /\ WF_vars(Read)

Bounded == Len(attempts) <= 2
OnlyEligible == Len(attempts) >= 2 =>
                    enabled /\ cloneable /\ attempts[1] \in Incomplete
RecoverableGetsRetry == (done /\ Len(attempts) = 1) =>
                           ~(enabled /\ cloneable /\ attempts[1] \in Incomplete)
Finishes == <>done
=============================================================================
