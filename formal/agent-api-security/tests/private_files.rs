#![cfg(windows)]

#[path = "../../../src-tauri/src/agent_api/secure_files.rs"]
mod secure_files;
mod support;

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use support::*;
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_CRC, ERROR_INVALID_DATA, ERROR_SHARING_VIOLATION,
};
use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

const PROBE: &str = env!("CARGO_BIN_EXE_private-file-probe");

struct OwnedChild(Child);
impl OwnedChild {
    fn wait(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "owned child exceeded 10s deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn run_probe(args: &[&std::ffi::OsStr]) -> String {
    let mut child = OwnedChild(
        Command::new(PROBE)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let status = child.wait();
    let mut output = String::new();
    child
        .0
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut output)
        .unwrap();
    assert!(status.success(), "probe failed: {status}, {output}");
    output.trim().to_owned()
}

#[test]
fn creation_is_private_before_any_secret_is_written_under_a_broad_parent() {
    let fixture = Fixture::new();
    let parent = security(&open_directory(&fixture.0));
    assert!(
        parent.aces.iter().any(|(_, _, _, sid)| sid == "S-1-1-0"),
        "fixture must actually be broad"
    );
    let path = fixture.0.join("endpoint.json");
    let mut file =
        secure_files::create_private_file(&path).expect("new private file must be creatable");
    // Inspect ACL and handle flags BEFORE representative secret bytes exist.
    assert_private(&file);
    assert_plain_file(&file);
    assert_eq!(file.metadata().unwrap().len(), 0);
    file.write_all(b"synthetic-secret").unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"synthetic-secret");
    let error = fs::rename(&path, fixture.0.join("moved")).unwrap_err();
    assert_eq!(
        error.raw_os_error(),
        Some(ERROR_SHARING_VIOLATION as i32),
        "created handle must omit FILE_SHARE_DELETE"
    );
}

#[test]
fn create_new_refuses_to_overwrite_or_repair_an_existing_broad_file() {
    let fixture = Fixture::new();
    let path = fixture.0.join("existing");
    fs::write(&path, b"keep-this-value").unwrap();
    let before = security(&File::open(&path).unwrap());
    assert!(before.aces.iter().any(|(_, _, _, sid)| sid == "S-1-1-0"));
    assert!(secure_files::create_private_file(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"keep-this-value");
    assert_eq!(security(&File::open(&path).unwrap()).aces, before.aces);
}

#[test]
fn reopened_private_handle_preserves_contents_and_honors_read_only_access() {
    let fixture = Fixture::new();
    let path = fixture.0.join("private");
    let mut seeded = create_file(&path, &private_sddl()).unwrap();
    seeded.write_all(b"original").unwrap();
    drop(seeded);
    let mut reader =
        secure_files::open_private_file(&path, false).expect("private file must reopen");
    assert_private(&reader);
    assert_plain_file(&reader);
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"original");
    assert_eq!(
        reader.write_all(b"x").unwrap_err().raw_os_error(),
        Some(ERROR_ACCESS_DENIED as i32)
    );
    assert_eq!(
        fs::rename(&path, fixture.0.join("renamed"))
            .unwrap_err()
            .raw_os_error(),
        Some(ERROR_SHARING_VIOLATION as i32)
    );
    drop(reader);
    let mut writer = secure_files::open_private_file(&path, true).unwrap();
    assert_private(&writer);
    writer.seek(SeekFrom::End(0)).unwrap();
    writer.write_all(b"-updated").unwrap();
    drop(writer);
    assert_eq!(fs::read(path).unwrap(), b"original-updated");
}

#[test]
fn opening_broad_null_or_unprotected_dacl_fails_closed_without_mutation() {
    let fixture = Fixture::new();
    let dir = fixture.private_dir("cases");
    let user = user_sid();
    let cases = [
        ("broad", format!("O:{user}D:P(A;;FA;;;{user})(A;;FA;;;WD)")),
        ("null", format!("O:{user}D:NO_ACCESS_CONTROL")),
        (
            "unprotected",
            format!("O:{user}D:(A;;FA;;;{user})(A;;FA;;;SY)"),
        ),
        (
            "propagating",
            format!("O:{user}D:P(A;OI;FA;;;{user})(A;;FA;;;SY)"),
        ),
    ];
    for (name, sddl) in cases {
        let path = dir.join(name);
        let mut seeded = create_file(&path, &sddl).unwrap();
        if name == "unprotected" {
            make_dacl_unprotected(&seeded, &sddl);
        }
        seeded.write_all(b"unchanged").unwrap();
        let before = security(&seeded);
        drop(seeded);
        if name == "null" {
            assert!(before.dacl_null);
        }
        if name == "unprotected" {
            assert_eq!(
                before.control & windows_sys::Win32::Security::SE_DACL_PROTECTED,
                0
            );
        }
        for write in [false, true] {
            assert!(
                secure_files::open_private_file(&path, write).is_err(),
                "must reject {name}, write={write}"
            );
        }
        assert_eq!(fs::read(&path).unwrap(), b"unchanged");
        assert_eq!(security(&File::open(path).unwrap()).aces, before.aces);
    }
}

#[test]
fn private_directory_is_created_and_existing_private_directory_is_accepted() {
    let fixture = Fixture::new();
    let path = fixture.0.join("runtime");
    secure_files::ensure_private_dir(&path).expect("new private directory must be created");
    assert!(path.is_dir());
    assert_private(&open_directory(&path));
    secure_files::ensure_private_dir(&path).expect("existing private directory is idempotent");
    assert_private(&open_directory(&path));
}

#[test]
fn broad_directory_and_regular_file_cannot_be_used_as_private_directory() {
    let fixture = Fixture::new();
    let file = fixture.0.join("regular");
    drop(create_file(&file, &private_sddl()).unwrap());
    let before = security(&open_directory(&fixture.0));
    assert!(
        secure_files::ensure_private_dir(&fixture.0).is_err(),
        "broad existing directory must fail closed"
    );
    assert_eq!(security(&open_directory(&fixture.0)).aces, before.aces);
    assert!(secure_files::ensure_private_dir(&file).is_err());
    assert!(file.is_file());
    assert!(
        secure_files::open_private_file(&fixture.0, false).is_err(),
        "directory is not an endpoint file"
    );
}

#[test]
fn directory_reparse_point_is_rejected_without_touching_its_target() {
    let fixture = Fixture::new();
    let target = fixture.private_dir("target");
    let junction = fixture.0.join("junction");
    let status = Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&target)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .unwrap();
    assert!(status.success(), "junction fixture must actually exist");
    let before = security(&open_directory(&target));
    assert!(
        secure_files::ensure_private_dir(&junction).is_err(),
        "reparse directory must be rejected"
    );
    assert!(secure_files::open_private_file(&junction, false).is_err());
    assert_eq!(security(&open_directory(&target)).aces, before.aces);
    fs::remove_dir(&junction).unwrap();
    assert!(target.is_dir());
}

#[test]
fn publication_replaces_stale_broad_descriptor_with_private_complete_bytes() {
    let fixture = Fixture::new();
    let dir = fixture.private_dir("runtime");
    let path = dir.join("endpoint.json");
    let user = user_sid();
    let mut stale =
        create_file(&path, &format!("O:{user}D:P(A;;FA;;;{user})(A;;FA;;;WD)")).unwrap();
    stale.write_all(b"stale-public-value").unwrap();
    drop(stale);
    assert!(security(&File::open(&path).unwrap())
        .aces
        .iter()
        .any(|(_, _, _, sid)| sid == "S-1-1-0"));
    let bytes = br#"{"token":"synthetic-token","port":12345}"#;
    secure_files::publish_private_file(&path, bytes)
        .expect("private atomic publication must succeed");
    let mut published = secure_files::open_private_file(&path, false).unwrap();
    assert_private(&published);
    let mut actual = Vec::new();
    published.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, bytes);
    drop(published);
    assert_eq!(
        fs::read_dir(&dir).unwrap().count(),
        1,
        "successful publish must leave no temp"
    );
    secure_files::publish_private_file(&path, b"replacement").unwrap();
    assert_eq!(fs::read(path).unwrap(), b"replacement");
}

#[test]
fn publication_creates_absent_destination_with_private_complete_bytes() {
    let fixture = Fixture::new();
    let dir = fixture.private_dir("runtime");
    let path = dir.join("endpoint.json");
    assert!(!path.exists());
    let bytes = br#"{"token":"synthetic-new-token","port":12345}"#;
    secure_files::publish_private_file(&path, bytes)
        .expect("publication to absent destination must succeed");
    let published = File::open(&path).unwrap();
    assert_private(&published);
    assert_plain_file(&published);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
}

fn assert_observed_publication(path: &Path, bytes: &[u8], previous: Option<&[u8]>) {
    let mut original_handle = previous.map(|_| File::open(path).unwrap());
    let original_identity = original_handle.as_ref().map(file_identity);
    let original_aces = original_handle.as_ref().map(|file| security(file).aces);
    let mut points = Vec::new();
    let mut staging = None;
    let mut staging_identity = None;
    let mut observe = |point, temp_path: &Path, temp_file: &File| {
        assert_ne!(temp_path, path, "publication must write a staging file");
        assert_eq!(
            temp_path.parent(),
            path.parent(),
            "staging must share destination directory"
        );
        assert_private(temp_file);
        assert_plain_file(temp_file);
        let opened_stage = File::open(temp_path).unwrap();
        assert_eq!(
            file_identity(&opened_stage),
            file_identity(temp_file),
            "observer must inspect actual named staging handle"
        );
        match previous {
            Some(old) => {
                assert_eq!(
                    fs::read(path).unwrap(),
                    old,
                    "old destination must stay intact until replacement"
                );
                let current = File::open(path).unwrap();
                assert_eq!(Some(file_identity(&current)), original_identity);
                assert_eq!(Some(security(&current).aces), original_aces);
            }
            None => assert!(
                !path.exists(),
                "new destination must remain absent until replacement"
            ),
        }
        match point {
            secure_files::PublishPoint::BeforeFirstWrite => {
                assert!(
                    points.is_empty(),
                    "first-write observation must occur first and exactly once"
                );
                assert_eq!(
                    temp_file.metadata().unwrap().len(),
                    0,
                    "no secret bytes may precede the private ACL observation"
                );
                assert!(fs::read(temp_path).unwrap().is_empty());
                staging = Some(temp_path.to_owned());
                staging_identity = Some(file_identity(temp_file));
                points.push("before-first-write");
            }
            secure_files::PublishPoint::BeforeReplace => {
                assert_eq!(points, ["before-first-write"]);
                assert_eq!(staging.as_deref(), Some(temp_path));
                assert_eq!(staging_identity, Some(file_identity(temp_file)));
                assert_eq!(
                    fs::read(temp_path).unwrap(),
                    bytes,
                    "staging must contain the entire value before replacement"
                );
                points.push("before-replace");
            }
        }
    };
    secure_files::publish_private_file_observed(path, bytes, &mut observe)
        .expect("observed production publication must succeed");
    assert_eq!(
        points,
        ["before-first-write", "before-replace"],
        "both actual side-effect boundaries must be observed"
    );
    let published = File::open(path).unwrap();
    assert_private(&published);
    assert_plain_file(&published);
    assert_eq!(
        Some(file_identity(&published)),
        staging_identity,
        "publication must move the staged file, not copy or rewrite bytes"
    );
    assert_eq!(fs::read(path).unwrap(), bytes);
    assert!(
        !staging.unwrap().exists(),
        "successful move must consume staging path"
    );
    if let Some(old) = original_handle.as_mut() {
        assert_ne!(Some(file_identity(&published)), original_identity);
        old.seek(SeekFrom::Start(0)).unwrap();
        let mut old_bytes = Vec::new();
        old.read_to_end(&mut old_bytes).unwrap();
        assert_eq!(
            Some(old_bytes.as_slice()),
            previous,
            "replacement must leave an existing open handle on the old object"
        );
    }
    assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
}

#[test]
fn publication_observes_private_stage_before_write_and_moves_it_to_absent_destination() {
    let fixture = Fixture::new();
    let dir = fixture.private_dir("runtime");
    let path = dir.join("endpoint.json");
    assert_observed_publication(&path, b"synthetic-new-publication", None);
}

#[test]
fn publication_observes_private_stage_and_replaces_without_rewriting_old_open_object() {
    let fixture = Fixture::new();
    let dir = fixture.private_dir("runtime");
    let path = dir.join("endpoint.json");
    let user = user_sid();
    let mut old = create_file(&path, &format!("O:{user}D:P(A;;FA;;;{user})(A;;FA;;;WD)")).unwrap();
    old.write_all(b"synthetic-stale-broad-value").unwrap();
    drop(old);
    assert_observed_publication(
        &path,
        b"synthetic-private-replacement",
        Some(b"synthetic-stale-broad-value"),
    );
}

#[test]
fn failed_publication_preserves_destination_and_cleans_only_owned_temp() {
    let fixture = Fixture::new();
    let dir = fixture.private_dir("runtime");
    let path = dir.join("endpoint.json");
    let mut seeded = create_file(&path, &private_sddl()).unwrap();
    seeded.write_all(b"last-known-good").unwrap();
    drop(seeded);
    let unrelated = dir.join("unrelated.tmp");
    fs::write(&unrelated, b"preserve-neighbor").unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(&path)
        .unwrap();
    assert!(
        secure_files::publish_private_file(&path, b"must-not-publish").is_err(),
        "rename must fail while destination denies delete sharing"
    );
    assert_eq!(fs::read(&path).unwrap(), b"last-known-good");
    assert_eq!(fs::read(&unrelated).unwrap(), b"preserve-neighbor");
    let mut names: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            std::ffi::OsString::from("endpoint.json"),
            std::ffi::OsString::from("unrelated.tmp")
        ],
        "failure must remove its new temp only"
    );
    drop(held);
}

#[test]
fn restricted_token_reads_broad_control_and_is_denied_private_file() {
    let fixture = Fixture::new();
    let private = fixture.0.join("private");
    let broad = fixture.0.join("broad-control");
    let mut file = secure_files::create_private_file(&private).unwrap();
    assert_private(&file);
    file.write_all(b"synthetic-private-data").unwrap();
    drop(file);
    fs::write(&broad, b"synthetic-public-control").unwrap();
    assert_eq!(
        run_probe(&[
            "restricted-read".as_ref(),
            broad.as_os_str(),
            private.as_os_str()
        ]),
        "broad-readable private-access-denied reverted"
    );
}

#[test]
fn real_process_host_lock_blocks_second_host_and_path_replacement_then_releases() {
    let fixture = Fixture::new();
    let path = fixture.0.join("host.lock");
    drop(secure_files::create_private_file(&path).unwrap());
    let mut host = OwnedChild(
        Command::new(PROBE)
            .arg("hold-lock")
            .arg(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let stdout = host.0.stdout.take().unwrap();
    let (send, receive) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = send.send(result);
    });
    assert_eq!(
        receive
            .recv_timeout(Duration::from_secs(10))
            .expect("host readiness deadline")
            .unwrap()
            .trim(),
        "locked"
    );
    reader.join().unwrap();
    assert_eq!(
        run_probe(&["try-lock".as_ref(), path.as_os_str()]),
        "blocked"
    );
    assert_eq!(
        fs::rename(&path, fixture.0.join("replacement"))
            .unwrap_err()
            .raw_os_error(),
        Some(ERROR_SHARING_VIOLATION as i32)
    );
    host.0
        .stdin
        .take()
        .unwrap()
        .write_all(b"release\n")
        .unwrap();
    assert!(host.wait().success());
    assert_eq!(
        run_probe(&["try-lock".as_ref(), path.as_os_str()]),
        "acquired"
    );
}

#[test]
fn missing_parent_and_embedded_nul_are_rejected_without_artifacts() {
    let fixture = Fixture::new();
    let missing = fixture.0.join("missing").join("file");
    assert!(secure_files::create_private_file(&missing).is_err());
    assert!(secure_files::open_private_file(&missing, true).is_err());
    assert!(secure_files::publish_private_file(&missing, b"x").is_err());
    let invalid = fixture.0.join("prefix\0suffix");
    assert!(secure_files::create_private_file(&invalid).is_err());
    assert!(secure_files::open_private_file(&invalid, false).is_err());
    assert!(secure_files::ensure_private_dir(&invalid).is_err());
    assert!(secure_files::publish_private_file(&invalid, b"x").is_err());
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}

#[test]
fn native_fixture_acl_oracle_and_restricted_token_control_are_usable() {
    let fixture = Fixture::new();
    let path = fixture.0.join("independent-private-control");
    let broad = fixture.0.join("broad-control");
    let mut file = create_file(&path, &private_sddl()).unwrap();
    assert_private(&file);
    file.write_all(b"control").unwrap();
    drop(file);
    fs::write(&broad, b"readable").unwrap();
    assert_eq!(
        run_probe(&[
            "restricted-read".as_ref(),
            broad.as_os_str(),
            path.as_os_str()
        ]),
        "broad-readable private-access-denied reverted"
    );
}

fn validation_fault() -> io::Error {
    io::Error::from_raw_os_error(ERROR_INVALID_DATA as i32)
}

#[test]
fn post_create_validation_failure_removes_empty_new_file_and_preserves_error() {
    let fixture = Fixture::new();
    let path = fixture.0.join("new-owned-file");
    let unrelated = fixture.0.join("unrelated");
    fs::write(&unrelated, b"preserve-neighbor").unwrap();
    let mut invoked = false;
    let result = secure_files::with_post_create_validation_error(validation_fault(), || {
        invoked = true;
        secure_files::create_private_file(&path)
    });
    assert!(
        invoked,
        "fault scope must invoke the real public create operation"
    );
    let error = result.expect_err("post-create validation error must propagate");
    assert_eq!(error.raw_os_error(), Some(ERROR_INVALID_DATA as i32));
    assert!(
        !path.exists(),
        "failed post-create validation must remove its empty newly created file"
    );
    assert_eq!(fs::read(&unrelated).unwrap(), b"preserve-neighbor");
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
}

#[test]
fn post_create_validation_failure_removes_empty_publish_stage_and_preserves_destination() {
    let fixture = Fixture::new();
    let dir = fixture.private_dir("runtime");
    let path = dir.join("endpoint.json");
    let mut old = create_file(&path, &private_sddl()).unwrap();
    old.write_all(b"last-known-good").unwrap();
    let old_identity = file_identity(&old);
    drop(old);
    let unrelated = dir.join("unrelated.tmp");
    fs::write(&unrelated, b"preserve-neighbor").unwrap();
    let mut invoked = false;
    let result = secure_files::with_post_create_validation_error(validation_fault(), || {
        invoked = true;
        secure_files::publish_private_file(&path, b"must-not-be-written")
    });
    assert!(
        invoked,
        "fault scope must invoke the real shared publication core"
    );
    let error = result.expect_err("stage validation failure must propagate");
    assert_eq!(error.raw_os_error(), Some(ERROR_INVALID_DATA as i32));
    assert_eq!(fs::read(&path).unwrap(), b"last-known-good");
    assert_eq!(file_identity(&File::open(&path).unwrap()), old_identity);
    assert_eq!(fs::read(&unrelated).unwrap(), b"preserve-neighbor");
    let mut names: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            std::ffi::OsString::from("endpoint.json"),
            std::ffi::OsString::from("unrelated.tmp")
        ],
        "validation failure must leave no newly created empty staging artifact"
    );
}

#[test]
fn post_create_fault_skips_preexisting_file_and_is_consumed_by_one_successful_creation() {
    let fixture = Fixture::new();
    let existing = fixture.0.join("preexisting");
    fs::write(&existing, b"do-not-delete-or-overwrite").unwrap();
    let existing_identity = file_identity(&File::open(&existing).unwrap());
    let existing_aces = security(&File::open(&existing).unwrap()).aces;
    let private_existing = fixture.0.join("private-existing");
    let mut seeded = create_file(&private_existing, &private_sddl()).unwrap();
    seeded.write_all(b"private-existing-value").unwrap();
    drop(seeded);
    let existing_directory = fixture.private_dir("existing-directory");
    let new_directory = fixture.0.join("new-directory");
    let failed_new = fixture.0.join("failed-new");
    let next_new = fixture.0.join("next-new");
    let result = secure_files::with_post_create_validation_error(validation_fault(), || {
        secure_files::with_post_create_validation_error(
            io::Error::from_raw_os_error(ERROR_CRC as i32),
            || Ok(()),
        )?;
        let mut reader = secure_files::open_private_file(&private_existing, false)?;
        assert_private(&reader);
        let mut existing_bytes = Vec::new();
        reader.read_to_end(&mut existing_bytes).unwrap();
        assert_eq!(existing_bytes, b"private-existing-value");
        drop(reader);
        let writer = secure_files::open_private_file(&private_existing, true)?;
        assert_private(&writer);
        drop(writer);
        secure_files::ensure_private_dir(&existing_directory)?;
        assert_private(&open_directory(&existing_directory));
        secure_files::ensure_private_dir(&new_directory)?;
        assert_private(&open_directory(&new_directory));
        let preexisting_error = secure_files::create_private_file(&existing).unwrap_err();
        assert_ne!(
            preexisting_error.raw_os_error(),
            Some(ERROR_INVALID_DATA as i32),
            "CREATE_NEW rejection must not consume the post-creation fault"
        );
        assert_eq!(fs::read(&existing).unwrap(), b"do-not-delete-or-overwrite");
        let injected_error = secure_files::create_private_file(&failed_new).unwrap_err();
        assert_eq!(
            injected_error.raw_os_error(),
            Some(ERROR_INVALID_DATA as i32)
        );
        assert!(
            !failed_new.exists(),
            "the failed new object is owned and must be removed"
        );
        let next = secure_files::create_private_file(&next_new)?;
        assert_private(&next);
        Ok(())
    });
    result.expect("a single injected failure must not poison subsequent creation");
    assert_eq!(fs::read(&existing).unwrap(), b"do-not-delete-or-overwrite");
    assert_eq!(
        file_identity(&File::open(&existing).unwrap()),
        existing_identity
    );
    assert_eq!(
        security(&File::open(&existing).unwrap()).aces,
        existing_aces
    );
    assert!(!failed_new.exists());
    assert!(next_new.is_file());
    assert_eq!(
        fs::read(&private_existing).unwrap(),
        b"private-existing-value"
    );
    assert!(new_directory.is_dir());
}

#[test]
fn unused_fault_scope_restores_state_after_success_error_and_unwind() {
    let fixture = Fixture::new();
    secure_files::with_post_create_validation_error(validation_fault(), || Ok(()))
        .expect("an unused scoped fault must preserve successful operation result");
    drop(secure_files::create_private_file(&fixture.0.join("after-success")).unwrap());
    let error = secure_files::with_post_create_validation_error::<()>(validation_fault(), || {
        Err(io::Error::from_raw_os_error(ERROR_CRC as i32))
    })
    .unwrap_err();
    assert_eq!(
        error.raw_os_error(),
        Some(ERROR_CRC as i32),
        "operation error must survive an unused fault scope"
    );
    drop(secure_files::create_private_file(&fixture.0.join("after-error")).unwrap());
    let panic = std::panic::catch_unwind(|| {
        let _ = secure_files::with_post_create_validation_error::<()>(validation_fault(), || {
            panic!("intentional scoped-fault unwind")
        });
    });
    assert!(
        panic.is_err(),
        "operation panic must pass through the scope"
    );
    drop(secure_files::create_private_file(&fixture.0.join("after-unwind")).unwrap());
}

#[test]
fn post_create_validation_fault_is_thread_local() {
    let fixture = Fixture::new();
    let other_thread = fixture.0.join("other-thread");
    let faulted_thread = fixture.0.join("faulted-thread");
    secure_files::with_post_create_validation_error(validation_fault(), || {
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let file = secure_files::create_private_file(&other_thread)
                        .expect("another thread must not consume this scope's validation fault");
                    assert_private(&file);
                })
                .join()
                .unwrap();
        });
        let error = secure_files::create_private_file(&faulted_thread).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(ERROR_INVALID_DATA as i32));
        assert!(!faulted_thread.exists());
        Ok(())
    })
    .expect("the owning thread alone must consume its one-shot validation fault");
    assert!(other_thread.is_file());
}

#[test]
fn nested_fault_scope_restores_armed_outer_after_inner_error_and_unwind() {
    let fixture = Fixture::new();
    for mode in ["error", "unwind"] {
        let faulted = fixture.0.join(format!("outer-fault-{mode}"));
        let next = fixture.0.join(format!("after-outer-fault-{mode}"));
        secure_files::with_post_create_validation_error(validation_fault(), || {
            if mode == "error" {
                let inner = secure_files::with_post_create_validation_error::<()>(
                    io::Error::from_raw_os_error(ERROR_CRC as i32),
                    || Err(io::Error::from_raw_os_error(ERROR_ACCESS_DENIED as i32)),
                )
                .unwrap_err();
                assert_eq!(inner.raw_os_error(), Some(ERROR_ACCESS_DENIED as i32));
            } else {
                let inner = std::panic::catch_unwind(|| {
                    let _ = secure_files::with_post_create_validation_error::<()>(
                        io::Error::from_raw_os_error(ERROR_CRC as i32),
                        || panic!("intentional nested fault-scope unwind"),
                    );
                });
                assert!(
                    inner.is_err(),
                    "inner operation panic must reach its caller"
                );
            }
            let restored = secure_files::create_private_file(&faulted).unwrap_err();
            assert_eq!(
                restored.raw_os_error(),
                Some(ERROR_INVALID_DATA as i32),
                "unused inner scope must restore the original armed outer fault"
            );
            assert!(!faulted.exists());
            let file = secure_files::create_private_file(&next)?;
            assert_private(&file);
            Ok(())
        })
        .expect("outer fault must remain armed across inner exits and be consumed once");
        assert!(!faulted.exists());
        assert!(next.is_file());
    }
}

#[test]
fn restricted_current_user_query_failure_is_propagated_without_creating_a_file() {
    let fixture = Fixture::new();
    let path = fixture.0.join("denied-current-user");
    let control = fixture.0.join("reverted-current-user-control");
    run_probe(&[
        "restricted-current-user-denial".as_ref(),
        path.as_os_str(),
        control.as_os_str(),
    ]);
    assert!(!path.try_exists().unwrap());
    assert_eq!(fs::read(&control).unwrap(), b"reverted-control");
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
}
