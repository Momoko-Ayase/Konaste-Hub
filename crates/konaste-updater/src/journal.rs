//! Durable write-ahead journal.
//!
//! Full checkpoints are flushed and atomically replaced. Installation progress
//! uses small, flushed records instead of rewriting the entire plan per file.
//! The log is tied to the exact checkpoint and replayed before recovery; a torn
//! final record cannot hide an earlier completed intent. Multi-file replacement
//! is still recoverable, not magically atomic.

mod download;
mod progress;
#[cfg(test)]
mod tests;

pub(crate) use download::DownloadLog;
pub(crate) use progress::ProgressLog;

use sha2::{Digest, Sha256};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::UpdateError;
use crate::manifest::CheckedManifest;
use crate::paths::WORK_DIR_NAME;

pub(crate) const JOURNAL_FILE: &str = "journal.json";
pub(crate) const JOURNAL_TEMP: &str = "journal.json.tmp";
pub const COMMITTED_MANIFEST_FILE: &str = "committed.manifest.json";
pub(crate) const STAGING_DIR: &str = "staging";
pub(crate) const LOCK_FILE: &str = "session.lock";

const JOURNAL_VERSION: u32 = 3;

pub(super) fn readable_version(version: u32) -> bool {
    version == JOURNAL_VERSION
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum JournalPhase {
    Planned,
    Downloading,
    Staged,
    Installing,
    Publishing,
    Committed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FilePhase {
    Pending,
    Partial,
    Staged,
    /// Previous bytes are in the backup slot; destination may be missing.
    Replacing,
    /// New bytes are at the destination and verified.
    Installed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct JournalFile {
    pub relative: String,
    pub sha256: String,
    pub size: u64,
    pub url: Option<String>,
    pub etag: Option<String>,
    pub phase: FilePhase,
    /// Bytes accepted into the partial/staged file. Recomputed from disk on open.
    pub received: u64,
    /// True when an installed file differed and a backup was (or must be) kept.
    pub had_previous: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct JournalOfficial {
    pub relative: String,
    pub sha256: String,
    pub size: u64,
    pub phase: FilePhase,
    pub had_previous: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Journal {
    pub version: u32,
    pub manifest_version: String,
    pub phase: JournalPhase,
    pub files: Vec<JournalFile>,
    pub official: Option<JournalOfficial>,
}

impl Journal {
    pub(crate) fn from_checked(checked: &CheckedManifest) -> Self {
        Self {
            version: JOURNAL_VERSION,
            manifest_version: checked.version.clone(),
            phase: JournalPhase::Planned,
            files: checked
                .files
                .iter()
                .map(|file| JournalFile {
                    relative: file.relative.display().to_string(),
                    sha256: file.sha256.as_str().to_string(),
                    size: file.size,
                    url: file.url.clone(),
                    etag: file.etag.clone(),
                    phase: FilePhase::Pending,
                    received: 0,
                    had_previous: false,
                })
                .collect(),
            official: checked.official.as_ref().map(|official| JournalOfficial {
                relative: official.relative.display().to_string(),
                sha256: official.sha256.as_str().to_string(),
                size: official.size,
                phase: FilePhase::Pending,
                had_previous: false,
            }),
        }
    }
}

pub(crate) struct WorkPaths {
    pub root: PathBuf,
    pub work: PathBuf,
    pub staging: PathBuf,
    pub committed: PathBuf,
}

impl WorkPaths {
    pub(crate) fn under(root: PathBuf) -> Self {
        let work = root.join(WORK_DIR_NAME);
        Self { staging: work.join(STAGING_DIR), committed: work.join(COMMITTED_MANIFEST_FILE), work, root }
    }
}

pub(crate) const JOURNAL_READ_LIMIT: u64 = 64 * 1024 * 1024;

pub(crate) fn load_from(
    work: &std::os::windows::io::OwnedHandle,
    policy: &crate::policy::DownloadPolicy,
) -> Result<Option<Journal>, UpdateError> {
    let Some(file) = crate::pin::open_file_read(work, std::ffi::OsStr::new(JOURNAL_FILE))? else {
        return Ok(None);
    };
    if file.size() > JOURNAL_READ_LIMIT {
        return Err(UpdateError::journal(format!("journal is {} bytes, above the {} byte read limit", file.size(), JOURNAL_READ_LIMIT)));
    }
    let bytes = crate::pin::read_exact(file.handle(), file.size())?;
    let mut journal = decode(&bytes, policy)?.expect("a decoded checkpoint contains a journal");
    if let Some(log) = crate::pin::open_file_read(work, std::ffi::OsStr::new(progress::NAME))? {
        if log.size() > progress::MAX_BYTES {
            return Err(UpdateError::journal("installation progress log is too large"));
        }
        let records = crate::pin::read_exact(log.handle(), log.size())?;
        progress::replay(&mut journal, Sha256::digest(&bytes).into(), &records)?;
        validate(&mut journal, policy)?;
    }
    if let Some(log) = crate::pin::open_file_read(work, std::ffi::OsStr::new(download::NAME))? {
        if log.size() > download::MAX_BYTES {
            return Err(UpdateError::journal("download progress log is too large"));
        }
        let records = crate::pin::read_exact(log.handle(), log.size())?;
        download::replay(&mut journal, Sha256::digest(&bytes).into(), &records)?;
        validate(&mut journal, policy)?;
    }
    Ok(Some(journal))
}

pub(crate) fn decode(bytes: &[u8], policy: &crate::policy::DownloadPolicy) -> Result<Option<Journal>, UpdateError> {
    let mut journal: Journal = serde_json::from_slice(bytes).map_err(|err| UpdateError::journal(format!("unreadable journal: {err}")))?;
    validate(&mut journal, policy)?;
    Ok(Some(journal))
}

/// Replace the journal only after the new document is flushed.
/// A journal on disk is untrusted. Every path is re-parsed with the manifest
/// gate, rewritten to its canonical form, and checked for case-insensitive
/// collisions before any path is joined onto the root.
pub(crate) fn validate(journal: &mut Journal, policy: &crate::policy::DownloadPolicy) -> Result<(), UpdateError> {
    if !readable_version(journal.version) {
        return Err(UpdateError::journal(format!("unsupported journal version {}", journal.version)));
    }
    if journal.manifest_version.trim().is_empty() || journal.manifest_version.len() > 128 {
        return Err(UpdateError::journal("manifest version is unusable"));
    }
    if journal.files.is_empty() && journal.official.is_none() {
        return Err(UpdateError::journal("journal has no files"));
    }
    if journal.files.len() > 100_000 {
        return Err(UpdateError::journal("journal has too many files"));
    }
    let mut seen = std::collections::BTreeSet::new();
    for file in &mut journal.files {
        let safe = crate::paths::parse_relative(&file.relative)?;
        let key = crate::paths::identity_key(&safe);
        if !seen.insert(key) {
            return Err(UpdateError::journal(format!("duplicate path `{}`", safe.display())));
        }
        file.relative = safe.display().to_string();
        crate::manifest::Sha256::parse(&file.sha256)?;
        if file.received > file.size {
            return Err(UpdateError::journal(format!("`{}` claims more received bytes than its size", file.relative)));
        }
        if let Some(url) = &file.url {
            policy.check_url(url)?;
        }
        if let Some(etag) = &file.etag
            && (etag.is_empty() || etag.len() > 512 || etag.chars().any(|c| c.is_control()))
        {
            return Err(UpdateError::journal(format!("`{}` has an unusable etag", file.relative)));
        }
        check_phase(file.phase, &file.relative)?;
    }
    if let Some(official) = &mut journal.official {
        let safe = crate::paths::parse_relative(&official.relative)?;
        let key = crate::paths::identity_key(&safe);
        if seen.contains(&key) {
            return Err(UpdateError::journal(format!("official path `{}` aliases a content file", safe.display())));
        }
        official.relative = safe.display().to_string();
        crate::manifest::Sha256::parse(&official.sha256)?;
        if official.size > 64 * 1024 * 1024 {
            return Err(UpdateError::journal("official manifest is larger than 64 MiB"));
        }
        check_phase(official.phase, &official.relative)?;
    }
    Ok(())
}

fn check_phase(phase: FilePhase, relative: &str) -> Result<(), UpdateError> {
    match phase {
        FilePhase::Pending | FilePhase::Partial | FilePhase::Staged | FilePhase::Replacing | FilePhase::Installed => Ok(()),
        #[allow(unreachable_patterns)]
        _ => Err(UpdateError::journal(format!("`{relative}` has an unknown phase"))),
    }
}

pub(crate) fn store(paths: &WorkPaths, journal: &Journal, lease: &mut crate::pin::Lease) -> Result<(), UpdateError> {
    #[derive(Serialize)]
    struct Checkpoint<'a> {
        version: u32,
        manifest_version: &'a str,
        phase: JournalPhase,
        files: &'a [JournalFile],
        official: &'a Option<JournalOfficial>,
    }
    // Versions 1 and 2 are readable, but never written again: an older executable
    // must refuse this transaction instead of ignoring its durable progress log.
    let checkpoint = Checkpoint {
        version: JOURNAL_VERSION,
        manifest_version: &journal.manifest_version,
        phase: journal.phase,
        files: &journal.files,
        official: &journal.official,
    };
    let bytes = serde_json::to_vec_pretty(&checkpoint).map_err(|err| UpdateError::journal(format!("could not encode journal: {err}")))?;
    let _ = paths;
    replace_in(lease, JOURNAL_TEMP, JOURNAL_FILE, &bytes)?;
    // The complete state is durable before its old log is retired. If a crash
    // happens here, the old header cannot match the newly published checkpoint.
    let work = lease.work()?;
    crate::pin::delete_file(&work, std::ffi::OsStr::new(progress::NAME))?;
    // Both logs are retired only after the checkpoint that contains their
    // records is durable. An obsolete log cannot match this new header.
    crate::pin::delete_file(&work, std::ffi::OsStr::new(download::NAME))
}

pub(crate) fn remove(paths: &WorkPaths, lease: &crate::pin::Lease) -> Result<(), UpdateError> {
    let _ = paths;
    let work = lease.work()?;
    crate::pin::delete_file(&work, std::ffi::OsStr::new(progress::NAME))?;
    crate::pin::delete_file(&work, std::ffi::OsStr::new(download::NAME))?;
    crate::pin::delete_file(&work, std::ffi::OsStr::new(JOURNAL_FILE))
}

/// Publish `bytes` as `final_name` in the work directory by writing a sibling
/// and renaming that handle into the pinned directory.
pub(crate) fn publish_bytes(lease: &mut crate::pin::Lease, temp_name: &str, final_name: &str, bytes: &[u8]) -> Result<(), UpdateError> {
    replace_in(lease, temp_name, final_name, bytes)
}

fn replace_in(lease: &mut crate::pin::Lease, temp_name: &str, final_name: &str, bytes: &[u8]) -> Result<(), UpdateError> {
    let work = lease.work()?;
    if !crate::pin::child_missing(&work, std::ffi::OsStr::new(temp_name))? {
        crate::pin::delete_file(&work, std::ffi::OsStr::new(temp_name))?;
    }
    let temp = crate::pin::create_file(&work, std::ffi::OsStr::new(temp_name))?;
    crate::pin::write_all(&temp, bytes)?;
    crate::pin::flush(&temp)?;
    crate::pin::rename_into(&temp, &work, std::ffi::OsStr::new(final_name), true)
}
