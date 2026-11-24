//! One writer for each Hub data directory, including headless installs.
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io,
    path::Path,
};

const LOCK: &str = "hub-instance.lock";
const ACTIVATE: &str = "hub-activate.request";

/// A lock lives as long as the returned file. Never unlink the lock file: new
/// processes must attempt to lock the same file, even across an abrupt exit.
pub fn acquire(directory: &Path) -> io::Result<Option<File>> {
    fs::create_dir_all(directory)?;
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(directory.join(LOCK))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

pub fn request_activation(directory: &Path) {
    let _ = File::create_new(directory.join(ACTIVATE));
}

pub fn take_activation(directory: &Path) -> bool {
    fs::remove_file(directory.join(ACTIVATE)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_writer_is_locked_out_and_can_request_activation() {
        let dir = tempfile::tempdir().unwrap();
        let lock = acquire(dir.path()).unwrap().unwrap();
        assert!(acquire(dir.path()).unwrap().is_none());
        request_activation(dir.path());
        request_activation(dir.path());
        assert!(take_activation(dir.path()));
        assert!(!take_activation(dir.path()));
        drop(lock);
        assert!(acquire(dir.path()).unwrap().is_some());
    }
}
