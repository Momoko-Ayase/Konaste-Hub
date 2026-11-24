//! Directory and file handles that stay open for the session.
//!
//! Ancestors are pinned top-down before a child path is created. A directory is
//! opened once with `FILE_OPEN_REPARSE_POINT` and rejected when that handle is
//! a reparse point. That same handle is the usable handle: it is not reopened
//! by file id, and there is no path fallback. Directory handles share read and
//! write (not delete), so `NtSetInformationFile(FileRenameInformation)` into
//! the pinned parent does not return `STATUS_SHARING_VIOLATION` and the
//! directory cannot be renamed away. Write sharing is the remaining way to set
//! a reparse point; child opens still use `FILE_OPEN_REPARSE_POINT` and reject
//! that attribute before use.
//!
//! Child directories are opened the same way, relative to the pinned parent.
//! They are never opened by a full path, so a junction cannot be followed and
//! then accepted because its target looks ordinary.
//!
//! Regular files share read only, so a handle used for hashing denies writers.
//! The session lock is the exception and shares read and write. Files that will
//! be renamed are opened with `DELETE` and `FILE_READ_DATA`, so the same
//! no-follow handle can be hashed immediately before the rename. Replacement
//! uses `NtSetInformationFile(FileRenameInformation)` with the pinned parent as
//! `RootDirectory`.

use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::{Component, Path, PathBuf};

use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::{
    FILE_CREATE, FILE_DIRECTORY_FILE, FILE_DIRECTORY_INFORMATION, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT,
    FILE_RENAME_INFORMATION, FILE_SYNCHRONOUS_IO_NONALERT, FileDirectoryInformation, FileRenameInformation, NtCreateFile,
    NtQueryDirectoryFile, NtSetInformationFile,
};
use windows_sys::Win32::Foundation::{
    DUPLICATE_SAME_ACCESS, DuplicateHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE, OBJ_CASE_INSENSITIVE, STATUS_SUCCESS,
    UNICODE_STRING,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ADD_FILE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_BEGIN, FILE_DISPOSITION_INFO, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FILE_TRAVERSE, FILE_WRITE_DATA, FileDispositionInfo, FlushFileBuffers, GetFileInformationByHandle, LOCKFILE_EXCLUSIVE_LOCK,
    LOCKFILE_FAIL_IMMEDIATELY, LockFileEx, ReadFile, SYNCHRONIZE, SetFileInformationByHandle, SetFilePointerEx, WriteFile,
};
use windows_sys::Win32::System::IO::OVERLAPPED;
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use crate::error::UpdateError;

const STATUS_NO_MORE_FILES: i32 = 0x8000_0006_u32 as i32;
const STATUS_NO_SUCH_FILE: i32 = 0xC000_000F_u32 as i32;
const STATUS_OBJECT_NAME_NOT_FOUND: i32 = 0xC000_0034_u32 as i32;
const STATUS_OBJECT_PATH_NOT_FOUND: i32 = 0xC000_003A_u32 as i32;

/// Ancestors, including a volume root that denies create access.
const DIR_READ: u32 = FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE;
/// The installation root and the work directory. Child creation was verified
/// through a no-follow handle opened with these rights.
const DIR_CREATE: u32 = DIR_READ | FILE_ADD_FILE;
const READ_ACCESS: u32 = FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE;
const WRITE_ACCESS: u32 = FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | DELETE | SYNCHRONIZE;
const CREATE_ACCESS: u32 = FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | DELETE | SYNCHRONIZE;
/// Delete plus read, so the same no-follow handle can be hashed immediately
/// before `NtSetInformationFile(FileRenameInformation)`.
const RENAME_ACCESS: u32 = DELETE | FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE;
const FILE_OPTS: u32 = FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT;
const DIR_OPTS: u32 = FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT;
/// Read and write. NtSetInformationFile(FileRenameInformation) into this parent
/// returns STATUS_SHARING_VIOLATION unless write is shared. Delete is not
/// shared, so the directory cannot be renamed away. Write sharing is the
/// remaining way to set a reparse point; child opens still use
/// FILE_OPEN_REPARSE_POINT and reject that attribute before use.
const DIR_SHARE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE;
/// Regular files deny other writers. Directory handles use [`DIR_SHARE`].
const FILE_SHARE: u32 = FILE_SHARE_READ;

/// One installation's directory handles. Ancestors, the root, the work
/// directory, and every directory created under them stay here until the
/// session drops. Operations duplicate a handle instead of opening a path.
pub(crate) struct Lease {
    path: PathBuf,
    /// No-follow probes, kept so a junction cannot replace a pinned directory.
    probes: Vec<OwnedHandle>,
    /// Usable directory handles. Index `root` is the installation root; later
    /// entries are the work directory and directories created under it.
    usable: Vec<OwnedHandle>,
    root: usize,
    work: usize,
    serial: u32,
    directories: std::collections::BTreeMap<PathBuf, usize>,
    work_name: Option<OsString>,
}

impl Lease {
    /// Pin `root`, which must already exist, then create `work_name` inside it
    /// before returning. The root pin is not dropped between those steps.
    pub(crate) fn open(root: &Path, work_name: &str) -> Result<Self, UpdateError> {
        let absolute = require_absolute(root)?;
        let mut probes = Vec::new();
        let mut usable = Vec::new();
        let mut serial = None;
        let chain = ancestry(&absolute)?;
        for (index, directory) in chain.iter().enumerate() {
            let access = if index + 1 == chain.len() { DIR_CREATE } else { DIR_READ };
            let (probe, handle, volume) = open_directory_path(directory, serial, access)?;
            serial = Some(volume);
            probes.push(probe);
            usable.push(handle);
        }
        let serial = serial.ok_or_else(|| UpdateError::Root("root has no directory to pin".into()))?;
        let root = usable.len() - 1;
        let mut lease = Self {
            path: absolute,
            probes,
            usable,
            root,
            work: root,
            serial,
            directories: [(PathBuf::new(), root)].into(),
            work_name: None,
        };
        lease.descend(&[work_name.to_string()])?;
        lease.work = lease.directories[Path::new(work_name)];
        lease.work_name = Some(work_name.into());
        Ok(lease)
    }

    /// Pin `root` and refuse other children before opening the work directory.
    pub(crate) fn open_empty(root: &Path, work_name: &str, allow_work: bool) -> Result<Self, UpdateError> {
        let lease = Self::open_existing(root)?;
        let children = list_children(lease.root())?;
        if children.iter().any(|name| !allow_work || name != std::ffi::OsStr::new(work_name)) {
            return Err(UpdateError::Root(format!("{} is not an empty directory", root.display())));
        }
        let mut lease = lease;
        lease.descend(&[work_name.to_string()])?;
        lease.work = lease.directories[Path::new(work_name)];
        lease.work_name = Some(work_name.into());
        Ok(lease)
    }

    pub(crate) fn open_existing(root: &Path) -> Result<Self, UpdateError> {
        let absolute = require_absolute(root)?;
        let mut probes = Vec::new();
        let mut usable = Vec::new();
        let mut serial = None;
        for directory in ancestry(&absolute)? {
            let (probe, handle, volume) = open_directory_path(&directory, serial, DIR_READ)?;
            serial = Some(volume);
            probes.push(probe);
            usable.push(handle);
        }
        let root = usable.len().saturating_sub(1);
        Ok(Self {
            path: absolute,
            serial: serial.ok_or_else(|| UpdateError::Root("root has no directory to pin".into()))?,
            probes,
            usable,
            root,
            work: root,
            directories: [(PathBuf::new(), root)].into(),
            work_name: None,
        })
    }

    pub(crate) fn serial(&self) -> u32 {
        self.serial
    }

    /// Installation root.
    pub(crate) fn root(&self) -> &OwnedHandle {
        &self.usable[self.root]
    }

    pub(crate) fn open_path(&mut self, path: &Path) -> Result<Option<OpenedFile>, UpdateError> {
        let relative = path
            .strip_prefix(&self.path)
            .map_err(|_| UpdateError::path(path.display().to_string(), "file is outside the pinned installation"))?;
        let mut parts = Vec::new();
        for part in relative.components() {
            match part {
                Component::Normal(name) => parts.push(name.to_owned()),
                _ => return Err(UpdateError::path("", "file path is not relative")),
            }
        }
        let name = parts.pop().ok_or_else(|| UpdateError::path("", "file has no name"))?;
        let Some(parent) = self.walk(&parts, false)? else {
            return Ok(None);
        };
        open_file_read(&parent, &name)
    }

    pub(crate) fn work(&self) -> Result<OwnedHandle, UpdateError> {
        duplicate(&self.usable[self.work])
    }

    /// Pin `parts` as directories under the root. Missing parts are created
    /// from the pinned parent. The deepest usable handle is returned duplicated.
    pub(crate) fn descend(&mut self, parts: &[String]) -> Result<OwnedHandle, UpdateError> {
        let names: Vec<OsString> = parts.iter().map(OsString::from).collect();
        self.walk(&names, true)?.ok_or_else(|| UpdateError::path("", "created directory is missing"))
    }

    /// `parts` start at the work directory rather than the installation root.
    pub(crate) fn descend_from_work(&mut self, parts: &[String]) -> Result<OwnedHandle, UpdateError> {
        let mut names: Vec<OsString> = self.work_name.iter().cloned().collect();
        names.extend(parts.iter().map(OsString::from));
        self.walk(&names, true)?.ok_or_else(|| UpdateError::path("", "created work directory is missing"))
    }

    fn walk(&mut self, parts: &[OsString], create: bool) -> Result<Option<OwnedHandle>, UpdateError> {
        let mut parent = duplicate(self.root())?;
        if information(&parent)?.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(UpdateError::path("", "pinned root became a reparse point"));
        }
        let mut key = PathBuf::new();
        for part in parts {
            single_segment(part)?;
            key.push(part);
            if let Some(index) = self.directories.get(&key).copied() {
                parent = duplicate(&self.usable[index])?;
                if information(&parent)?.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    return Err(UpdateError::path(key.display().to_string(), "pinned directory became a reparse point"));
                }
                continue;
            }
            let opened = open_directory_child(&parent, part, DIR_READ)?;
            let (probe, handle) = match opened {
                Some(pair) => pair,
                None if create => {
                    let created = create_directory_child(&parent, part)?;
                    (duplicate(&created)?, created)
                }
                None => return Ok(None),
            };
            self.accept(probe, handle, &key)?;
            let index = self.usable.len() - 1;
            self.directories.insert(key.clone(), index);
            parent = duplicate(&self.usable[index])?;
        }
        Ok(Some(parent))
    }

    fn accept(&mut self, probe: OwnedHandle, handle: OwnedHandle, label: &Path) -> Result<(), UpdateError> {
        let info = information(&handle)?;
        if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(UpdateError::path(label.display().to_string(), "refusing a reparse point"));
        }
        if info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
            return Err(UpdateError::path(label.display().to_string(), "pinned path is not a directory"));
        }
        if info.dwVolumeSerialNumber != self.serial {
            return Err(UpdateError::Root(format!("{} is not on the installation volume", label.display())));
        }
        let probe_info = information(&probe)?;
        if probe_info.nFileIndexHigh != info.nFileIndexHigh || probe_info.nFileIndexLow != info.nFileIndexLow {
            return Err(UpdateError::path(label.display().to_string(), "directory open followed a reparse point"));
        }
        self.probes.push(probe);
        self.usable.push(handle);
        Ok(())
    }
}

pub(crate) struct OpenedFile {
    handle: OwnedHandle,
    size: u64,
    serial: u32,
}

impl OpenedFile {
    pub(crate) fn size(&self) -> u64 {
        self.size
    }

    pub(crate) fn serial(&self) -> u32 {
        self.serial
    }

    pub(crate) fn handle(&self) -> &OwnedHandle {
        &self.handle
    }

    pub(crate) fn into_handle(self) -> OwnedHandle {
        self.handle
    }

    /// Hash this handle from the start. The file position is left at the end.
    /// `on_bytes` receives the running total after each chunk and must not do IO.
    pub(crate) fn hash_with(&mut self, mut on_bytes: impl FnMut(u64)) -> Result<[u8; 32], UpdateError> {
        use sha2::{Digest, Sha256};
        rewind(&self.handle)?;
        let mut hasher = Sha256::new();
        let mut buf = [0_u8; 64 * 1024];
        let mut total = 0_u64;
        loop {
            let n = read_at(&self.handle, &mut buf)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > self.size {
                return Err(UpdateError::path("", "pinned file grew while hashing"));
            }
            hasher.update(&buf[..n]);
            on_bytes(total);
            #[cfg(test)]
            HASH_HOOK.with(|hook| {
                if let Some(hook) = hook.borrow().as_ref() {
                    hook(n as u64);
                }
            });
        }
        if total != self.size {
            return Err(UpdateError::path("", "pinned file shrank while hashing"));
        }
        Ok(hasher.finalize().into())
    }
}

#[cfg(test)]
type HashHook = Box<dyn Fn(u64)>;

#[cfg(test)]
std::thread_local! {
    static HASH_HOOK: std::cell::RefCell<Option<HashHook>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn with_hash_hook<T>(hook: impl Fn(u64) + 'static, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<HashHook>);
    impl Drop for Restore {
        fn drop(&mut self) {
            HASH_HOOK.with(|slot| {
                slot.replace(self.0.take());
            });
        }
    }
    let _restore = Restore(HASH_HOOK.with(|slot| slot.replace(Some(Box::new(hook)))));
    run()
}

impl Read for OpenedFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        read_at(&self.handle, buf).map_err(update_to_io)
    }
}

impl Write for OpenedFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        write_at(&self.handle, buf).map_err(update_to_io)
    }

    fn flush(&mut self) -> io::Result<()> {
        flush(&self.handle).map_err(update_to_io)
    }
}

impl Seek for OpenedFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        seek(&self.handle, pos).map_err(update_to_io)
    }
}

pub(crate) fn open_file_read(parent: &OwnedHandle, name: &OsStr) -> Result<Option<OpenedFile>, UpdateError> {
    open_file(parent, name, READ_ACCESS)
}

pub(crate) fn open_file_rename(parent: &OwnedHandle, name: &OsStr) -> Result<Option<OpenedFile>, UpdateError> {
    open_file(parent, name, RENAME_ACCESS)
}

pub(crate) fn open_file_write(parent: &OwnedHandle, name: &OsStr) -> Result<Option<OpenedFile>, UpdateError> {
    open_file(parent, name, WRITE_ACCESS)
}

pub(crate) fn create_file(parent: &OwnedHandle, name: &OsStr) -> Result<OwnedHandle, UpdateError> {
    create_child(parent, name, CREATE_ACCESS, FILE_NON_DIRECTORY_FILE | FILE_OPTS, FILE_ATTRIBUTE_NORMAL)
}

pub(crate) fn write_all(handle: &OwnedHandle, bytes: &[u8]) -> Result<(), UpdateError> {
    let mut offset = 0;
    while offset < bytes.len() {
        let n = write_at(handle, &bytes[offset..])?;
        if n == 0 {
            return Err(io_error("short write"));
        }
        offset += n;
    }
    Ok(())
}

pub(crate) fn read_exact(handle: &OwnedHandle, size: u64) -> Result<Vec<u8>, UpdateError> {
    rewind(handle)?;
    let mut out = Vec::new();
    let mut buf = [0_u8; 64 * 1024];
    loop {
        let n = read_at(handle, &mut buf)?;
        if n == 0 {
            break;
        }
        if out.len() as u64 + n as u64 > size {
            return Err(UpdateError::path("", "pinned file grew while reading"));
        }
        out.extend_from_slice(&buf[..n]);
    }
    if out.len() as u64 != size {
        return Err(UpdateError::path("", "pinned file shrank while reading"));
    }
    Ok(out)
}

pub(crate) fn flush(handle: &OwnedHandle) -> Result<(), UpdateError> {
    let ok = unsafe { FlushFileBuffers(handle.as_raw_handle() as HANDLE) };
    if ok == 0 { Err(io_error("flush")) } else { Ok(()) }
}

pub(crate) fn seek(handle: &OwnedHandle, pos: SeekFrom) -> Result<u64, UpdateError> {
    let (method, distance) = match pos {
        SeekFrom::Start(offset) => (FILE_BEGIN, offset as i64),
        SeekFrom::Current(offset) => (windows_sys::Win32::Storage::FileSystem::FILE_CURRENT, offset),
        SeekFrom::End(offset) => (windows_sys::Win32::Storage::FileSystem::FILE_END, offset),
    };
    let mut new_pos = 0_i64;
    let ok = unsafe { SetFilePointerEx(handle.as_raw_handle() as HANDLE, distance, &mut new_pos, method) };
    if ok == 0 { Err(io_error("seek")) } else { Ok(new_pos as u64) }
}

fn rewind(handle: &OwnedHandle) -> Result<(), UpdateError> {
    seek(handle, SeekFrom::Start(0)).map(|_| ())
}

pub(crate) fn rename_into(file: &OwnedHandle, parent: &OwnedHandle, name: &OsStr, replace: bool) -> Result<(), UpdateError> {
    let wide = single_segment(name)?;
    let name_bytes = wide.len() * std::mem::size_of::<u16>();
    let total = std::mem::size_of::<FILE_RENAME_INFORMATION>() + name_bytes.saturating_sub(std::mem::size_of::<u16>());
    // Win32 FileRenameInfo rejects a non-null RootDirectory. The NT call accepts
    // the pinned parent. The buffer is u64-aligned and holds the struct plus name.
    let mut buffer = vec![0_u64; total.div_ceil(8)];
    let bytes = buffer.len() * 8;
    let info = FILE_RENAME_INFORMATION {
        Anonymous: windows_sys::Wdk::Storage::FileSystem::FILE_RENAME_INFORMATION_0 { ReplaceIfExists: replace },
        RootDirectory: parent.as_raw_handle() as HANDLE,
        FileNameLength: name_bytes as u32,
        FileName: [0],
    };
    // SAFETY: `buffer` is aligned and sized for the struct plus the wide name.
    // The name contains no NUL or separator. `file` was opened with DELETE.
    unsafe {
        let raw = buffer.as_mut_ptr().cast::<u8>();
        std::ptr::write(raw.cast::<FILE_RENAME_INFORMATION>(), info);
        let name_at = std::mem::offset_of!(FILE_RENAME_INFORMATION, FileName);
        std::ptr::copy_nonoverlapping(wide.as_ptr().cast::<u8>(), raw.add(name_at), name_bytes);
        let mut io = windows_sys::Win32::System::IO::IO_STATUS_BLOCK::default();
        let status = NtSetInformationFile(file.as_raw_handle() as HANDLE, &mut io, raw.cast(), bytes as u32, FileRenameInformation);
        if status != STATUS_SUCCESS {
            return Err(nt_error(&format!("rename {}", name.to_string_lossy()), status));
        }
    }
    Ok(())
}

pub(crate) fn delete_file(parent: &OwnedHandle, name: &OsStr) -> Result<(), UpdateError> {
    let Some(file) = open_file(parent, name, DELETE | SYNCHRONIZE | FILE_READ_ATTRIBUTES)? else {
        return Ok(());
    };
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    let ok = unsafe {
        SetFileInformationByHandle(
            file.handle.as_raw_handle() as HANDLE,
            FileDispositionInfo,
            &info as *const FILE_DISPOSITION_INFO as *const _,
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if ok == 0 { Err(io_error("delete")) } else { Ok(()) }
}

pub(crate) fn child_missing(parent: &OwnedHandle, name: &OsStr) -> Result<bool, UpdateError> {
    Ok(open_file_read(parent, name)?.is_none())
}

pub(crate) fn try_lock_exclusive(handle: &OwnedHandle) -> Result<(), UpdateError> {
    let mut overlapped = OVERLAPPED::default();
    let ok = unsafe {
        LockFileEx(
            handle.as_raw_handle() as HANDLE,
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            u32::MAX,
            u32::MAX,
            &mut overlapped,
        )
    };
    if ok == 0 {
        Err(UpdateError::state(format!("installation is already open by another updater session: {}", io::Error::last_os_error())))
    } else {
        Ok(())
    }
}

pub(crate) fn open_lock(parent: &OwnedHandle, name: &OsStr) -> Result<OwnedHandle, UpdateError> {
    // The lock file is the one regular file that shares write. Everything else
    // opened as a non-directory shares read only.
    let share = FILE_SHARE_READ | FILE_SHARE_WRITE;
    let wide = single_segment(name)?;
    match nt_create(Some(parent.as_raw_handle()), &wide, READ_ACCESS, FILE_OPEN, FILE_NON_DIRECTORY_FILE | FILE_OPTS, 0, share) {
        Ok(handle) => {
            accept_file(handle.try_clone().map_err(UpdateError::Io)?, name)?;
            Ok(handle)
        }
        Err(OpenError::Missing(_)) => nt_create(
            Some(parent.as_raw_handle()),
            &wide,
            READ_ACCESS,
            FILE_CREATE,
            FILE_NON_DIRECTORY_FILE | FILE_OPTS,
            FILE_ATTRIBUTE_NORMAL,
            share,
        )
        .map_err(OpenError::into_update),
        Err(err) => Err(err.into_update()),
    }
}

pub(crate) fn list_children(directory: &OwnedHandle) -> Result<Vec<OsString>, UpdateError> {
    let mut out = Vec::new();
    let mut buffer = vec![0_u8; 4096];
    let mut restart = true;
    loop {
        let mut io = windows_sys::Win32::System::IO::IO_STATUS_BLOCK::default();
        // SAFETY: `directory` is a directory handle and `buffer` is writable.
        let status = unsafe {
            NtQueryDirectoryFile(
                directory.as_raw_handle() as HANDLE,
                std::ptr::null_mut(),
                None,
                std::ptr::null(),
                &mut io,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                FileDirectoryInformation,
                false,
                std::ptr::null(),
                restart,
            )
        };
        restart = false;
        if status == STATUS_NO_MORE_FILES || status == STATUS_NO_SUCH_FILE {
            break;
        }
        if status != STATUS_SUCCESS {
            return Err(nt_error("list directory", status));
        }
        let mut offset = 0_usize;
        loop {
            if offset + std::mem::size_of::<FILE_DIRECTORY_INFORMATION>() > buffer.len() {
                break;
            }
            // SAFETY: the directory query wrote FILE_DIRECTORY_INFORMATION
            // records into `buffer`, and `offset` starts at one of them.
            let info = unsafe { &*buffer.as_ptr().add(offset).cast::<FILE_DIRECTORY_INFORMATION>() };
            let name_at = offset + std::mem::offset_of!(FILE_DIRECTORY_INFORMATION, FileName);
            let name_len = info.FileNameLength as usize;
            if name_at + name_len > buffer.len() {
                return Err(UpdateError::path("", "directory entry is truncated"));
            }
            let units = unsafe { std::slice::from_raw_parts(buffer.as_ptr().add(name_at).cast::<u16>(), name_len / 2) };
            let name = OsString::from_wide(units);
            if name != "." && name != ".." {
                if info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    return Err(UpdateError::path(name.to_string_lossy().to_string(), "refusing a reparse point"));
                }
                out.push(name);
            }
            if info.NextEntryOffset == 0 {
                break;
            }
            offset += info.NextEntryOffset as usize;
        }
    }
    Ok(out)
}

fn open_file(parent: &OwnedHandle, name: &OsStr, access: u32) -> Result<Option<OpenedFile>, UpdateError> {
    match open_child(parent, name, access, FILE_OPEN, FILE_NON_DIRECTORY_FILE | FILE_OPTS, 0) {
        Ok(handle) => Ok(Some(accept_file(handle, name)?)),
        Err(OpenError::Missing(_)) => Ok(None),
        Err(err) => Err(err.into_update()),
    }
}

fn open_directory_path(path: &Path, expected: Option<u32>, access: u32) -> Result<(OwnedHandle, OwnedHandle, u32), UpdateError> {
    let wide = nt_path(path).map_err(OpenError::into_update)?;
    let probe = nt_create(None, &wide, access, FILE_OPEN, FILE_DIRECTORY_FILE | DIR_OPTS, 0, DIR_SHARE).map_err(OpenError::into_update)?;
    let probe_info = information(&probe)?;
    if probe_info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(UpdateError::path(path.display().to_string(), "refusing a reparse point"));
    }
    if probe_info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(UpdateError::path(path.display().to_string(), "pinned ancestor is not a directory"));
    }
    if let Some(expected) = expected
        && probe_info.dwVolumeSerialNumber != expected
    {
        return Err(UpdateError::Root(format!("{} is not on the same volume as its parent", path.display())));
    }
    // The no-follow handle is the usable handle. Child creation through it was
    // checked on this Windows version; a second writable open is not required.
    let usable = duplicate(&probe)?;
    Ok((probe, usable, probe_info.dwVolumeSerialNumber))
}

fn open_directory_child(parent: &OwnedHandle, name: &OsStr, access: u32) -> Result<Option<(OwnedHandle, OwnedHandle)>, UpdateError> {
    match open_child_share(parent, name, access, FILE_OPEN, FILE_DIRECTORY_FILE | DIR_OPTS, 0, DIR_SHARE) {
        Ok(probe) => {
            let probe_info = information(&probe)?;
            if probe_info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(UpdateError::path(name.to_string_lossy().to_string(), "refusing a reparse point"));
            }
            if probe_info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
                return Err(UpdateError::path(name.to_string_lossy().to_string(), "pinned path is not a directory"));
            }
            let usable = duplicate(&probe)?;
            Ok(Some((probe, usable)))
        }
        Err(OpenError::Missing(_)) => Ok(None),
        Err(err) => Err(err.into_update()),
    }
}

fn create_directory_child(parent: &OwnedHandle, name: &OsStr) -> Result<OwnedHandle, UpdateError> {
    open_child_share(parent, name, DIR_CREATE, FILE_CREATE, FILE_DIRECTORY_FILE | DIR_OPTS, FILE_ATTRIBUTE_NORMAL, DIR_SHARE)
        .map_err(OpenError::into_update)
}

fn duplicate(handle: &OwnedHandle) -> Result<OwnedHandle, UpdateError> {
    let mut out: HANDLE = std::ptr::null_mut();
    // SAFETY: both process handles are this process, and `handle` is owned.
    let ok = unsafe {
        DuplicateHandle(GetCurrentProcess(), handle.as_raw_handle() as HANDLE, GetCurrentProcess(), &mut out, 0, 0, DUPLICATE_SAME_ACCESS)
    };
    if ok == 0 || out.is_null() {
        return Err(io_error("duplicate handle"));
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(out as RawHandle) })
}

fn accept_file(handle: OwnedHandle, name: &OsStr) -> Result<OpenedFile, UpdateError> {
    let info = information(&handle)?;
    let label = name.to_string_lossy();
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(UpdateError::path(label, "refusing a reparse point"));
    }
    if info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        return Err(UpdateError::path(label, "expected a regular file"));
    }
    if info.nNumberOfLinks != 1 {
        return Err(UpdateError::path(label, "refusing a file with more than one link"));
    }
    Ok(OpenedFile { size: ((info.nFileSizeHigh as u64) << 32) | info.nFileSizeLow as u64, serial: info.dwVolumeSerialNumber, handle })
}

fn information(handle: &OwnedHandle) -> Result<BY_HANDLE_FILE_INFORMATION, UpdateError> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `handle` owns an open file and `info` is a writable ABI record.
    let ok = unsafe { GetFileInformationByHandle(handle.as_raw_handle() as HANDLE, &mut info) };
    if ok == 0 { Err(io_error("file information")) } else { Ok(info) }
}

enum OpenError {
    Missing(i32),
    Failed(UpdateError),
}

impl OpenError {
    fn into_update(self) -> UpdateError {
        match self {
            Self::Missing(status) => nt_error("open", status),
            Self::Failed(err) => err,
        }
    }
}

/// Regular-file open. Share is read only, so a hashed file denies writers.
/// Directory opens use [`open_child_share`] with [`DIR_SHARE`] instead.
fn open_child(
    parent: &OwnedHandle,
    name: &OsStr,
    access: u32,
    disposition: u32,
    options: u32,
    attributes: u32,
) -> Result<OwnedHandle, OpenError> {
    open_child_share(parent, name, access, disposition, options, attributes, FILE_SHARE)
}

fn open_child_share(
    parent: &OwnedHandle,
    name: &OsStr,
    access: u32,
    disposition: u32,
    options: u32,
    attributes: u32,
    share: u32,
) -> Result<OwnedHandle, OpenError> {
    let wide = single_segment(name).map_err(OpenError::Failed)?;
    nt_create(Some(parent.as_raw_handle()), &wide, access, disposition, options, attributes, share)
}

fn create_child(parent: &OwnedHandle, name: &OsStr, access: u32, options: u32, attributes: u32) -> Result<OwnedHandle, UpdateError> {
    open_child(parent, name, access, FILE_CREATE, options, attributes).map_err(OpenError::into_update)
}

fn nt_create(
    parent: Option<RawHandle>,
    wide: &[u16],
    access: u32,
    disposition: u32,
    options: u32,
    attributes: u32,
    share: u32,
) -> Result<OwnedHandle, OpenError> {
    if wide.len() > (u16::MAX as usize) / 2 {
        return Err(OpenError::Failed(UpdateError::path("", "path is unreasonably long")));
    }
    let name = UNICODE_STRING { Length: (wide.len() * 2) as u16, MaximumLength: (wide.len() * 2) as u16, Buffer: wide.as_ptr().cast_mut() };
    let object = OBJECT_ATTRIBUTES {
        Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.map(|handle| handle as HANDLE).unwrap_or(std::ptr::null_mut()),
        ObjectName: &name,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: std::ptr::null(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut io = windows_sys::Win32::System::IO::IO_STATUS_BLOCK::default();
    let mut handle: HANDLE = std::ptr::null_mut();
    let file_attributes = if attributes == 0 { FILE_ATTRIBUTE_NORMAL } else { attributes };
    // SAFETY: `name` and `object` live for the call. Directory opens set
    // FILE_OPEN_REPARSE_POINT and keep that handle; they are not reopened by
    // file id and there is no path fallback.
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            access,
            &object,
            &mut io,
            std::ptr::null(),
            file_attributes,
            share,
            disposition,
            options,
            std::ptr::null(),
            0,
        )
    };
    if status == STATUS_OBJECT_NAME_NOT_FOUND || status == STATUS_OBJECT_PATH_NOT_FOUND {
        return Err(OpenError::Missing(status));
    }
    if status != STATUS_SUCCESS || handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(OpenError::Failed(nt_error("open", status)));
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) })
}

fn ancestry(path: &Path) -> Result<Vec<PathBuf>, UpdateError> {
    let mut cursor = PathBuf::new();
    let mut chain = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => cursor.push(prefix.as_os_str()),
            Component::RootDir => {
                cursor.push(component.as_os_str());
                chain.push(cursor.clone());
            }
            Component::Normal(part) => {
                cursor.push(part);
                chain.push(cursor.clone());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(UpdateError::path(path.display().to_string(), "parent traversal is not pinned"));
            }
        }
    }
    if chain.is_empty() {
        return Err(UpdateError::path(path.display().to_string(), "path has no directory to pin"));
    }
    Ok(chain)
}

fn require_absolute(path: &Path) -> Result<PathBuf, UpdateError> {
    if !path.is_absolute() || path.components().any(|part| matches!(part, Component::ParentDir)) {
        return Err(UpdateError::Root(format!("{} must be an absolute directory", path.display())));
    }
    Ok(path.to_path_buf())
}

fn nt_path(path: &Path) -> Result<Vec<u16>, OpenError> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(OpenError::Failed(UpdateError::path(path.display().to_string(), "path contains NUL")));
    }
    let mut out: Vec<u16> = OsStr::new(r"\??\").encode_wide().collect();
    const VERBATIM: [u16; 4] = [b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    if wide.starts_with(&VERBATIM) {
        out.extend_from_slice(&wide[4..]);
    } else if wide.starts_with(&[b'\\' as u16, b'\\' as u16]) {
        out.extend(OsStr::new(r"UNC\").encode_wide());
        out.extend_from_slice(&wide[2..]);
    } else {
        out.extend_from_slice(&wide);
    }
    Ok(out)
}

fn single_segment(name: &OsStr) -> Result<Vec<u16>, UpdateError> {
    let wide: Vec<u16> = name.encode_wide().collect();
    if wide.is_empty()
        || wide.contains(&0)
        || wide.contains(&(b'\\' as u16))
        || wide.contains(&(b'/' as u16))
        || wide.contains(&(b':' as u16))
    {
        return Err(UpdateError::path(name.to_string_lossy().to_string(), "name is not a single path segment"));
    }
    Ok(wide)
}

fn read_at(handle: &OwnedHandle, buf: &mut [u8]) -> Result<usize, UpdateError> {
    if buf.is_empty() {
        return Ok(0);
    }
    let mut read = 0_u32;
    // SAFETY: `handle` is open for read and `buf` is writable for this call.
    let ok = unsafe {
        ReadFile(
            handle.as_raw_handle() as HANDLE,
            buf.as_mut_ptr(),
            buf.len().min(u32::MAX as usize) as u32,
            &mut read,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 { Err(io_error("read")) } else { Ok(read as usize) }
}

fn write_at(handle: &OwnedHandle, buf: &[u8]) -> Result<usize, UpdateError> {
    if buf.is_empty() {
        return Ok(0);
    }
    let mut written = 0_u32;
    // SAFETY: `handle` is open for write and `buf` is readable for this call.
    let ok = unsafe {
        WriteFile(
            handle.as_raw_handle() as HANDLE,
            buf.as_ptr(),
            buf.len().min(u32::MAX as usize) as u32,
            &mut written,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 { Err(io_error("write")) } else { Ok(written as usize) }
}

fn nt_error(what: &str, status: i32) -> UpdateError {
    let _ = unsafe { GetLastError() };
    UpdateError::Io(io::Error::other(format!("{what}: ntstatus {status:#010x}: {}", io::Error::last_os_error())))
}

fn io_error(what: &str) -> UpdateError {
    let err = io::Error::last_os_error();
    let _ = unsafe { GetLastError() };
    UpdateError::Io(io::Error::other(format!("{what}: {err}")))
}

fn update_to_io(err: UpdateError) -> io::Error {
    match err {
        UpdateError::Io(err) => err,
        other => io::Error::other(other.to_string()),
    }
}
