//! No-follow directory pins used to compare volumes.
//!
//! `Held` keeps the directory handles from `Lease::open_existing` alive.
//! Volume identity is the serial on those handles, not a path that a junction
//! could retarget.

use std::path::Path;

use crate::error::UpdateError;
use crate::pin;

/// Directory handles kept open for one rooted check.
pub(crate) struct Held {
    lease: pin::Lease,
}

impl Held {
    pub(crate) fn pin(directory: &Path) -> Result<Self, UpdateError> {
        Ok(Self { lease: pin::Lease::open_existing(directory)? })
    }

    pub(crate) fn serial(&self) -> u32 {
        self.lease.serial()
    }
}

pub(crate) fn same_volume_paths(left: &Path, right: &Path) -> Result<(), UpdateError> {
    let left_pin = if left.is_dir() { Held::pin(left)? } else { Held::pin(left.parent().unwrap_or(left))? };
    let right_pin = if right.is_dir() { Held::pin(right)? } else { Held::pin(right.parent().unwrap_or(right))? };
    if left_pin.serial() != right_pin.serial() {
        return Err(UpdateError::Root(format!("{} is not on the same volume as {}", right.display(), left.display())));
    }
    Ok(())
}
