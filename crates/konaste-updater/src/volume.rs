//! Same-volume placement for staging and backups.
//!
//! Volume identity comes from the pinned directory handles, not from a path
//! that a junction could retarget between the check and the write.

use std::path::Path;

use crate::error::UpdateError;

pub(crate) fn same_volume(root: &Path, work: &Path) -> Result<(), UpdateError> {
    crate::fsutil::same_volume_paths(root, work)
}

pub(crate) fn free_bytes(path: &Path) -> Result<u64, UpdateError> {
    // The pin proves `path` is the directory we will write, not a junction.
    let _held = crate::fsutil::Held::pin(path)?;
    windows_free_bytes(path)
}

fn windows_free_bytes(path: &Path) -> Result<u64, UpdateError> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free_to_caller: u64 = 0;
    // SAFETY: `wide` is a NUL-terminated path of the directory just pinned.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free_to_caller, std::ptr::null_mut(), std::ptr::null_mut()) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(free_to_caller)
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetDiskFreeSpaceExW(directory: *const u16, free_to_caller: *mut u64, total: *mut u64, free: *mut u64) -> i32;
}
