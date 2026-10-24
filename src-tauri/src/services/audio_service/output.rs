use std::collections::HashMap;

use tokio::sync::oneshot;

/// A completed dictation keeps the target captured when its recording began.
#[derive(Clone)]
pub(crate) struct PendingPaste {
    pub session_id: u64,
    pub target_window: Option<isize>,
    pub text: String,
}

impl PendingPaste {
    pub fn target_matches(&self, current_window: Option<isize>) -> bool {
        self.target_window.is_some() && self.target_window == current_window
    }
}

#[derive(Default)]
pub(crate) struct PendingPasteQueue {
    pending: Vec<PendingPaste>,
    completions: HashMap<u64, (tokio::time::Instant, oneshot::Sender<Option<u64>>)>,
}

impl PendingPasteQueue {
    pub fn push_with_completion(
        &mut self,
        output: PendingPaste,
        started_at: std::time::Instant,
    ) -> oneshot::Receiver<Option<u64>> {
        let (completed, receipt) = oneshot::channel();
        if self.completions.contains_key(&output.session_id)
            || self
                .pending
                .iter()
                .any(|item| item.session_id == output.session_id)
        {
            let _ = completed.send(None);
            return receipt;
        }
        self.completions.insert(
            output.session_id,
            (tokio::time::Instant::from_std(started_at), completed),
        );
        self.push(output);
        receipt
    }

    pub fn finish_output(&mut self, session_id: u64, pasted: bool) {
        if let Some((started_at, completed)) = self.completions.remove(&session_id) {
            let total_ms =
                pasted.then(|| started_at.elapsed().as_millis().min(u64::MAX as u128) as u64);
            let _ = completed.send(total_ms);
        }
    }

    pub fn push(&mut self, output: PendingPaste) {
        if !self
            .pending
            .iter()
            .any(|item| item.session_id == output.session_id)
        {
            self.pending.push(output);
        }
    }

    pub fn take_if_idle(&mut self, recording: bool) -> Vec<PendingPaste> {
        if recording {
            Vec::new()
        } else {
            std::mem::take(&mut self.pending)
        }
    }
}

pub(crate) async fn settle_history_output<P, R, U, UF>(
    persist: P,
    receipt: Option<oneshot::Receiver<Option<u64>>>,
    publish: R,
    update: U,
) -> Result<(), String>
where
    P: std::future::Future<Output = Result<Option<i64>, String>>,
    R: FnOnce(u64),
    U: FnOnce(i64, u64) -> UF,
    UF: std::future::Future<Output = Result<bool, String>>,
{
    let output = async {
        let total_ms = match receipt {
            Some(receipt) => receipt.await.ok().flatten(),
            None => None,
        };
        if let Some(total_ms) = total_ms {
            publish(total_ms);
        }
        total_ms
    };
    let (inserted, total_ms) = tokio::join!(persist, output);
    if let (Some(id), Some(total_ms)) = (inserted?, total_ms) {
        update(id, total_ms).await?;
    }
    Ok(())
}
