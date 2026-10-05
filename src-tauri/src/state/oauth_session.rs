use parking_lot::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OAuthOperation(u64);

pub struct OAuthSessionState<T: Clone> {
    inner: Mutex<OAuthSessionInner<T>>,
    refresh: tokio::sync::Mutex<()>,
}

struct OAuthSessionInner<T> {
    generation: u64,
    session: Option<T>,
    login_pending: bool,
    login_owner: Option<OAuthOperation>,
    pending_challenge: Option<OAuthPendingChallenge>,
}

struct OAuthPendingChallenge {
    operation: OAuthOperation,
    binding: String,
}

pub struct OAuthLoginOperation<'a, T: Clone> {
    owner: &'a OAuthSessionState<T>,
    operation: OAuthOperation,
}

impl<T: Clone> Default for OAuthSessionState<T> {
    fn default() -> Self {
        Self {
            inner: Mutex::new(OAuthSessionInner {
                generation: 0,
                session: None,
                login_pending: false,
                login_owner: None,
                pending_challenge: None,
            }),
            refresh: tokio::sync::Mutex::new(()),
        }
    }
}

impl<T: Clone> OAuthSessionState<T> {
    pub fn read(&self) -> Option<T> {
        self.inner.lock().session.clone()
    }

    #[cfg(test)]
    pub fn restore(&self, session: Option<T>) {
        let mut inner = self.inner.lock();
        inner.generation = next_generation(inner.generation);
        inner.session = session;
        inner.login_pending = false;
        inner.login_owner = None;
        inner.pending_challenge = None;
    }

    pub fn begin_login(&self) -> OAuthLoginOperation<'_, T> {
        let mut inner = self.inner.lock();
        inner.generation = next_generation(inner.generation);
        let operation = OAuthOperation(inner.generation);
        inner.login_pending = true;
        inner.login_owner = Some(operation);
        inner.pending_challenge = None;
        OAuthLoginOperation {
            owner: self,
            operation,
        }
    }

    pub async fn begin_login_after_refresh(&self) -> OAuthLoginOperation<'_, T> {
        // Preserve a refresh-token rotation before a failed account switch can
        // leave the previous session active. Pending logins block later refreshes.
        let _guard = self.lock_refresh().await;
        self.begin_login()
    }

    pub fn snapshot_for_refresh(&self) -> Option<(OAuthOperation, T)> {
        let inner = self.inner.lock();
        if inner.login_pending || inner.pending_challenge.is_some() {
            return None;
        }
        inner
            .session
            .clone()
            .map(|session| (OAuthOperation(inner.generation), session))
    }

    pub fn publish_challenge(
        &self,
        operation: OAuthOperation,
        binding: impl Into<String>,
    ) -> Result<bool, String> {
        let binding = binding.into();
        if binding.is_empty() {
            return Ok(false);
        }
        let mut inner = self.inner.lock();
        if inner.generation != operation.0
            || !inner.login_pending
            || inner.login_owner != Some(operation)
        {
            return Ok(false);
        }
        inner.pending_challenge = Some(OAuthPendingChallenge { operation, binding });
        Ok(true)
    }

    pub fn claim_challenge(&self, binding: &str) -> Option<OAuthLoginOperation<'_, T>> {
        let mut inner = self.inner.lock();
        let pending = inner.pending_challenge.as_ref()?;
        if pending.binding != binding || inner.generation != pending.operation.0 {
            return None;
        }
        let operation = inner.pending_challenge.take()?.operation;
        inner.login_pending = true;
        inner.login_owner = Some(operation);
        Some(OAuthLoginOperation {
            owner: self,
            operation,
        })
    }

    pub async fn lock_refresh(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.refresh.lock().await
    }

    pub fn commit(
        &self,
        operation: OAuthOperation,
        session: T,
        persist: impl FnOnce(&T) -> Result<(), String>,
    ) -> Result<bool, String> {
        let mut inner = self.inner.lock();
        if inner.generation != operation.0 {
            return Ok(false);
        }
        persist(&session)?;
        inner.session = Some(session);
        inner.pending_challenge = None;
        inner.generation = next_generation(inner.generation);
        Ok(true)
    }

    pub fn invalidate(
        &self,
        operation: Option<OAuthOperation>,
        clear: impl FnOnce() -> Result<(), String>,
    ) -> Result<bool, String> {
        let mut inner = self.inner.lock();
        if operation.is_some_and(|operation| inner.generation != operation.0) {
            return Ok(false);
        }

        inner.generation = next_generation(inner.generation);
        inner.session = None;
        inner.login_pending = false;
        inner.login_owner = None;
        inner.pending_challenge = None;
        clear().map(|_| true)
    }

    fn finish_login(&self, operation: OAuthOperation) {
        let mut inner = self.inner.lock();
        if inner.login_owner != Some(operation) {
            return;
        }
        inner.login_pending = false;
        inner.login_owner = None;
        if inner
            .pending_challenge
            .as_ref()
            .is_some_and(|pending| pending.operation == operation)
        {
            return;
        }
        inner.generation = next_generation(inner.generation);
    }

    /// Retain an issued registration before token exchange without publishing
    /// credentials. Cancellation/removal and progress use the same lock/epoch.
    pub fn record_login_progress(
        &self,
        operation: OAuthOperation,
        persist: impl FnOnce() -> Result<(), String>,
    ) -> Result<bool, String> {
        let inner = self.inner.lock();
        if inner.generation != operation.0
            || !inner.login_pending
            || inner.login_owner != Some(operation)
        {
            return Ok(false);
        }
        persist()?;
        Ok(true)
    }

    /// Update account records atomically with login publication. Invalidate a
    /// matching session, preserve other accounts, and cancel pending logins.
    pub fn invalidate_matching(
        &self,
        matches: impl FnOnce(&T) -> bool,
        update: impl FnOnce(bool) -> Result<(), String>,
    ) -> Result<Option<T>, String> {
        let mut inner = self.inner.lock();
        let matched = inner.session.as_ref().is_some_and(matches);
        let removed = if matched { inner.session.take() } else { None };
        inner.generation = next_generation(inner.generation);
        inner.login_pending = false;
        inner.login_owner = None;
        inner.pending_challenge = None;
        update(matched)?;
        Ok(removed)
    }
}

fn next_generation(generation: u64) -> u64 {
    generation
        .checked_add(1)
        .expect("OAuth session generation exhausted")
}

impl<T: Clone> OAuthLoginOperation<'_, T> {
    pub fn token(&self) -> OAuthOperation {
        self.operation
    }
}

impl<T: Clone> Drop for OAuthLoginOperation<'_, T> {
    fn drop(&mut self) {
        self.owner.finish_login(self.operation);
    }
}

#[cfg(test)]
#[path = "oauth_session_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "oauth_session_challenge_tests.rs"]
mod challenge_tests;
