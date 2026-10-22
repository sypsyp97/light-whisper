use parking_lot::Mutex;

#[derive(Default)]
pub(crate) struct HotkeyStartOwner {
    inner: Mutex<StartOwnership>,
}

#[derive(Default)]
struct StartOwnership {
    generation: u64,
    active: bool,
    session_id: Option<u64>,
}

impl StartOwnership {
    fn begin(&mut self) -> Option<u64> {
        if let Some(generation) = self.generation.checked_add(1) {
            self.generation = generation;
            self.active = true;
            self.session_id = None;
            Some(generation)
        } else {
            None
        }
    }

    fn bind(&mut self, generation: u64, session_id: u64) -> bool {
        if !self.active || self.generation != generation || self.session_id.is_some() {
            return false;
        }
        self.session_id = Some(session_id);
        true
    }

    fn release(&mut self) -> Option<u64> {
        self.active = false;
        self.session_id.take()
    }

    fn fail(&mut self, generation: u64) -> bool {
        if !self.active || self.generation != generation {
            return false;
        }
        self.active = false;
        self.session_id = None;
        true
    }
}

impl HotkeyStartOwner {
    pub(crate) fn begin(&self) -> u64 {
        let mut inner = self.inner.lock();
        inner.begin().expect("hotkey generation exhausted")
    }

    pub(crate) fn bind(&self, generation: u64, session_id: u64) -> bool {
        let mut inner = self.inner.lock();
        inner.bind(generation, session_id)
    }

    pub(crate) fn release(&self) -> Option<u64> {
        let mut inner = self.inner.lock();
        inner.release()
    }

    pub(crate) fn fail(&self, generation: u64) -> bool {
        let mut inner = self.inner.lock();
        inner.fail(generation)
    }
}

#[cfg(test)]
mod tests {
    use super::{HotkeyStartOwner, StartOwnership};

    fn owner_with_state(
        generation: u64,
        active: bool,
        session_id: Option<u64>,
    ) -> HotkeyStartOwner {
        HotkeyStartOwner {
            inner: parking_lot::Mutex::new(StartOwnership {
                generation,
                active,
                session_id,
            }),
        }
    }

    fn snapshot(owner: &HotkeyStartOwner) -> (u64, bool, Option<u64>) {
        let inner = owner.inner.lock();
        (inner.generation, inner.active, inner.session_id)
    }

    #[test]
    fn begin_advances_once_and_clears_any_prior_binding() {
        for generation in [0, 1, u64::MAX - 1] {
            for active in [false, true] {
                for session_id in [None, Some(0), Some(u64::MAX)] {
                    let owner = owner_with_state(generation, active, session_id);

                    assert_eq!(owner.begin(), generation + 1);
                    assert_eq!(snapshot(&owner), (generation + 1, true, None));
                }
            }
        }
    }

    #[test]
    fn exhausted_begin_panics_without_changing_any_state() {
        for active in [false, true] {
            for session_id in [None, Some(0), Some(u64::MAX)] {
                let owner = owner_with_state(u64::MAX, active, session_id);
                let before = snapshot(&owner);

                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| owner.begin()));

                assert!(result.is_err(), "exhausted generation must not wrap");
                assert_eq!(snapshot(&owner), before);
            }
        }
    }

    #[test]
    fn bind_accepts_only_an_active_matching_unbound_generation() {
        for generation in [0, 1, u64::MAX] {
            for requested in [0, 1, 2, u64::MAX - 1, u64::MAX] {
                for active in [false, true] {
                    for session_id in [None, Some(0), Some(u64::MAX)] {
                        for incoming in [0, 23, u64::MAX] {
                            let owner = owner_with_state(generation, active, session_id);
                            let before = snapshot(&owner);
                            let accepted =
                                active && requested == generation && session_id.is_none();

                            assert_eq!(
                                owner.bind(requested, incoming),
                                accepted,
                                "{before:?}, requested={requested}, incoming={incoming}"
                            );
                            assert_eq!(
                                snapshot(&owner),
                                if accepted {
                                    (generation, true, Some(incoming))
                                } else {
                                    before
                                },
                                "{before:?}, requested={requested}, incoming={incoming}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn release_takes_binding_once_and_preserves_generation() {
        for generation in [0, 1, u64::MAX] {
            for active in [false, true] {
                for session_id in [None, Some(0), Some(u64::MAX)] {
                    let owner = owner_with_state(generation, active, session_id);

                    assert_eq!(owner.release(), session_id);
                    assert_eq!(snapshot(&owner), (generation, false, None));
                    assert_eq!(owner.release(), None);
                    assert_eq!(snapshot(&owner), (generation, false, None));
                    assert!(!owner.bind(generation, 23));
                    assert_eq!(snapshot(&owner), (generation, false, None));
                }
            }
        }
    }

    #[test]
    fn fail_accepts_only_an_active_matching_generation_and_preserves_rejections() {
        for generation in [0, 1, u64::MAX] {
            for requested in [0, 1, 2, u64::MAX - 1, u64::MAX] {
                for active in [false, true] {
                    for session_id in [None, Some(0), Some(u64::MAX)] {
                        let owner = owner_with_state(generation, active, session_id);
                        let before = snapshot(&owner);
                        let accepted = active && requested == generation;
                        let expected = if accepted {
                            (generation, false, None)
                        } else {
                            before
                        };

                        assert_eq!(
                            owner.fail(requested),
                            accepted,
                            "{before:?}, requested={requested}"
                        );
                        assert_eq!(snapshot(&owner), expected);
                        assert!(!owner.fail(requested));
                        assert_eq!(snapshot(&owner), expected);
                    }
                }
            }
        }
    }

    #[test]
    fn release_before_async_start_rejects_late_bind() {
        let owner = HotkeyStartOwner::default();
        let generation = owner.begin();

        assert_eq!(owner.release(), None);
        assert!(!owner.bind(generation, 41));
        assert_eq!(owner.release(), None);
    }

    #[test]
    fn old_generation_cannot_bind_after_a_new_begin() {
        let owner = HotkeyStartOwner::default();
        let old_generation = owner.begin();
        let new_generation = owner.begin();

        assert!(!owner.bind(old_generation, 101));
        assert!(owner.bind(new_generation, 202));
        assert_eq!(owner.release(), Some(202));
    }

    #[test]
    fn old_generation_failure_cannot_cancel_new_generation() {
        let owner = HotkeyStartOwner::default();
        let old_generation = owner.begin();
        let new_generation = owner.begin();

        assert!(!owner.fail(old_generation));
        assert!(owner.bind(new_generation, 202));
        assert_eq!(owner.release(), Some(202));
    }

    #[test]
    fn duplicate_bind_does_not_replace_the_owned_session() {
        let owner = HotkeyStartOwner::default();
        let generation = owner.begin();

        assert!(owner.bind(generation, 101));
        assert!(!owner.bind(generation, 202));
        assert_eq!(owner.release(), Some(101));
    }

    #[test]
    fn registration_gates_have_independent_session_ownership() {
        let first_gate = HotkeyStartOwner::default();
        let second_gate = HotkeyStartOwner::default();
        let first_generation = first_gate.begin();
        let second_generation = second_gate.begin();

        assert!(first_gate.bind(first_generation, 101));
        assert!(second_gate.bind(second_generation, 202));
        assert_eq!(first_gate.release(), Some(101));
        assert_eq!(second_gate.release(), Some(202));
    }
}
