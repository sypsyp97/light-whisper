----------------------------- MODULE PrivateFiles -----------------------------
EXTENDS Naturals, FiniteSets

CONSTANTS PublicTemp, PreserveOldAcl, ShareDelete, LeakTemp, DeleteNeighbor,
          StallCleanup, LateTighten, DirectDestinationWrite, CopyInsteadOfMove,
          DestinationInitiallyExists, ForgetUnvalidatedOwnership
Hosts == {1, 2}
VARIABLES phase, tempExists, tempPrivate, tempSecret, destinationPrivate,
          destinationSecret, outcome, neighborExists, locks, seen, pathGeneration,
          blockedRename, destinationExists, destinationIdentity
vars == <<phase, tempExists, tempPrivate, tempSecret, destinationPrivate,
          destinationSecret, outcome, neighborExists, locks, seen, pathGeneration,
          blockedRename, destinationExists, destinationIdentity>>

Init == /\ phase = "idle"
        /\ tempExists = FALSE /\ tempPrivate = FALSE /\ tempSecret = FALSE
        /\ destinationPrivate = FALSE /\ destinationSecret = FALSE
        /\ destinationExists = DestinationInitiallyExists
        /\ destinationIdentity = IF DestinationInitiallyExists THEN 1 ELSE 0
        /\ outcome = "old" /\ neighborExists = TRUE
        /\ locks = [h \in Hosts |-> 0] /\ seen = {}
        /\ pathGeneration = 1 /\ blockedRename = FALSE

CreateTemp == /\ phase = "idle"
              /\ phase' = "new-unvalidated" /\ tempExists' = TRUE
              /\ tempPrivate' = ~PublicTemp
              /\ UNCHANGED <<tempSecret, destinationPrivate, destinationSecret,
                  outcome, neighborExists, locks, seen, pathGeneration, blockedRename,
                  destinationExists, destinationIdentity>>

ValidateCreated == /\ phase = "new-unvalidated" /\ phase' = "created"
                   /\ UNCHANGED <<tempExists, tempPrivate, tempSecret,
                       destinationPrivate, destinationSecret, outcome, neighborExists,
                       locks, seen, pathGeneration, blockedRename,
                       destinationExists, destinationIdentity>>

ValidationFails == /\ phase = "new-unvalidated"
                   /\ phase' = "creation-failed" /\ outcome' = "creation-failed"
                   /\ UNCHANGED <<tempExists, tempPrivate, tempSecret,
                       destinationPrivate, destinationSecret, neighborExists,
                       locks, seen, pathGeneration, blockedRename,
                       destinationExists, destinationIdentity>>

CleanupCreationFailure == /\ phase = "creation-failed"
                          /\ phase' = "done"
                          /\ tempExists' = ForgetUnvalidatedOwnership
                          /\ UNCHANGED <<tempPrivate, tempSecret, destinationPrivate,
                              destinationSecret, outcome, neighborExists, locks,
                              seen, pathGeneration, blockedRename,
                              destinationExists, destinationIdentity>>

BroadenBeforeWrite == /\ phase = "created" /\ LateTighten
                     /\ phase' = "broad-before-write" /\ tempPrivate' = FALSE
                     /\ UNCHANGED <<tempExists, tempSecret, destinationPrivate,
                         destinationSecret, outcome, neighborExists, locks, seen,
                         pathGeneration, blockedRename, destinationExists, destinationIdentity>>

WriteSecret == /\ phase = (IF LateTighten THEN "broad-before-write" ELSE "created")
               /\ phase' = IF LateTighten THEN "exposed" ELSE "written"
               /\ tempSecret' = TRUE
               /\ destinationExists' = IF DirectDestinationWrite THEN TRUE ELSE destinationExists
               /\ destinationSecret' = DirectDestinationWrite
               /\ destinationPrivate' = IF DirectDestinationWrite THEN tempPrivate ELSE destinationPrivate
               /\ UNCHANGED <<tempExists, tempPrivate, outcome, neighborExists, locks,
                   seen, pathGeneration, blockedRename, destinationIdentity>>

TightenAfterWrite == /\ phase = "exposed" /\ LateTighten
                    /\ phase' = "written" /\ tempPrivate' = TRUE
                    /\ UNCHANGED <<tempExists, tempSecret, destinationPrivate,
                        destinationSecret, outcome, neighborExists, locks, seen,
                        pathGeneration, blockedRename, destinationExists, destinationIdentity>>

Publish == /\ phase = "written"
           /\ phase' = "done" /\ tempExists' = FALSE
           /\ destinationPrivate' = IF PreserveOldAcl THEN destinationPrivate
                                     ELSE tempPrivate
           /\ destinationSecret' = TRUE /\ outcome' = "published"
           /\ destinationExists' = TRUE
           /\ destinationIdentity' = IF CopyInsteadOfMove THEN 3 ELSE 2
           /\ UNCHANGED <<tempPrivate, tempSecret, neighborExists, locks, seen,
               pathGeneration, blockedRename>>

PublishFails == /\ phase = "written"
                /\ phase' = "failed" /\ outcome' = "failed"
                /\ UNCHANGED <<tempExists, tempPrivate, tempSecret,
                    destinationPrivate, destinationSecret, neighborExists,
                    locks, seen, pathGeneration, blockedRename,
                    destinationExists, destinationIdentity>>

Cleanup == /\ phase = "failed" /\ ~StallCleanup
           /\ phase' = "done" /\ tempExists' = LeakTemp
           /\ neighborExists' = ~DeleteNeighbor
           /\ UNCHANGED <<tempPrivate, tempSecret, destinationPrivate,
               destinationSecret, outcome, locks, seen, pathGeneration, blockedRename,
               destinationExists, destinationIdentity>>

Acquire(h) == /\ h \notin seen
              /\ \A other \in Hosts: locks[other] # pathGeneration
              /\ locks' = [locks EXCEPT ![h] = pathGeneration]
              /\ seen' = seen \cup {h}
              /\ UNCHANGED <<phase, tempExists, tempPrivate, tempSecret,
                  destinationPrivate, destinationSecret, outcome, neighborExists,
                  pathGeneration, blockedRename, destinationExists, destinationIdentity>>

Release(h) == /\ locks[h] # 0
              /\ locks' = [locks EXCEPT ![h] = 0]
              /\ UNCHANGED <<phase, tempExists, tempPrivate, tempSecret,
                  destinationPrivate, destinationSecret, outcome, neighborExists,
                  seen, pathGeneration, blockedRename, destinationExists, destinationIdentity>>

ReplaceLockPath == /\ pathGeneration = 1
                   /\ ShareDelete \/ (\A h \in Hosts: locks[h] = 0)
                   /\ pathGeneration' = 2
                   /\ UNCHANGED <<phase, tempExists, tempPrivate, tempSecret,
                       destinationPrivate, destinationSecret, outcome,
                       neighborExists, locks, seen, blockedRename,
                       destinationExists, destinationIdentity>>

RejectLockPathReplacement == /\ ~ShareDelete /\ ~blockedRename
                             /\ \E h \in Hosts: locks[h] # 0
                             /\ blockedRename' = TRUE
                             /\ UNCHANGED <<phase, tempExists, tempPrivate,
                                 tempSecret, destinationPrivate, destinationSecret,
                                 outcome, neighborExists, locks, seen, pathGeneration,
                                 destinationExists, destinationIdentity>>

Advance == CreateTemp \/ ValidateCreated \/ ValidationFails \/ CleanupCreationFailure
           \/ BroadenBeforeWrite \/ WriteSecret \/ TightenAfterWrite \/ Publish \/ PublishFails \/ Cleanup
Terminal == /\ phase = "done" /\ UNCHANGED vars
MissingCleanupLoop == /\ phase = "failed" /\ StallCleanup /\ UNCHANGED vars
Next == Advance \/ (\E h \in Hosts: Acquire(h) \/ Release(h))
        \/ ReplaceLockPath \/ RejectLockPathReplacement \/ Terminal
        \/ MissingCleanupLoop
Spec == Init /\ [][Next]_vars /\ WF_vars(Advance)

TypeOK == /\ phase \in {"idle", "new-unvalidated", "created", "creation-failed",
                       "broad-before-write", "exposed", "written", "failed", "done"}
          /\ outcome \in {"old", "published", "failed", "creation-failed"}
          /\ <<tempExists, tempPrivate, tempSecret, destinationPrivate,
               destinationSecret, neighborExists, blockedRename>> \in BOOLEAN \X
               BOOLEAN \X BOOLEAN \X BOOLEAN \X BOOLEAN \X BOOLEAN \X BOOLEAN
          /\ locks \in [Hosts -> {0, 1, 2}] /\ seen \subseteq Hosts
          /\ pathGeneration \in {1, 2}
          /\ destinationExists \in BOOLEAN /\ destinationIdentity \in {0, 1, 2, 3}
PrivateBeforeSecret == tempExists /\ tempSecret => tempPrivate
PublishedPrivate == destinationSecret => destinationPrivate
UnpublishedDestinationUnchanged == outcome # "published" =>
    /\ destinationExists = DestinationInitiallyExists
    /\ destinationIdentity = (IF DestinationInitiallyExists THEN 1 ELSE 0)
    /\ ~destinationSecret
PublishedMovesStaging == outcome = "published" => destinationIdentity = 2
SingleHost == Cardinality({h \in Hosts: locks[h] # 0}) <= 1
FailureCleansTemp == phase = "done" /\ outcome = "failed" => ~tempExists
CreationFailureCleansEmpty == phase = "done" /\ outcome = "creation-failed" => ~tempExists
CleanupOwnsOnlyTemp == neighborExists
PublicationSettles == <>(phase = "done")

NotWritten == ~(phase = "written" /\ tempPrivate /\ tempSecret)
NotPublished == ~(outcome = "published" /\ destinationPrivate /\ destinationSecret)
NotCleaned == ~(phase = "done" /\ outcome = "failed" /\ ~tempExists)
NotLockProtected == ~blockedRename
NotSuccessor == ~(seen = Hosts /\ Cardinality({h \in Hosts: locks[h] # 0}) = 1)
NotLateTightenedPublication == ~(LateTighten /\ outcome = "published" /\ destinationPrivate)
NotCreationFailureCleaned == ~(phase = "done" /\ outcome = "creation-failed" /\ ~tempExists)
=============================================================================
