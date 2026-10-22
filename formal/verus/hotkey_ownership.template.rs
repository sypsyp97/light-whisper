//! Arbitrary ownership state; locking and asynchronous scheduling remain outside this proof.
use vstd::prelude::*;

verus! {

/*__START_OWNERSHIP_STRUCT__*/

impl StartOwnership {
    fn begin(&mut self) -> (result: Option<u64>)
        ensures
            result.is_some() == (old(self).generation < u64::MAX),
            result.is_some() ==> (
                result == Some(final(self).generation)
                && final(self).generation as int == old(self).generation as int + 1
                && final(self).active
                && final(self).session_id == None
            ),
            result.is_none() ==> (
                final(self).generation == old(self).generation
                && final(self).active == old(self).active
                && final(self).session_id == old(self).session_id
            ),
    {
/*__BEGIN_BODY__*/
    }

    fn bind(&mut self, generation: u64, session_id: u64) -> (result: bool)
        ensures
            result == (
                old(self).active
                && old(self).generation == generation
                && old(self).session_id.is_none()
            ),
            final(self).generation == old(self).generation,
            final(self).active == old(self).active,
            final(self).session_id == (if result { Some(session_id) } else { old(self).session_id }),
    {
/*__BIND_BODY__*/
    }

    fn release(&mut self) -> (result: Option<u64>)
        ensures
            result == old(self).session_id,
            final(self).generation == old(self).generation,
            !final(self).active,
            final(self).session_id == None,
    {
/*__RELEASE_BODY__*/
    }

    fn fail(&mut self, generation: u64) -> (result: bool)
        ensures
            result == (old(self).active && old(self).generation == generation),
            final(self).generation == old(self).generation,
            final(self).active == (if result { false } else { old(self).active }),
            final(self).session_id == (if result { None } else { old(self).session_id }),
    {
/*__FAIL_BODY__*/
    }
}

}
