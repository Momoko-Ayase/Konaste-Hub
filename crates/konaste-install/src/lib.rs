//! Fresh official metadata translated into a recoverable title-bound update.
//!
//! Discovery owns authenticity and freshness; the updater owns transport,
//! filesystem protection, staged verification, and durable replacement. This
//! module binds the two contracts without treating a content revision as a PE
//! ProductVersion or reconstructing the official index. Callers obtain a new
//! source for every install/update/resume attempt and hold the updater's native
//! running-game guard throughout destination mutation. No game is started here.

use konaste_bootstrap::GameFamily;
use konaste_discovery::CurrentMetadata;
use konaste_runtime::Title;
use konaste_updater::{DownloadPolicy, Manifest, ManifestFile, OfficialManifest, Plan, Sha256, UpdateError, Updater};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SourceError {
    #[error("the official index cannot describe this title's installation")]
    InvalidMetadata,
}

/// A title-specific source made only from fresh, independently checked metadata.
/// Not a cached grant, a frontend value, or a serializable transaction.
pub struct UpdateSource {
    family: GameFamily,
    title: &'static Title,
    revision: u32,
    manifest: Manifest,
    official: OfficialManifest,
}

impl UpdateSource {
    pub fn from_metadata(metadata: CurrentMetadata) -> Result<Self, SourceError> {
        // Use the authenticated body and immutable title, not the caller-mutable
        // decoded projection. A different title cannot widen the host policy.
        Self::decode(metadata.family(), metadata.bytes(), metadata.sha256)
    }

    pub fn family(&self) -> GameFamily {
        self.family
    }

    pub fn revision(&self) -> u32 {
        self.revision
    }

    pub fn download_policy(&self) -> DownloadPolicy {
        DownloadPolicy::official([self.title.host]).expect("the fixed title HTTPS host is valid")
    }

    /// Bind/diff this fresh source. The updater preserves matching partials and
    /// refuses to apply a different interrupted replacement transaction.
    pub fn prepare(&self, updater: &Updater) -> Result<Plan, UpdateError> {
        updater.prepare(self.manifest.clone(), Some(self.official.clone()))
    }

    fn decode(family: GameFamily, bytes: &[u8], expected: [u8; 32]) -> Result<Self, SourceError> {
        let invalid = || SourceError::InvalidMetadata;
        let title = konaste_runtime::title(family).map_err(|_| invalid())?;
        let index = konaste_manifest::decode(bytes, expected, &[title.host]).map_err(|_| invalid())?;
        if !index.files.iter().any(|file| file.path == title.manifest_module && file.stored_size > 0) {
            return Err(invalid());
        }
        let mut files = Vec::with_capacity(index.files.len());
        for file in index.files {
            let relative_path =
                if title.content_root.is_empty() { file.path.clone() } else { format!("{}/{}", title.content_root, file.path) };
            if relative_path.eq_ignore_ascii_case(title.index_path)
                || !file.url.path().starts_with(title.content_prefix)
                || !file.url.path().ends_with(&format!("/{}", file.path))
            {
                return Err(invalid());
            }
            let digest: String = file.stored_sha256.iter().map(|byte| format!("{byte:02x}")).collect();
            files.push(ManifestFile {
                relative_path,
                size: file.stored_size,
                sha256: Sha256::parse(&digest).map_err(|_| invalid())?,
                url: Some(file.url.into()),
                etag: None,
            });
        }
        Ok(Self {
            family,
            title,
            revision: index.revision,
            manifest: Manifest { version: index.revision.to_string(), files },
            official: OfficialManifest { path: title.index_path.into(), bytes: bytes.to_vec() },
        })
    }
}

#[cfg(test)]
mod tests;
