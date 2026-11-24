//! Small durable download records tied to one exact checkpoint.
//!
//! The full plan stays in journal.json. A record carries only a file index, the
//! strong response ETag, and bytes accepted into the original stage. It never
//! carries a path, URL, digest, or size, and replaying it never authorizes
//! metadata the checkpoint did not already contain.
use std::{ffi::OsStr, os::windows::io::OwnedHandle};

use sha2::{Digest, Sha256};

use super::{FilePhase, Journal, JournalFile};
use crate::{error::UpdateError, pin};

pub(crate) const NAME: &str = "download.log";
const MAGIC: &[u8; 8] = b"KHUDWAL1";
const HEADER: usize = 40;
/// sequence, index, phase, etag length, received.
const BODY: usize = 24;
const RECORD: usize = BODY + 32;
const MAX_ETAG: usize = 512;
/// Header plus two records and one ETag per file, for the journal file cap.
pub(super) const MAX_BYTES: u64 = HEADER as u64 + 100_000 * (2 * RECORD as u64 + MAX_ETAG as u64);

pub(crate) struct DownloadLog {
    handle: OwnedHandle,
    checkpoint: [u8; 32],
    previous: [u8; 32],
    sequence: u64,
    sizes: Vec<u64>,
}

impl DownloadLog {
    /// A full checkpoint must have succeeded first. Create-only refuses an
    /// unretired log rather than appending after an unknown or torn tail.
    pub(crate) fn open(lease: &mut pin::Lease) -> Result<Self, UpdateError> {
        let work = lease.work()?;
        let base = pin::open_file_read(&work, OsStr::new(super::JOURNAL_FILE))?
            .ok_or_else(|| UpdateError::journal("download checkpoint is missing"))?;
        if base.size() > super::JOURNAL_READ_LIMIT {
            return Err(UpdateError::journal("download checkpoint is too large"));
        }
        let bytes = pin::read_exact(base.handle(), base.size())?;
        let journal: Journal = serde_json::from_slice(&bytes).map_err(|_| UpdateError::journal("download checkpoint is unreadable"))?;
        if !super::readable_version(journal.version) || journal.files.len() > 100_000 {
            return Err(UpdateError::journal("download checkpoint is unsupported"));
        }
        let checkpoint: [u8; 32] = Sha256::digest(&bytes).into();
        let sizes = journal.files.iter().map(|file| file.size).collect();
        if let Some(existing) = pin::open_file_write(&work, OsStr::new(NAME))? {
            let size = existing.size();
            if size > MAX_BYTES {
                return Err(UpdateError::journal("download progress log is too large"));
            }
            let records = pin::read_exact(existing.handle(), size)?;
            let (previous, sequence, kept) = resume(&checkpoint, &records)?;
            // A torn tail is not a record. The next append starts at the last
            // complete one; the tail stays past the new end after the next write.
            pin::seek(existing.handle(), std::io::SeekFrom::Start(kept))?;
            return Ok(Self { handle: existing.into_handle(), checkpoint, previous, sequence, sizes });
        }
        let handle = pin::create_file(&work, OsStr::new(NAME))?;
        let mut header = [0; HEADER];
        header[..8].copy_from_slice(MAGIC);
        header[8..].copy_from_slice(&checkpoint);
        pin::write_all(&handle, &header)?;
        pin::flush(&handle)?;
        Ok(Self { handle, checkpoint, previous: checkpoint, sequence: 0, sizes })
    }

    /// Persist a strong ETag before any original-stage byte. The caller flushes
    /// this record and only then consumes the response body.
    pub(crate) fn record_etag(&mut self, index: usize, etag: &str, file: &JournalFile) -> Result<(), UpdateError> {
        self.record(index, file, Some(etag))
    }

    /// Bytes accepted into the original stage. An absent ETag keeps the one
    /// already recorded; this record does not invent a validator.
    pub(crate) fn record_received(&mut self, index: usize, file: &JournalFile) -> Result<(), UpdateError> {
        self.record(index, file, None)
    }

    fn record(&mut self, index: usize, file: &JournalFile, etag: Option<&str>) -> Result<(), UpdateError> {
        let expected = self.sizes.get(index).ok_or_else(|| UpdateError::journal("download progress index is out of bounds"))?;
        if file.size != *expected || file.received > *expected {
            return Err(UpdateError::journal("download progress size is invalid"));
        }
        let etag = etag.unwrap_or("");
        if etag.len() > MAX_ETAG || etag.chars().any(|c| c.is_control()) {
            return Err(UpdateError::journal("download progress etag is unusable"));
        }
        let sequence = self.sequence.checked_add(1).ok_or_else(|| UpdateError::journal("download progress sequence is exhausted"))?;
        let bytes = HEADER as u64 + sequence.saturating_mul(RECORD as u64) + etag.len() as u64;
        if bytes > MAX_BYTES {
            return Err(UpdateError::journal("download progress log is too large"));
        }
        let mut record = vec![0_u8; RECORD + etag.len()];
        record[..8].copy_from_slice(&sequence.to_le_bytes());
        record[8..12].copy_from_slice(&(index as u32).to_le_bytes());
        record[12] = encode_phase(file.phase);
        record[13..15].copy_from_slice(&(etag.len() as u16).to_le_bytes());
        record[16..BODY].copy_from_slice(&file.received.to_le_bytes());
        record[BODY..BODY + etag.len()].copy_from_slice(etag.as_bytes());
        let digest = checksum(&self.checkpoint, &self.previous, &record[..BODY + etag.len()]);
        record[BODY + etag.len()..].copy_from_slice(&digest);
        pin::write_all(&self.handle, &record)?;
        pin::flush(&self.handle)?;
        self.sequence = sequence;
        self.previous = digest;
        Ok(())
    }
}

/// Continue an existing generation. A torn tail is where the next record starts;
/// a damaged complete record is refused rather than written past.
fn resume(checkpoint: &[u8; 32], bytes: &[u8]) -> Result<([u8; 32], u64, u64), UpdateError> {
    if bytes.len() < HEADER || &bytes[..8] != MAGIC || bytes[8..HEADER] != *checkpoint {
        return Err(UpdateError::journal("download progress log does not match its checkpoint"));
    }
    let mut previous = *checkpoint;
    let mut cursor = HEADER;
    let mut sequence = 0_u64;
    while bytes.len() - cursor >= RECORD {
        let length = u16::from_le_bytes(bytes[cursor + 13..cursor + 15].try_into().expect("fixed etag length")) as usize;
        if length > MAX_ETAG {
            return Err(UpdateError::journal("download progress record is damaged"));
        }
        if bytes.len() - cursor < RECORD + length {
            break;
        }
        let span = RECORD + length;
        let record = &bytes[cursor..cursor + span];
        sequence += 1;
        let stored = u64::from_le_bytes(record[..8].try_into().expect("fixed sequence"));
        let digest = checksum(checkpoint, &previous, &record[..BODY + length]);
        if stored != sequence || record[BODY + length..] != digest || record[15] != 0 {
            return Err(UpdateError::journal("download progress record is damaged"));
        }
        previous = digest;
        cursor += span;
    }
    Ok((previous, sequence, cursor as u64))
}

pub(super) fn replay(journal: &mut Journal, checkpoint: [u8; 32], bytes: &[u8]) -> Result<(), UpdateError> {
    if bytes.len() < HEADER {
        // A torn header precedes every stage byte. No record inside it applies.
        return Ok(());
    }
    if &bytes[..8] != MAGIC {
        return Err(UpdateError::journal("download progress header is invalid"));
    }
    if bytes[8..HEADER] != checkpoint {
        // Checkpoint publication precedes old-log retirement. A crash between
        // them leaves an obsolete log, whose changes are already in the base.
        return Ok(());
    }
    let mut previous = checkpoint;
    let mut cursor = HEADER;
    let mut sequence = 0_u64;
    while bytes.len() - cursor >= RECORD {
        let length = u16::from_le_bytes(bytes[cursor + 13..cursor + 15].try_into().expect("fixed etag length")) as usize;
        if length > MAX_ETAG {
            return Err(UpdateError::journal("download progress record is damaged"));
        }
        if bytes.len() - cursor < RECORD + length {
            break;
        }
        let span = RECORD + length;
        let record = &bytes[cursor..cursor + span];
        let body = &record[..BODY + length];
        sequence += 1;
        let stored = u64::from_le_bytes(record[..8].try_into().expect("fixed sequence"));
        let index = u32::from_le_bytes(record[8..12].try_into().expect("fixed index")) as usize;
        let received = u64::from_le_bytes(record[16..BODY].try_into().expect("fixed size"));
        let digest = checksum(&checkpoint, &previous, body);
        if stored != sequence || record[BODY + length..] != digest || record[15] != 0 {
            return Err(UpdateError::journal("download progress record is damaged"));
        }
        let file = journal.files.get_mut(index).ok_or_else(|| UpdateError::journal("download progress index is out of bounds"))?;
        if received > file.size {
            return Err(UpdateError::journal("download progress size is invalid"));
        }
        let phase = decode_phase(record[12])?;
        // Installation already moved this file. A download record cannot pull
        // it backwards or invent metadata the checkpoint does not carry.
        if matches!(file.phase, FilePhase::Replacing | FilePhase::Installed) {
            return Err(UpdateError::journal("download progress record follows an installation phase"));
        }
        if length > 0 {
            let etag = std::str::from_utf8(&record[BODY..BODY + length])
                .map_err(|_| UpdateError::journal("download progress etag is unusable"))?;
            if etag.chars().any(|c| c.is_control()) {
                return Err(UpdateError::journal("download progress etag is unusable"));
            }
            file.etag = Some(etag.to_string());
        }
        file.phase = phase;
        file.received = received;
        previous.copy_from_slice(&digest);
        cursor += span;
    }
    // Only an incomplete final record is ignored. A complete corrupt record
    // above is refused; progress never skips over a gap or bad checksum.
    Ok(())
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
        _ => Err(UpdateError::journal("download progress phase is invalid")),
    }
}
