mod worker_lifecycle {
    use super::super::{run_download_worker, wait_download_exit};
    use crate::state::AppState;
    use crate::utils::AppError;
    use serde_json::Value;
    use std::cell::{Cell, RefCell};
    use std::future::Future;
    use std::process::Stdio;
    use std::task::{Context, Waker};
    use std::time::Duration;
    use tokio::io::AsyncReadExt;
    use tokio::process::Command;
    use tokio::sync::{oneshot, watch};
    use tokio::time::timeout;

    const LIMIT: Duration = Duration::from_secs(3);

    fn fake_command(script: &str) -> Command {
        let python =
            std::env::var_os("LIGHT_WHISPER_TEST_PYTHON").unwrap_or_else(|| "python".into());
        let mut command = Command::new(python);
        command
            .args(["-u", "-c", script])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        command
    }

    #[derive(Debug)]
    struct Observation {
        payload: Value,
        slot_id: Option<u64>,
        done_notified: bool,
    }

    #[derive(Default)]
    struct Trace {
        events: RefCell<Vec<Observation>>,
        task_id: Cell<Option<u64>>,
        done: RefCell<Option<watch::Receiver<bool>>>,
    }

    impl Trace {
        fn record(&self, state: &AppState, payload: Value) {
            let slot = state
                .engine
                .download_task
                .try_lock()
                .expect("emit outside slot lock");
            let mut done = self.done.borrow_mut();
            if let Some(task) = slot.as_ref() {
                if self.task_id.get().is_none() {
                    self.task_id.set(Some(task.id));
                    *done = Some(task.done.clone());
                }
            }
            let done_notified = done
                .as_ref()
                .is_some_and(|receiver| *receiver.borrow() || receiver.has_changed().is_err());
            self.events.borrow_mut().push(Observation {
                payload,
                slot_id: slot.as_ref().map(|task| task.id),
                done_notified,
            });
        }

        async fn assert_finished(&self, state: &AppState, owner: &str, terminal: &str) {
            assert!(
                state.engine.download_task.lock().await.is_none(),
                "finished worker releases its slot"
            );
            let events = self.events.borrow();
            let terminals: Vec<_> = events
                .iter()
                .filter(|event| {
                    matches!(
                        event.payload["status"].as_str(),
                        Some("completed" | "error" | "cancelled")
                    )
                })
                .collect();
            assert_eq!(terminals.len(), 1, "exactly one terminal event: {events:?}");
            let mut violations = Vec::new();
            if events
                .iter()
                .any(|event| event.payload["requestId"].as_str() != Some(owner))
            {
                violations.push("every event must carry the captured requestId");
            }
            if terminals[0].payload["status"] != terminal {
                violations.push("terminal status must describe the worker outcome");
            }
            if self.task_id.get().is_none() || terminals[0].slot_id != self.task_id.get() {
                violations
                    .push("terminal publication must precede release of the worker's own slot");
            }
            if terminals[0].done_notified {
                violations.push("done must remain pending while terminal publication runs");
            }
            assert!(violations.is_empty(), "{violations:?}: {events:?}");
            let done = self.done.borrow();
            let done = done
                .as_ref()
                .expect("observe the worker completion channel");
            assert!(
                *done.borrow() || done.has_changed().is_err(),
                "worker return must finish all cancellation waiters"
            );
        }
    }

    async fn signal_cancel(state: &AppState) -> watch::Receiver<bool> {
        let mut slot = state.engine.download_task.lock().await;
        let task = slot
            .as_mut()
            .expect("pending worker must reserve a visible request slot");
        let done = task.done.clone();
        task.cancel
            .take()
            .expect("worker is cancellable")
            .send(())
            .expect("worker owns receiver");
        done
    }

    #[tokio::test]
    async fn started_preparation_keeps_ownership_until_settled_then_cancels_without_spawning() {
        let state = AppState::default();
        let trace = Trace::default();
        let preparation_owner = tokio::sync::Mutex::new(());
        let preparation_finished = Cell::new(false);
        let (started_tx, started_rx) = oneshot::channel();
        let (prepare_tx, prepare_rx) = oneshot::channel::<Command>();
        let preparation = async {
            let _guard = preparation_owner.lock().await;
            started_tx
                .send(())
                .expect("announce actual preparation start");
            let command = prepare_rx
                .await
                .map_err(|_| AppError::Other("test preparation closed".into()))?;
            preparation_finished.set(true);
            Ok(command)
        };
        let mut worker = Box::pin(run_download_worker(
            &state,
            Some("prepare-owner".into()),
            preparation,
            |payload| trace.record(&state, payload),
        ));
        timeout(LIMIT, async {
            tokio::select! {
                result = worker.as_mut() => panic!("worker finished before preparation started: {result:?}"),
                started = started_rx => started.expect("preparation is running"),
            }
        }).await.expect("preparation must start");
        let original_id = state.engine.download_task.lock().await.as_ref().unwrap().id;
        let done = signal_cancel(&state).await;
        let mut context = Context::from_waker(Waker::noop());

        assert!(
            worker.as_mut().poll(&mut context).is_pending(),
            "started preparation must settle before cancellation is acknowledged"
        );
        assert!(
            preparation_owner.try_lock().is_err(),
            "cancellation must retain the actual in-flight preparation guard"
        );
        assert!(!preparation_finished.get());
        assert!(!*done.borrow());
        assert!(done.has_changed().is_ok());
        assert_eq!(
            state
                .engine
                .download_task
                .lock()
                .await
                .as_ref()
                .map(|task| task.id),
            Some(original_id)
        );
        assert!(trace.events.borrow().is_empty());

        // An empty executable can never be spawned successfully. Cancellation
        // must wait for preparation, then discard its command without spawning.
        let mut forbidden_command = Command::new("");
        forbidden_command.kill_on_drop(true);
        prepare_tx
            .send(forbidden_command)
            .expect("preparation receiver must stay alive");

        let result = timeout(LIMIT, worker.as_mut())
            .await
            .expect("settled preparation must finish the pending cancellation");

        assert!(result.is_ok());
        assert!(preparation_finished.get());
        assert!(preparation_owner.try_lock().is_ok());
        trace
            .assert_finished(&state, "prepare-owner", "cancelled")
            .await;
        assert!(
            trace
                .events
                .borrow()
                .iter()
                .all(|event| event.payload["status"] == "cancelled"),
            "cancelled preparation must not start or report a download"
        );
        assert!(*done.borrow() || done.has_changed().is_err());
    }

    #[tokio::test]
    async fn waiting_for_lifecycle_lock_already_has_a_cancellable_request_slot() {
        let state = AppState::default();
        let lifecycle_guard = state.engine.funasr_lifecycle_op.lock().await;
        let trace = Trace::default();
        let preparation_started = Cell::new(false);
        let preparation = async {
            preparation_started.set(true);
            Err(AppError::Other(
                "preparation must not run after cancellation".into(),
            ))
        };
        let mut worker = Box::pin(run_download_worker(
            &state,
            Some("locked-owner".into()),
            preparation,
            |payload| trace.record(&state, payload),
        ));
        let mut context = Context::from_waker(Waker::noop());
        assert!(worker.as_mut().poll(&mut context).is_pending());
        let done = signal_cancel(&state).await;

        let result = timeout(LIMIT, worker.as_mut())
            .await
            .expect("cancellation must not wait for the unrelated lifecycle lock");

        assert!(result.is_ok());
        assert!(!preparation_started.get());
        trace
            .assert_finished(&state, "locked-owner", "cancelled")
            .await;
        assert!(*done.borrow() || done.has_changed().is_err());
        drop(lifecycle_guard);
    }

    #[tokio::test]
    async fn progress_cancellation_stops_the_owned_child_and_releases_the_slot() {
        let state = AppState::default();
        let trace = Trace::default();
        let (ready_tx, ready_rx) = oneshot::channel();
        let ready_tx = RefCell::new(Some(ready_tx));
        let command = fake_command("import json, os, time\nprint(json.dumps({'stage':'downloading','progress':25,'message':'pid:'+str(os.getpid())}), flush=True)\ntime.sleep(30)");
        let mut worker = Box::pin(run_download_worker(
            &state,
            Some("progress-owner".into()),
            async { Ok(command) },
            |payload| {
                trace.record(&state, payload.clone());
                if payload["status"] == "progress" {
                    if let Some(sender) = ready_tx.borrow_mut().take() {
                        let _ = sender.send(payload);
                    }
                }
            },
        ));
        let progress = timeout(LIMIT, async {
            tokio::select! {
                result = worker.as_mut() => panic!("worker exited before progress: {result:?}"),
                progress = ready_rx => progress.expect("worker publishes progress"),
            }
        })
        .await
        .expect("fake child must publish progress");
        assert_eq!(progress["progress"], 25.0);
        let pid: u32 = progress["message"]
            .as_str()
            .unwrap()
            .strip_prefix("pid:")
            .unwrap()
            .parse()
            .unwrap();
        #[cfg(windows)]
        let process = {
            use std::os::windows::io::{FromRawHandle, OwnedHandle};
            use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};
            let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
            assert!(!handle.is_null(), "observe the still-running test child");
            unsafe { OwnedHandle::from_raw_handle(handle) }
        };
        let done = signal_cancel(&state).await;

        let result = timeout(LIMIT, worker.as_mut())
            .await
            .expect("progress cancellation must finish promptly");

        assert!(result.is_ok());
        assert!(state.engine.download_task.lock().await.is_none());
        assert!(*done.borrow() || done.has_changed().is_err());
        assert_eq!(
            trace
                .events
                .borrow()
                .iter()
                .filter(|event| event.payload["status"] == "cancelled")
                .count(),
            1
        );
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
            use windows_sys::Win32::System::Threading::WaitForSingleObject;
            assert_eq!(
                unsafe { WaitForSingleObject(process.as_raw_handle(), 0) },
                WAIT_OBJECT_0,
                "owned child must have exited before worker return"
            );
        }
        #[cfg(target_os = "linux")]
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "owned child must be reaped before worker return"
        );
        #[cfg(not(any(windows, target_os = "linux")))]
        let _ = pid;
        trace
            .assert_finished(&state, "progress-owner", "cancelled")
            .await;
    }

    #[tokio::test]
    async fn stdout_eof_does_not_disable_cancellation_while_waiting_for_child_exit() {
        let mut child = fake_command("import os, time\nos.close(1)\ntime.sleep(30)")
            .spawn()
            .expect("spawn owned fake child");
        let mut stdout = child.stdout.take().expect("capture fake stdout");
        let mut output = Vec::new();
        let eof = timeout(LIMIT, stdout.read_to_end(&mut output)).await;
        if eof.is_err() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        eof.expect("fake stdout must close while its child stays alive")
            .expect("read stdout EOF");
        assert!(child.try_wait().expect("inspect child").is_none());
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        cancel_tx.send(()).unwrap();

        let waited = timeout(LIMIT, wait_download_exit(&mut child, &mut cancel_rx)).await;
        if child.try_wait().expect("inspect helper result").is_none() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }

        let (_, cancelled) = waited
            .expect("stdout EOF must not make child.wait ignore cancellation")
            .expect("wait helper reaps child");
        assert!(cancelled, "wait helper reports the accepted cancellation");
        assert!(child.try_wait().expect("inspect reaped child").is_some());
    }

    #[test]
    fn cancellation_aware_exit_helper_is_bound_to_the_real_worker_path() {
        let source = include_str!("download_service.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(
            source.matches("run_download_worker").count() >= 2,
            "public run_download must invoke the tested worker core"
        );
        assert!(
            source.matches("wait_download_exit(").count() >= 2,
            "real worker must invoke the tested EOF/exit helper"
        );
    }

    #[tokio::test]
    async fn preparation_failure_publishes_owned_terminal_before_release_and_done() {
        let state = AppState::default();
        let trace = Trace::default();
        let result = timeout(
            LIMIT,
            run_download_worker(
                &state,
                Some("prepare-error-owner".into()),
                async { Err(AppError::Download("fixture preparation failed".into())) },
                |payload| trace.record(&state, payload),
            ),
        )
        .await
        .expect("preparation failure must settle");

        assert!(result.is_err());
        trace
            .assert_finished(&state, "prepare-error-owner", "error")
            .await;
    }

    async fn assert_process_terminal(protocol_success: bool, terminal: &str) {
        let state = AppState::default();
        let trace = Trace::default();
        let success = if protocol_success { "True" } else { "False" };
        let error_stage = if protocol_success {
            ""
        } else {
            "print(json.dumps({'stage':'error','error':'fixture download failed'}))\n"
        };
        let command = fake_command(&format!("import json\nprint(json.dumps({{'stage':'downloading','progress':25}}))\n{error_stage}print(json.dumps({{'success':{success},'message':'fixture outcome'}}))"));
        let result = timeout(
            LIMIT,
            run_download_worker(
                &state,
                Some("process-owner".into()),
                async { Ok(command) },
                |payload| trace.record(&state, payload),
            ),
        )
        .await
        .expect("owned fake child must finish");

        assert_eq!(result.is_ok(), protocol_success);
        trace
            .assert_finished(&state, "process-owner", terminal)
            .await;
    }

    #[tokio::test]
    async fn success_publishes_owned_terminal_before_release_and_done() {
        assert_process_terminal(true, "completed").await;
    }

    #[tokio::test]
    async fn protocol_failure_publishes_owned_terminal_before_release_and_done() {
        assert_process_terminal(false, "error").await;
    }
}
