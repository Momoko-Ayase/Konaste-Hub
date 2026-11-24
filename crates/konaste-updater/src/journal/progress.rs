//! Small, durable installation progress records tied to one exact checkpoint.
//! The full plan stays in journal.json; this log never contains paths or URLs.
use std::{ffi::OsStr, os::windows::io::OwnedHandle};

use sha2::{Digest, Sha256};

use super::{FilePhase, Journal, JournalFile};
use crate::{error::UpdateError, pin};

pub(super) const NAME: &str = "install.log";
const MAGIC: &[u8; 8] = b"KHUPWAL1";
const HEADER: usize = 40;
const BODY: usize = 24;
const RECORD: usize = BODY + 32;
pub(super) const MAX_BYTES: u64 = 32 * 1024 * 1024;

pub(crate) struct ProgressLog {
    handle: OwnedHandle,
    checkpoint: [u8; 32],
    previous: [u8; 32],
    sequence: u64,
    sizes: Vec<u64>,
}

impl ProgressLog {
    /// A full checkpoint must have succeeded first. The first call creates the
    /// log. A later call for the same checkpoint appends, so one record does not
    /// need the lease held across the file it describes.
    pub(crate) fn open(lease: &mut pin::Lease) -> Result<Self, UpdateError> {
        let work = lease.work()?;
        let base = pin::open_file_read(&work, OsStr::new(super::JOURNAL_FILE))?
            .ok_or_else(|| UpdateError::journal("installation checkpoint is missing"))?;
        if base.size() > super::JOURNAL_READ_LIMIT {
            return Err(UpdateError::journal("installation checkpoint is too large"));
        }
        let bytes = pin::read_exact(base.handle(), base.size())?;
        let journal: Journal = serde_json::from_slice(&bytes).map_err(|_| UpdateError::journal("installation checkpoint is unreadable"))?;
        if !super::readable_version(journal.version) || journal.files.len() > 100_000 {
            return Err(UpdateError::journal("installation checkpoint is unsupported"));
        }
        let checkpoint: [u8; 32] = Sha256::digest(&bytes).into();
        let sizes: Vec<u64> = journal.files.iter().map(|file| file.size).collect();
        if let Some(existing) = pin::open_file_write(&work, OsStr::new(NAME))? {
            let mut replayed = journal.clone();
            let records = if existing.size() > MAX_BYTES {
                return Err(UpdateError::journal("installation progress log is too large"));
            } else {
                pin::read_exact(existing.handle(), existing.size())?
            };
            replay(&mut replayed, checkpoint, &records)?;
            let (sequence, previous) = tail(&checkpoint, &records)?;
            let handle = existing.into_handle();
            pin::seek(&handle, std::io::SeekFrom::End(0))?;
            return Ok(Self { handle, checkpoint, previous, sequence, sizes });
        }
        let handle = pin::create_file(&work, OsStr::new(NAME))?;
        let mut header = [0; HEADER];
        header[..8].copy_from_slice(MAGIC);
        header[8..].copy_from_slice(&checkpoint);
        pin::write_all(&handle, &header)?;
        pin::flush(&handle)?;
        Ok(Self { handle, checkpoint, previous: checkpoint, sequence: 0, sizes })
    }

    pub(crate) fn record_file(&mut self, index: usize, file: &JournalFile) -> Result<(), UpdateError> {
        let expected = self.sizes.get(index).ok_or_else(|| UpdateError::journal("installation progress index is out of bounds"))?;
        if file.size != *expected || file.received > *expected {
            return Err(UpdateError::journal("installation progress size is invalid"));
        }
        let sequence = self.sequence.checked_add(1).ok_or_else(|| UpdateError::journal("installation progress sequence is exhausted"))?;
        if HEADER as u64 + sequence.saturating_mul(RECORD as u64) > MAX_BYTES {
            return Err(UpdateError::journal("installation progress log is too large"));
        }
        let mut record = [0; RECORD];
        record[..8].copy_from_slice(&sequence.to_le_bytes());
        record[8..12].copy_from_slice(&(index as u32).to_le_bytes());
        record[12] = encode_phase(file.phase);
        record[13] = u8::from(file.had_previous);
        record[16..BODY].copy_from_slice(&file.received.to_le_bytes());
        let digest = checksum(&self.checkpoint, &self.previous, &record[..BODY]);
        record[BODY..].copy_from_slice(&digest);
        pin::write_all(&self.handle, &record)?;
        pin::flush(&self.handle)?;
        self.sequence = sequence;
        self.previous = digest;
        Ok(())
    }
}

pub(super) fn replay(journal: &mut Journal, checkpoint: [u8; 32], bytes: &[u8]) -> Result<(), UpdateError> {
    if bytes.len() < HEADER {
        // A torn header precedes every destination mutation. The checkpoint is
        // complete, and no record in an incomplete header can grant progress.
        return Ok(());
    }
    if &bytes[..8] != MAGIC {
        return Err(UpdateError::journal("installation progress header is invalid"));
    }
    if bytes[8..HEADER] != checkpoint {
        // Checkpoint publication precedes old-log retirement. A crash between
        // them leaves an obsolete log, whose changes are already in the base.
        return Ok(());
    }
    let mut previous = checkpoint;
    for (number, record) in bytes[HEADER..].as_chunks::<RECORD>().0.iter().enumerate() {
        let sequence = u64::from_le_bytes(record[..8].try_into().expect("fixed sequence"));
        let index = u32::from_le_bytes(record[8..12].try_into().expect("fixed index")) as usize;
        let received = u64::from_le_bytes(record[16..BODY].try_into().expect("fixed size"));
        let digest = checksum(&checkpoint, &previous, &record[..BODY]);
        if sequence != number as u64 + 1 || record[BODY..] != digest || record[14..16] != [0, 0] || record[13] > 1 {
            return Err(UpdateError::journal("installation progress record is damaged"));
        }
        let file = journal.files.get_mut(index).ok_or_else(|| UpdateError::journal("installation progress index is out of bounds"))?;
        if received > file.size {
            return Err(UpdateError::journal("installation progress size is invalid"));
        }
        file.phase = decode_phase(record[12])?;
        file.had_previous = record[13] != 0;
        file.received = received;
        previous = digest;
    }
    // Only an incomplete final record is ignored. A complete corrupt record
    // above is refused; progress never skips over a gap or bad checksum.
    Ok(())
}

fn tail(checkpoint: &[u8; 32], bytes: &[u8]) -> Result<(u64, [u8; 32]), UpdateError> {
    if bytes.len() < HEADER || &bytes[..8] != MAGIC || bytes[8..HEADER] != *checkpoint {
        return Err(UpdateError::journal("installation progress log does not match its checkpoint"));
    }
    let complete = (bytes.len() - HEADER) / RECORD;
    let mut previous = *checkpoint;
    for record in bytes[HEADER..].as_chunks::<RECORD>().0 {
        previous = checksum(checkpoint, &previous, &record[..BODY]);
    }
    Ok((complete as u64, previous))
}

fn checksum(checkpoint: &[u8; 32], previous: &[u8; 32], body: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(checkpoint);
    hash.update(previous);
    hash.update(body);
    hash.finalize().into()
}

fn encode_phase(phase: FilePhase) -> u8 {
    match phase {
        FilePhase::Pending => 0,
        FilePhase::Partial => 1,
        FilePhase::Staged => 2,
        FilePhase::Replacing => 3,
        FilePhase::Installed => 4,
    }
}

fn decode_phase(value: u8) -> Result<FilePhase, UpdateError> {
    match value {
        0 => Ok(FilePhase::Pending),
        1 => Ok(FilePhase::Partial),
        2 => Ok(FilePhase::Staged),
        3 => Ok(FilePhase::Replacing),
        4 => Ok(FilePhase::Installed),
        _ => Err(UpdateError::journal("installation progress phase is invalid")),
    }
}
