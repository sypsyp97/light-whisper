#![cfg(windows)]

#[path = "../../../../src-tauri/src/agent_api/secure_files.rs"]
mod secure_files;
#[path = "../../tests/support/mod.rs"]
mod support;

use std::fs::File;
use std::io::{self, BufRead, Read, Write};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{LocalFree, ERROR_ACCESS_DENIED};
use windows_sys::Win32::Security::Authorization::ConvertStringSidToSidW;
use windows_sys::Win32::Security::*;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

struct Impersonation;
impl Drop for Impersonation {
    fn drop(&mut self) {
        // Isolated child: error/panic paths must also revert impersonation.
        if unsafe { RevertToSelf() } == 0 {
            std::process::abort();
        }
    }
}

fn restricted_read(broad: &std::path::Path, private: &std::path::Path) -> io::Result<()> {
    let token = support::process_token()?;
    let mut everyone = null_mut();
    if unsafe { ConvertStringSidToSidW(support::wide("S-1-1-0").as_ptr(), &mut everyone) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let restriction = SID_AND_ATTRIBUTES {
        Sid: everyone,
        Attributes: 0,
    };
    let mut restricted = null_mut();
    let created = unsafe {
        CreateRestrictedToken(
            token.0,
            DISABLE_MAX_PRIVILEGE,
            0,
            null(),
            0,
            null(),
            1,
            &restriction,
            &mut restricted,
        )
    };
    unsafe { LocalFree(everyone) };
    if created == 0 {
        return Err(io::Error::last_os_error());
    }
    let restricted = support::Handle(restricted);
    if unsafe { ImpersonateLoggedOnUser(restricted.0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let impersonation = Impersonation;
    let mut control = Vec::new();
    File::open(broad)?.read_to_end(&mut control)?;
    if control.is_empty() {
        return Err(io::Error::other("broad fixture must contain data"));
    }
    let error = File::open(private).expect_err("restricted token must not read private file");
    assert_eq!(error.raw_os_error(), Some(ERROR_ACCESS_DENIED as i32));
    drop(impersonation);
    File::open(private)?.read_to_end(&mut Vec::new())?;
    println!("broad-readable private-access-denied reverted");
    Ok(())
}

fn query_current_process_token() -> io::Result<support::Handle> {
    let mut token = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(support::Handle(token))
}

fn restricted_current_user_denial(
    path: &std::path::Path,
    control: &std::path::Path,
) -> io::Result<()> {
    drop(query_current_process_token()?);
    assert!(!path.try_exists()?);
    assert!(!control.try_exists()?);
    let token = support::process_token()?;
    let mut everyone = null_mut();
    if unsafe { ConvertStringSidToSidW(support::wide("S-1-1-0").as_ptr(), &mut everyone) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let restriction = SID_AND_ATTRIBUTES {
        Sid: everyone,
        Attributes: 0,
    };
    let mut restricted = null_mut();
    let created = unsafe {
        CreateRestrictedToken(
            token.0,
            DISABLE_MAX_PRIVILEGE,
            0,
            null(),
            0,
            null(),
            1,
            &restriction,
            &mut restricted,
        )
    };
    let creation_error = io::Error::last_os_error();
    unsafe { LocalFree(everyone) };
    if created == 0 {
        return Err(creation_error);
    }
    let restricted = support::Handle(restricted);
    if unsafe { ImpersonateLoggedOnUser(restricted.0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let impersonation = Impersonation;
    let native_error = match query_current_process_token() {
        Ok(_) => panic!("restricted thread must not query the process token"),
        Err(error) => error,
    };
    assert_eq!(native_error.raw_os_error(), Some(ERROR_ACCESS_DENIED as i32));
    let error = secure_files::create_private_file(path)
        .expect_err("process-token denial must propagate from private creation");
    assert_eq!(error.raw_os_error(), Some(ERROR_ACCESS_DENIED as i32));
    drop(impersonation);
    drop(query_current_process_token()?);
    assert!(!path.try_exists()?);
    let mut file = secure_files::create_private_file(control)?;
    support::assert_private(&file);
    file.write_all(b"reverted-control")?;
    drop(file);
    let mut bytes = Vec::new();
    secure_files::open_private_file(control, false)?.read_to_end(&mut bytes)?;
    assert_eq!(bytes, b"reverted-control");
    println!("process-token-access-denied no-file reverted-control-readable");
    Ok(())
}

fn main() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    let path = std::path::Path::new(&args[2]);
    match args[1].to_str().unwrap() {
        "fixture-capabilities" => {
            let fixture = support::Fixture::new();
            let target = fixture.0.join("target");
            drop(support::create_file(&target, &support::private_sddl())?);
            let link = fixture.0.join("file-link");
            match std::os::windows::fs::symlink_file(&target, &link) {
                Ok(()) => println!("file-symlink available"),
                Err(error) => println!("file-symlink unavailable {:?}", error.raw_os_error()),
            }
            let user = support::user_sid();
            let foreign = fixture.0.join("non-user-owner");
            match support::create_file(&foreign, &format!("O:BAD:P(A;;FA;;;{user})(A;;FA;;;SY)")) {
                Ok(file) => {
                    assert_ne!(support::security(&file).owner, user);
                    println!("non-user-owner available");
                }
                Err(error) => println!("non-user-owner unavailable {:?}", error.raw_os_error()),
            }
        }
        "restricted-current-user-denial" => {
            restricted_current_user_denial(path, std::path::Path::new(&args[3]))?
        }
        "restricted-read" => restricted_read(path, std::path::Path::new(&args[3]))?,
        "hold-lock" => {
            let file = secure_files::open_private_file(path, true)?;
            file.try_lock().map_err(io::Error::from)?;
            println!("locked");
            io::stdout().flush()?;
            let mut line = String::new();
            io::stdin().lock().read_line(&mut line)?;
            assert_eq!(line.trim(), "release");
            file.unlock()?;
        }
        "try-lock" => {
            let file = secure_files::open_private_file(path, true)?;
            match file.try_lock() {
                Ok(()) => {
                    println!("acquired");
                    file.unlock()?;
                }
                Err(std::fs::TryLockError::WouldBlock) => println!("blocked"),
                Err(error) => return Err(io::Error::from(error)),
            }
        }
        _ => return Err(io::Error::other("unknown probe mode")),
    }
    Ok(())
}
