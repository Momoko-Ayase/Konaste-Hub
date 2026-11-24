//! Caller-supplied update description.
//!
//! This crate does not discover or authenticate a manifest. The parent proves
//! where the bytes and the official resource index came from; we only check
//! that the values are internally usable.

use serde::{Deserialize, Serialize};

use crate::error::UpdateError;
use crate::paths::{self, SafeRelative};

/// Lowercase hex SHA-256 of a file's exact bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sha256(String);

impl Sha256 {
    pub fn parse(hex_text: &str) -> Result<Self, UpdateError> {
        let text = hex_text.trim().to_ascii_lowercase();
        if text.len() != 64 || !text.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(UpdateError::manifest(format!("sha256 must be 64 hex characters, got {hex_text:?}")));
        }
        Ok(Self(text))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn matches(&self, digest: &[u8; 32]) -> bool {
        hex::encode(digest) == self.0
    }
}

/// One file the installation should contain after a successful update.
///
/// `url` is optional. A file already on disk with the expected size and hash
/// is left untouched and is not downloaded. A missing or different file
/// without a URL cannot be planned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestFile {
    pub relative_path: String,
    pub sha256: Sha256,
    pub size: u64,
    pub url: Option<String>,
    /// Strong validator captured when the parent learned the URL (`ETag`).
    /// Resume refuses to append when a later response disagrees or drops it.
    pub etag: Option<String>,
}

/// Desired installation contents. Order is preserved for replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub version: String,
    pub files: Vec<ManifestFile>,
}

/// Official manifest/resource-index bytes the parent wants published inside
/// the installation. The path is title-specific (`ri.bin` for Bombergirl);
/// this crate does not assume a filename and does not parse the bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfficialManifest {
    pub path: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(crate) struct CheckedFile {
    pub relative: SafeRelative,
    pub sha256: Sha256,
    pub size: u64,
    pub url: Option<String>,
    pub etag: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct CheckedManifest {
    pub version: String,
    pub files: Vec<CheckedFile>,
    pub official: Option<CheckedOfficial>,
}

#[derive(Debug, Clone)]
pub(crate) struct CheckedOfficial {
    pub relative: SafeRelative,
    pub bytes: Vec<u8>,
    pub sha256: Sha256,
    pub size: u64,
}

impl Manifest {
    pub(crate) fn check(&self, official: Option<OfficialManifest>) -> Result<CheckedManifest, UpdateError> {
        if self.version.trim().is_empty() || self.version.len() > 128 {
            return Err(UpdateError::manifest("version must be 1..=128 characters"));
        }
        if self.version.chars().any(|c| c.is_control()) {
            return Err(UpdateError::manifest("version contains control characters"));
        }
        if self.files.is_empty() && official.is_none() {
            return Err(UpdateError::manifest("manifest has no files"));
        }
        if self.files.len() > 100_000 {
            return Err(UpdateError::manifest("manifest has too many files"));
        }

        let mut files = Vec::with_capacity(self.files.len());
        let mut seen = std::collections::BTreeSet::new();
        for file in &self.files {
            let relative = paths::parse_relative(&file.relative_path)?;
            let key = paths::identity_key(&relative);
            if !seen.insert(key) {
                return Err(UpdateError::manifest(format!("duplicate path `{}` (case-insensitive)", relative.display())));
            }
            if let Some(url) = &file.url
                && url.trim().is_empty()
            {
                return Err(UpdateError::manifest(format!("`{}` has an empty url", relative.display())));
            }
            if let Some(etag) = &file.etag
                && (etag.is_empty() || etag.len() > 512 || etag.chars().any(|c| c.is_control()))
            {
                return Err(UpdateError::manifest(format!("`{}` has an unusable etag", relative.display())));
            }
            files.push(CheckedFile {
                relative,
                sha256: file.sha256.clone(),
                size: file.size,
                url: file.url.clone(),
                etag: file.etag.clone(),
            });
        }

        let official = match official {
            Some(item) => {
                if item.bytes.len() > 64 * 1024 * 1024 {
                    return Err(UpdateError::manifest("official manifest is larger than 64 MiB"));
                }
                let relative = paths::parse_relative(&item.path)?;
                let key = paths::identity_key(&relative);
                if seen.contains(&key) {
                    return Err(UpdateError::manifest(format!(
                        "official manifest path `{}` collides with a content file",
                        relative.display()
                    )));
                }
                let digest = crate::hash::sha256(&item.bytes);
                Some(CheckedOfficial {
                    relative,
                    size: item.bytes.len() as u64,
                    sha256: Sha256::parse(&hex::encode(digest))?,
                    bytes: item.bytes,
                })
            }
            None => None,
        };

        Ok(CheckedManifest { version: self.version.clone(), files, official })
    }
}
