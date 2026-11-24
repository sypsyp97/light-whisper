-------------------------- MODULE NativeCliSession --------------------------
EXTENDS Naturals, Sequences
CONSTANT Mode
VARIABLES pc, owner, records, cursor, launches, wire, failedAt, terminal
v == <<pc, owner, records, cursor, launches, wire, failedAt, terminal>>
Kinds == {"op", "invalid", "exit"}
Init ==
    /\ pc = "read" /\ owner = FALSE
    /\ records \in [1..2 -> Kinds] /\ cursor = 1
    /\ launches = 0 /\ wire = <<>> /\ failedAt = 0 /\ terminal = FALSE

Read ==
    /\ pc = "read"
    /\ IF cursor > 2 THEN /\ pc' = "close" /\ UNCHANGED cursor
       ELSE CASE records[cursor] = "exit" -> /\ pc' = "close" /\ UNCHANGED cursor
              [] records[cursor] = "invalid" ->
                   /\ pc' = IF Mode = "SendInvalid" THEN "issue" ELSE "read"
                   /\ cursor' = IF Mode = "SendInvalid" THEN cursor ELSE cursor + 1
              [] OTHER -> /\ pc' = (IF owner THEN "issue" ELSE "open") /\ UNCHANGED cursor
    /\ UNCHANGED <<owner, records, launches, wire, failedAt, terminal>>

Acquire ==
    /\ pc = "open" /\ pc' = "startup" /\ owner' = TRUE /\ launches' = launches + 1
    /\ UNCHANGED <<records, cursor, wire, failedAt, terminal>>

Ready ==
    /\ pc = "startup" /\ pc' = "issue"
    /\ UNCHANGED <<owner, records, cursor, launches, wire, failedAt, terminal>>

Issue ==
    /\ pc = "issue" /\ wire' = Append(wire, cursor) /\ pc' = "wait"
    /\ UNCHANGED <<owner, records, cursor, launches, failedAt, terminal>>

OrdinaryReply ==
    /\ pc = "wait" /\ cursor' = cursor + 1 /\ pc' = "read"
    /\ UNCHANGED <<owner, records, launches, wire, failedAt, terminal>>

StartupOrTransportFailure ==
    /\ pc \in {"startup", "wait"} /\ failedAt = 0 /\ failedAt' = cursor
    /\ pc' = IF Mode = "Retry" /\ pc = "wait" THEN "issue" ELSE "close"
    /\ UNCHANGED <<owner, records, cursor, launches, wire, terminal>>

Close ==
    /\ pc = "close" /\ Mode # "Hang"
    /\ pc' = "publish" /\ owner' = IF Mode = "ForgetChild" THEN owner ELSE FALSE
    /\ UNCHANGED <<records, cursor, launches, wire, failedAt, terminal>>

Publish ==
    /\ pc = "publish" /\ pc' = "done" /\ terminal' = TRUE
    /\ UNCHANGED <<owner, records, cursor, launches, wire, failedAt>>

Advance == Read \/ Acquire \/ Ready \/ Issue \/ OrdinaryReply \/
           StartupOrTransportFailure \/ Close \/ Publish
Stutter == pc = "done" \/ (pc = "close" /\ Mode = "Hang")
Spec == Init /\ [][Advance \/ (Stutter /\ UNCHANGED v)]_v /\ WF_v(Advance)

TypeOK == /\ pc \in {"read", "open", "startup", "issue", "wait", "close", "publish", "done"}
          /\ owner \in BOOLEAN /\ cursor \in 1..3 /\ records \in [1..2 -> Kinds]
          /\ launches \in 0..1 /\ wire \in Seq(1..2) /\ failedAt \in 0..2 /\ terminal \in BOOLEAN
OnlyValid == \A i \in 1..Len(wire) : records[wire[i]] = "op"
NoRetry == \A i,j \in 1..Len(wire) : i # j => wire[i] # wire[j]
Retired == terminal => ~owner
EventuallyDone == <>terminal
NotDoubleUse == ~(terminal /\ launches = 1 /\ Len(wire) = 2)
NotFailureRetired == ~(terminal /\ failedAt # 0 /\ launches = 1 /\ ~owner)
=============================================================================
