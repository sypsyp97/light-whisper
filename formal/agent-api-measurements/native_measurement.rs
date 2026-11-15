//! Measurement-only Cargo bridge to the existing real native host driver.
#![cfg(windows)]

mod agent_api {
    mod tests {
        use sha2::{Digest, Sha256};
        use std::io::Write;
        use std::os::windows::{io::AsRawHandle, process::CommandExt};
        use std::path::{Path, PathBuf};
        use std::process::{Child, Command, Stdio};
        use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };

        const EARLY_RUN: &str = r#"
            if std::env::args().any(|argument| argument == "--agent-host")
                && std::env::var_os("LIGHT_WHISPER_DATA_DIR").is_none_or(|directory| directory.is_empty())
            {
                eprintln!("--agent-host requires LIGHT_WHISPER_DATA_DIR");
                std::process::exit(2);
            }
            if std::env::args().any(|argument| argument == "--agent-client") {
                if let Err(error) = agent_api::run_client() {
                    eprintln!("Agent client failed: {error}");
                    std::process::exit(1);
                }
                return;
            }
            if std::env::args().any(|argument| argument == "--agent-host") {
                if let Err(error) = agent_api::run_host() {
                    eprintln!("Agent host failed: {error}");
                    std::process::exit(1);
                }
                return;
            }
        "#;

        fn compact(source: &str) -> String {
            source.chars().filter(|c| !c.is_whitespace()).collect()
        }

        fn source_guard(repo: &Path) {
            let source = std::fs::read_to_string(repo.join("src-tauri/src/lib.rs")).unwrap();
            let (_, run) = source
                .split_once("pub fn run() {")
                .expect("one owning run entrypoint");
            let (early, _) = run.split_once("tauri::Builder::default()").unwrap();
            assert_eq!(
                compact(early),
                compact(EARLY_RUN),
                "source-safety refusal: early agent dispatch changed; no process spawned"
            );
            let source =
                std::fs::read_to_string(repo.join("src-tauri/src/agent_api/host.rs")).unwrap();
            let (_, run) = source
                .split_once("pub(crate) fn run_host() -> Result<(), Box<dyn std::error::Error>> {")
                .unwrap();
            assert!(compact(run).starts_with(&compact(
                "let mut context = tauri::generate_context!(); context.config_mut().app.windows.clear();")),
                "source-safety refusal: window clearing changed; no process spawned");
        }

        fn digest(path: &Path) -> String {
            format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
        }

        struct Driver {
            child: Child,
            job: HANDLE,
        }

        impl Drop for Driver {
            fn drop(&mut self) {
                let _ = self.child.kill();
                // SAFETY: This job and child belong exclusively to this test. Closing
                // the job also contains descendants if the test process is killed.
                if !self.job.is_null() {
                    unsafe {
                        TerminateJobObject(self.job, 96);
                        CloseHandle(self.job);
                    }
                }
                let _ = self.child.wait();
            }
        }

        #[test]
        fn actual_native_host() {
            let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
            source_guard(repo);
            let cohort: serde_json::Value =
                serde_json::from_str(include_str!("cohort.json")).unwrap();
            let fixture = PathBuf::from(
                std::env::var_os("LW_MEASUREMENT_FIXTURE").expect("trusted fixture required"),
            );
            assert_eq!(
                digest(&fixture),
                cohort["trusted_fixture_sha256"].as_str().unwrap()
            );
            let negative = std::env::var_os("LW_MEASUREMENT_MARKER_NEGATIVE_EXE");
            let app = negative
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_light-whisper")));
            if negative.is_some() {
                assert_eq!(
                    digest(&app),
                    digest(&fixture),
                    "negative PE must be the trusted harmless fixture"
                );
            }
            let evidence = PathBuf::from(
                std::env::var_os("LW_MEASUREMENT_EVIDENCE").expect("owned evidence root required"),
            )
            .join(format!(
                "native-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&evidence).unwrap();
            std::fs::write(evidence.join("bridge-identity.json"), serde_json::to_vec_pretty(&serde_json::json!({
                "app_sha256": digest(&app), "fixture_sha256": digest(&fixture),
                "source_lib_sha256": digest(&repo.join("src-tauri/src/lib.rs")),
                "source_host_sha256": digest(&repo.join("src-tauri/src/agent_api/host.rs")),
                "cohort_sha256": digest(&repo.join("formal/agent-api-measurements/cohort.json")),
                "marker_negative_only": negative.is_some()
            })).unwrap()).unwrap();

            // Block before loading the existing driver or creating descendants.
            // Assign to the kill-on-close job, then release exactly this child.
            let bootstrap = "import runpy,sys; assert sys.stdin.buffer.read(1)==b'1'; p=sys.argv.pop(1); runpy.run_path(p,run_name='__main__')";
            let python = std::env::var_os("LW_MEASUREMENT_PYTHON")
                .expect("explicit supported project Python required");
            let mut command = Command::new(python);
            command
                .args(["-c", bootstrap])
                .arg(repo.join("formal/agent-api-host/run.py"))
                .arg("--app-exe")
                .arg(&app)
                .arg("--fixture-exe")
                .arg(&fixture)
                .arg("--evidence")
                .arg(&evidence)
                .current_dir(repo)
                .stdin(Stdio::piped())
                .creation_flags(0x08000000);
            if negative.is_some() {
                command.args(["--case", "test_01_compiled_protocol_marker"]);
            }
            let child = command.spawn().unwrap();
            let mut driver = Driver {
                child,
                job: std::ptr::null_mut(),
            };
            // SAFETY: Null attributes/name create one private, initially empty job.
            let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            assert!(!job.is_null());
            driver.job = job;
            // SAFETY: Initialized POD structure and owned live job/process handles.
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            assert_ne!(
                unsafe {
                    SetInformationJobObject(
                        job,
                        JobObjectExtendedLimitInformation,
                        (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                        std::mem::size_of_val(&limits) as u32,
                    )
                },
                0
            );
            assert_ne!(
                unsafe { AssignProcessToJobObject(job, driver.child.as_raw_handle().cast()) },
                0
            );
            driver.child.stdin.take().unwrap().write_all(b"1").unwrap();
            let deadline = Instant::now() + Duration::from_secs(900);
            let status = loop {
                if let Some(status) = driver.child.try_wait().unwrap() {
                    break status;
                }
                assert!(
                    Instant::now() < deadline,
                    "owned native driver watchdog expired"
                );
                std::thread::sleep(Duration::from_millis(25));
            };
            assert!(
                status.success(),
                "existing actual native host driver failed: {status}; evidence {}",
                evidence.display()
            );
        }
    }
}
