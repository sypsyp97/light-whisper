use std::fs::File;
use std::io;
use std::path::Path;

#[cfg(test)]
thread_local! {
    static POST_CREATE_VALIDATION_ERROR: std::cell::RefCell<Option<io::Error>> = const {
        std::cell::RefCell::new(None)
    };
}

#[cfg(test)]
#[allow(dead_code)]
pub fn with_post_create_validation_error<T>(
    error: io::Error,
    operation: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    struct Restore(Option<io::Error>);
    impl Drop for Restore {
        fn drop(&mut self) {
            POST_CREATE_VALIDATION_ERROR.with(|slot| {
                slot.replace(self.0.take());
            });
        }
    }
    let previous = POST_CREATE_VALIDATION_ERROR.with(|slot| slot.replace(Some(error)));
    let _restore = Restore(previous);
    operation()
}

#[cfg(windows)]
pub fn create_private_file(path: &Path) -> io::Result<File> {
    windows::create_private_file(path)
}

#[cfg(windows)]
pub fn open_private_file(path: &Path, write: bool) -> io::Result<File> {
    windows::open_private_file(path, write)
}

#[cfg(windows)]
pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    windows::ensure_private_dir(path)
}

#[cfg(windows)]
pub fn publish_private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    windows::publish_private_file(path, bytes)
}

#[cfg(all(windows, test))]
#[allow(dead_code)]
pub fn publish_private_file_observed(
    path: &Path,
    bytes: &[u8],
    observe: &mut dyn FnMut(PublishPoint, &Path, &File),
) -> io::Result<()> {
    windows::publish_private_file_observed(path, bytes, observe)
}

#[cfg(not(windows))]
pub fn create_private_file(_path: &Path) -> io::Result<File> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(not(windows))]
pub fn open_private_file(_path: &Path, _write: bool) -> io::Result<File> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(not(windows))]
pub fn ensure_private_dir(_path: &Path) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(not(windows))]
pub fn publish_private_file(_path: &Path, _bytes: &[u8]) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(test)]
pub enum PublishPoint {
    BeforeFirstWrite,
    BeforeReplace,
}

#[cfg(all(not(windows), test))]
pub fn publish_private_file_observed(
    _path: &Path,
    _bytes: &[u8],
    _observe: &mut dyn FnMut(PublishPoint, &Path, &File),
) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

#[cfg(windows)]
mod windows {
    #[cfg(test)]
    use super::PublishPoint;
    use super::{io, File, Path};
    use rand::RngCore;
    use std::ffi::c_void;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::mem::{offset_of, size_of, zeroed};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::path::PathBuf;
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{
        LocalFree, ERROR_ALREADY_EXISTS, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, GENERIC_READ,
        GENERIC_WRITE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::{
        EqualSid, GetAce, GetSecurityDescriptorControl, GetSecurityDescriptorDacl,
        GetTokenInformation, IsValidAcl, IsValidSid, IsWellKnownSid, TokenUser, WinLocalSystemSid,
        ACCESS_ALLOWED_ACE, ACE_HEADER, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
        PSID, SECURITY_ATTRIBUTES, SE_DACL_PROTECTED, SID, TOKEN_QUERY, TOKEN_USER,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateDirectoryW, CreateFileW, FileRenameInfoEx, GetFileInformationByHandle, GetFileType,
        SetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, CREATE_NEW, DELETE,
        FILE_ALL_ACCESS, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_RENAME_INFO, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TYPE_DISK, READ_CONTROL,
    };
    use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    // winbase.h FILE_RENAME_INFO flags, available from Windows 10 RS1.
    // https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information
    const FILE_RENAME_FLAG_REPLACE_IF_EXISTS: u32 = 0x1;
    const FILE_RENAME_FLAG_POSIX_SEMANTICS: u32 = 0x2;

    struct LocalMemory(*mut c_void);

    impl Drop for LocalMemory {
        fn drop(&mut self) {
            // SAFETY: This owns a LocalAlloc result from a Windows security API.
            unsafe { LocalFree(self.0) };
        }
    }

    struct CurrentUser(Vec<usize>);

    impl CurrentUser {
        fn get() -> io::Result<Self> {
            let mut raw_token = null_mut();
            // SAFETY: The process pseudo-handle is valid and the output is writable.
            if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) } == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: OpenProcessToken returned one owned, valid token handle.
            let token = unsafe { OwnedHandle::from_raw_handle(raw_token) };
            let mut length = 0;
            // SAFETY: A null, zero-length buffer queries the required allocation size.
            let result = unsafe {
                GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut length)
            };
            let error = io::Error::last_os_error();
            if result != 0 || error.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32) {
                return Err(error);
            }
            if (length as usize) < size_of::<TOKEN_USER>() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid TokenUser length",
                ));
            }
            let mut data = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
            // SAFETY: usize storage has native pointer alignment and at least length bytes;
            // the token remains alive, and Windows initializes TOKEN_USER plus its SID.
            if unsafe {
                GetTokenInformation(
                    token.as_raw_handle(),
                    TokenUser,
                    data.as_mut_ptr().cast(),
                    length,
                    &mut length,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(data))
        }

        fn sid(&self) -> PSID {
            // SAFETY: get initialized an aligned TOKEN_USER; its embedded SID points
            // into this allocation, which is retained for every use of the pointer.
            unsafe { (*(self.0.as_ptr().cast::<TOKEN_USER>())).User.Sid }
        }

        fn descriptor(&self) -> io::Result<LocalMemory> {
            let mut text = null_mut();
            // SAFETY: The SID belongs to the live TokenUser buffer; output is writable.
            if unsafe { ConvertSidToStringSidW(self.sid(), &mut text) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let _text_memory = LocalMemory(text.cast());
            let mut length = 0;
            // SAFETY: Successful conversion returns a NUL-terminated UTF-16 allocation,
            // retained by _text_memory until after the text has been copied.
            let user = unsafe {
                while *text.add(length) != 0 {
                    length += 1;
                }
                String::from_utf16(std::slice::from_raw_parts(text, length))
            }
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            let sddl: Vec<_> = format!("O:{user}D:P(A;;FA;;;{user})(A;;FA;;;SY)")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let mut descriptor = null_mut();
            // SAFETY: sddl is terminated and alive for the call; descriptor is writable.
            if unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(LocalMemory(descriptor))
        }
    }

    fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
        let mut wide: Vec<_> = path.as_os_str().encode_wide().collect();
        if wide.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path contains NUL",
            ));
        }
        wide.push(0);
        Ok(wide)
    }

    fn reject() -> io::Error {
        io::Error::new(io::ErrorKind::PermissionDenied, "object is not private")
    }

    fn validate(file: &File, user: &CurrentUser, directory: bool) -> io::Result<()> {
        // SAFETY: This C output structure permits an all-zero initial value.
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { zeroed() };
        // SAFETY: The borrowed handle is live and info is a writable output structure.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: The borrowed handle remains live for this query.
        if unsafe { GetFileType(file.as_raw_handle()) } != FILE_TYPE_DISK
            || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
        {
            return Err(reject());
        }
        let mut owner = null_mut();
        let mut dacl = null_mut();
        let mut descriptor = null_mut();
        // SAFETY: Query the same live handle returned to the caller. All output pointers
        // are writable; Windows allocates descriptor and its embedded owner/DACL.
        let status = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut dacl,
                null_mut(),
                &mut descriptor,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let descriptor = LocalMemory(descriptor);
        let mut control = 0;
        let mut revision = 0;
        let mut present = 0;
        let mut defaulted = 0;
        // SAFETY: The successful query returned this live descriptor; every output
        // is writable. Owner and DACL remain inside its LocalAlloc allocation.
        unsafe {
            if GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) == 0
                || GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted)
                    == 0
            {
                return Err(io::Error::last_os_error());
            }
            if owner.is_null()
                || IsValidSid(owner) == 0
                || EqualSid(owner, user.sid()) == 0
                || control & SE_DACL_PROTECTED == 0
                || present == 0
                || dacl.is_null()
                || IsValidAcl(dacl) == 0
            {
                return Err(reject());
            }
            let mut grants_user = false;
            for index in 0..(*dacl).AceCount {
                let mut ace = null_mut();
                if GetAce(dacl, u32::from(index), &mut ace) == 0 {
                    return Err(io::Error::last_os_error());
                }
                // IsValidAcl/GetAce guarantee an aligned ACE_HEADER within the DACL.
                let header = &*ace.cast::<ACE_HEADER>();
                let sid_offset = offset_of!(ACCESS_ALLOWED_ACE, SidStart);
                let sid_header = offset_of!(SID, SubAuthority);
                if u32::from(header.AceType) != ACCESS_ALLOWED_ACE_TYPE
                    || header.AceFlags != 0
                    || usize::from(header.AceSize) < sid_offset + sid_header
                {
                    return Err(reject());
                }
                let allow = &*ace.cast::<ACCESS_ALLOWED_ACE>();
                let sid = ace.cast::<u8>().add(sid_offset);
                // The checked fixed SID header includes SubAuthorityCount. Check its
                // variable tail before letting Windows examine the complete SID.
                let count = *sid.add(offset_of!(SID, SubAuthorityCount));
                if sid_offset + sid_header + usize::from(count) * size_of::<u32>()
                    > usize::from(header.AceSize)
                    || IsValidSid(sid.cast()) == 0
                    || allow.Mask != FILE_ALL_ACCESS
                {
                    return Err(reject());
                }
                let is_user = EqualSid(sid.cast(), user.sid()) != 0;
                if !is_user && IsWellKnownSid(sid.cast(), WinLocalSystemSid) == 0 {
                    return Err(reject());
                }
                grants_user |= is_user;
            }
            if !grants_user {
                return Err(reject());
            }
        }
        Ok(())
    }

    pub fn create_private_file(path: &Path) -> io::Result<File> {
        let wide = wide_path(path)?;
        let user = CurrentUser::get()?;
        let descriptor = user.descriptor()?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        // SAFETY: The terminated path and descriptor outlive this call. CREATE_NEW
        // applies the explicit private descriptor before any file bytes can exist.
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE | READ_CONTROL | DELETE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                &attributes,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut cleanup = StagingPath {
            path: path.to_owned(),
            owned: true,
        };
        // SAFETY: Successful CreateFileW transfers this unique handle to File.
        // The cleanup guard precedes File so failures close the handle first.
        let file = unsafe { File::from_raw_handle(handle) };
        #[cfg(test)]
        if let Some(error) = super::POST_CREATE_VALIDATION_ERROR.with(|slot| slot.take()) {
            return Err(error);
        }
        validate(&file, &user, false)?;
        cleanup.owned = false;
        Ok(file)
    }

    pub fn open_private_file(path: &Path, write: bool) -> io::Result<File> {
        wide_path(path)?;
        let user = CurrentUser::get()?;
        let file = OpenOptions::new()
            .read(true)
            .write(write)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        validate(&file, &user, false)?;
        Ok(file)
    }

    pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
        let wide = wide_path(path)?;
        let user = CurrentUser::get()?;
        let descriptor = user.descriptor()?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        // SAFETY: The terminated path and explicit descriptor remain live for creation.
        if unsafe { CreateDirectoryW(wide.as_ptr(), &attributes) } == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_ALREADY_EXISTS as i32) {
                return Err(error);
            }
        }
        let directory = OpenOptions::new()
            .access_mode(READ_CONTROL)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        validate(&directory, &user, true)
    }

    struct StagingPath {
        path: PathBuf,
        owned: bool,
    }

    impl Drop for StagingPath {
        fn drop(&mut self) {
            if self.owned {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }

    pub fn publish_private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
        #[cfg(test)]
        return publish(path, bytes, None);
        #[cfg(not(test))]
        publish(path, bytes)
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub fn publish_private_file_observed(
        path: &Path,
        bytes: &[u8],
        observe: &mut dyn FnMut(PublishPoint, &Path, &File),
    ) -> io::Result<()> {
        publish(path, bytes, Some(observe))
    }

    fn publish(
        path: &Path,
        bytes: &[u8],
        #[cfg(test)] mut observe: Option<&mut dyn FnMut(PublishPoint, &Path, &File)>,
    ) -> io::Result<()> {
        let absolute = std::path::absolute(path)?;
        let destination = wide_path(&absolute)?;
        let parent = absolute
            .parent()
            .filter(|_| path.file_name().is_some())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "destination must name a file")
            })?;
        let mut nonce = [0u8; 16];
        rand::rngs::OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(io::Error::other)?;
        let mut staging = StagingPath {
            path: parent.join(format!(
                ".light-whisper-{:032x}.tmp",
                u128::from_ne_bytes(nonce)
            )),
            owned: false,
        };
        let name_length = (destination.len() - 1)
            .checked_mul(size_of::<u16>())
            .and_then(|length| u32::try_from(length).ok())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "destination is too long")
            })?;
        let info_length = offset_of!(FILE_RENAME_INFO, FileName)
            .checked_add(name_length as usize)
            .and_then(|length| length.checked_add(size_of::<u16>()))
            .map(|length| length.max(size_of::<FILE_RENAME_INFO>()))
            .and_then(|length| u32::try_from(length).ok())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "destination is too long")
            })?;
        let mut rename_buffer = vec![0usize; (info_length as usize).div_ceil(size_of::<usize>())];
        // SAFETY: usize storage provides FILE_RENAME_INFO's native pointer alignment.
        // Checked lengths cover its fixed structure and the complete absolute UTF-16
        // path, including NUL; FileNameLength counts only the path bytes before NUL.
        unsafe {
            let info = rename_buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
            (*info).Anonymous.Flags =
                FILE_RENAME_FLAG_REPLACE_IF_EXISTS | FILE_RENAME_FLAG_POSIX_SEMANTICS;
            (*info).RootDirectory = null_mut();
            (*info).FileNameLength = name_length;
            std::ptr::copy_nonoverlapping(
                destination.as_ptr(),
                rename_buffer
                    .as_mut_ptr()
                    .cast::<u8>()
                    .add(offset_of!(FILE_RENAME_INFO, FileName))
                    .cast::<u16>(),
                destination.len(),
            );
        }
        // Declare the file after its cleanup guard so error/unwind drops close the
        // non-delete-sharing handle before removing only this operation's stage.
        let mut file = create_private_file(&staging.path)?;
        staging.owned = true;
        #[cfg(test)]
        if let Some(callback) = observe.as_mut() {
            callback(PublishPoint::BeforeFirstWrite, &staging.path, &file);
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        #[cfg(test)]
        if let Some(callback) = observe.as_mut() {
            callback(PublishPoint::BeforeReplace, &staging.path, &file);
        }
        // SAFETY: The checked, aligned rename buffer remains alive. The stage handle
        // owns DELETE access and stays open until after this atomic same-directory
        // replacement, which moves its private object/DACL and preserves old handles.
        if unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FileRenameInfoEx,
                rename_buffer.as_ptr().cast(),
                info_length,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        staging.owned = false;
        Ok(())
    }
}
