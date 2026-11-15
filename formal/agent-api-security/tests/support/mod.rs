#![allow(dead_code)]

use std::ffi::c_void;
use std::fs::File;
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::Authorization::*;
use windows_sys::Win32::Security::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;
use windows_sys::Win32::System::Threading::*;

pub fn wide(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

pub struct Handle(pub HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

pub fn process_token() -> io::Result<Handle> {
    let mut token = null_mut();
    if unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_IMPERSONATE,
            &mut token,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(Handle(token))
}

pub unsafe fn sid_text(sid: PSID) -> String {
    let mut text = null_mut();
    assert_ne!(ConvertSidToStringSidW(sid, &mut text), 0);
    let mut len = 0;
    while *text.add(len) != 0 {
        len += 1;
    }
    let result = String::from_utf16(std::slice::from_raw_parts(text, len)).unwrap();
    LocalFree(text as *mut c_void);
    result
}

pub fn user_sid() -> String {
    let token = process_token().unwrap();
    let mut len = 0;
    unsafe {
        GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut len);
        // usize storage gives the TOKEN_USER and embedded pointers native alignment.
        let mut data = vec![0usize; (len as usize).div_ceil(size_of::<usize>())];
        assert_ne!(
            GetTokenInformation(token.0, TokenUser, data.as_mut_ptr().cast(), len, &mut len),
            0
        );
        sid_text((*(data.as_ptr().cast::<TOKEN_USER>())).User.Sid)
    }
}

pub struct Descriptor(pub PSECURITY_DESCRIPTOR);
impl Descriptor {
    pub fn parse(sddl: &str) -> io::Result<Self> {
        let mut descriptor = null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide(sddl).as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(descriptor))
    }

    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0,
            bInheritHandle: 0,
        }
    }
}
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0) };
    }
}

pub fn private_sddl() -> String {
    let user = user_sid();
    format!("O:{user}D:P(A;;FA;;;{user})(A;;FA;;;SY)")
}

pub fn create_file(path: &Path, sddl: &str) -> io::Result<File> {
    let descriptor = Descriptor::parse(sddl)?;
    let handle = unsafe {
        CreateFileW(
            wide(path).as_ptr(),
            GENERIC_READ | GENERIC_WRITE | READ_CONTROL | WRITE_DAC,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &descriptor.attributes(),
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_handle(handle) })
    }
}

pub fn create_dir(path: &Path, sddl: &str) -> io::Result<()> {
    let descriptor = Descriptor::parse(sddl)?;
    if unsafe { CreateDirectoryW(wide(path).as_ptr(), &descriptor.attributes()) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub fn make_dacl_unprotected(file: &File, sddl: &str) {
    let descriptor = Descriptor::parse(sddl).unwrap();
    let mut present = 0;
    let mut defaulted = 0;
    let mut dacl = null_mut();
    unsafe {
        assert_ne!(
            GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted),
            0
        );
        assert_eq!(
            SetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | UNPROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                dacl,
                null()
            ),
            ERROR_SUCCESS
        );
    }
}

#[derive(Debug)]
pub struct Security {
    pub owner: String,
    pub control: u16,
    pub dacl_present: bool,
    pub dacl_null: bool,
    pub aces: Vec<(u8, u8, u32, String)>,
}

pub fn security(file: &File) -> Security {
    unsafe {
        let mut owner = null_mut();
        let mut dacl = null_mut();
        let mut descriptor = null_mut();
        assert_eq!(
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut dacl,
                null_mut(),
                &mut descriptor,
            ),
            ERROR_SUCCESS
        );
        let descriptor = Descriptor(descriptor);
        let mut control = 0;
        let mut revision = 0;
        assert_ne!(
            GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision),
            0
        );
        let mut present = 0;
        let mut defaulted = 0;
        assert_ne!(
            GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted),
            0
        );
        let mut aces = Vec::new();
        if !dacl.is_null() {
            for index in 0..(*dacl).AceCount {
                let mut ace = null_mut();
                assert_ne!(GetAce(dacl, u32::from(index), &mut ace), 0);
                let header = &*(ace.cast::<ACE_HEADER>());
                // Fixtures and the allowed production contract contain simple allow ACEs.
                // Assert type before interpreting the simple ACE layout.
                assert_eq!(u32::from(header.AceType), ACCESS_ALLOWED_ACE_TYPE);
                let allow = &*(ace.cast::<ACCESS_ALLOWED_ACE>());
                aces.push((
                    header.AceType,
                    header.AceFlags,
                    allow.Mask,
                    sid_text(std::ptr::addr_of!(allow.SidStart) as PSID),
                ));
            }
        }
        Security {
            owner: sid_text(owner),
            control,
            dacl_present: present != 0,
            dacl_null: dacl.is_null(),
            aces,
        }
    }
}

pub fn assert_private(file: &File) {
    let observed = security(file);
    let user = user_sid();
    assert_eq!(observed.owner, user, "owner must be TokenUser SID");
    assert!(
        observed.dacl_present && !observed.dacl_null,
        "DACL must exist: {observed:?}"
    );
    assert_ne!(
        observed.control & SE_DACL_PROTECTED,
        0,
        "DACL must be protected: {observed:?}"
    );
    assert!(
        !observed.aces.is_empty(),
        "DACL must grant its owner access"
    );
    for (_, flags, mask, sid) in &observed.aces {
        assert_eq!(*flags, 0, "file ACE must not inherit or propagate");
        assert_eq!(
            *mask, FILE_ALL_ACCESS,
            "private allow ACE must grant full access"
        );
        assert!(
            sid == &user || sid == "S-1-5-18",
            "unexpected trustee: {observed:?}"
        );
    }
    assert!(observed.aces.iter().any(|(_, _, _, sid)| sid == &user));
    let mut flags = 0;
    assert_ne!(
        unsafe { GetHandleInformation(file.as_raw_handle(), &mut flags) },
        0
    );
    assert_eq!(
        flags & HANDLE_FLAG_INHERIT,
        0,
        "handle must not be inheritable"
    );
}

pub fn open_directory(path: &Path) -> File {
    let handle = unsafe {
        CreateFileW(
            wide(path).as_ptr(),
            READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    assert_ne!(
        handle,
        INVALID_HANDLE_VALUE,
        "directory inspection: {}",
        io::Error::last_os_error()
    );
    unsafe { File::from_raw_handle(handle) }
}

pub fn assert_plain_file(file: &File) {
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
    assert_ne!(
        unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) },
        0
    );
    assert_eq!(
        info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY),
        0
    );
}

pub fn file_identity(file: &File) -> (u32, u32, u32) {
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
    assert_ne!(
        unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) },
        0
    );
    (
        info.dwVolumeSerialNumber,
        info.nFileIndexHigh,
        info.nFileIndexLow,
    )
}

pub struct Fixture(pub PathBuf);
impl Fixture {
    pub fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lw-private-verification-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let user = user_sid();
        create_dir(
            &path,
            &format!("O:{user}D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;WD)"),
        )
        .unwrap();
        Self(path)
    }

    pub fn private_dir(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        create_dir(&path, &private_sddl()).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Only the exact random directory created by this fixture is owned here.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
