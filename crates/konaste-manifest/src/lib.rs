//! Decode the observed ASCII Konaste binary resource-index subset.
//!
//! The caller supplies a SHA-256 learned through validated current official
//! discovery and an exact DNS host allowlist. Deriving that expected digest
//! from the same body does not authenticate it. This module performs no I/O,
//! discovery, downloads, entitlement checks, or installation operations.
//!
//! A resource index describes final stored files. It is not an updater plan:
//! exact HTTP payload semantics must be verified separately. Numeric index
//! revisions are distinct from the game's full PE ProductVersion.

mod binary;
mod validation;

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};
use thiserror::Error;
use url::Url;

use binary::Tree;
use validation::{AllowedHosts, checked_hash, checked_path};

/// The largest verified body is the 6,158,980-byte SDVX index. Eight mebibytes
/// covers that exact body without accepting an unbounded download.
pub const MAX_INDEX_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_FILES: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceIndex {
    pub revision: u32,
    /// Original official order. Absence is not a deletion instruction.
    pub files: Vec<ResourceFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceFile {
    /// Forward-slash, ASCII, installation-root-relative path.
    /// Lexical validation does not protect against filesystem reparse races.
    pub path: String,
    pub revision: u32,
    pub stored_size: u64,
    pub stored_sha256: [u8; 32],
    pub url: Url,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IndexError {
    #[error("resource index exceeds the byte limit")]
    TooLarge,
    #[error("resource index does not match the supplied SHA-256")]
    HashMismatch,
    #[error("invalid exact DNS host allowlist")]
    InvalidAllowedHosts,
    #[error("invalid binary resource index: {0}")]
    Binary(&'static str),
    #[error("unsupported resource-index schema: {0}")]
    Schema(&'static str),
    #[error("unsafe Windows relative path")]
    UnsafePath,
    #[error("resource-index paths alias or conflict")]
    PathAlias,
    #[error("resource URL is outside the allowed HTTPS policy")]
    UnsafeUrl,
}

/// Verify the supplied digest, then parse the complete observed wire subset.
///
/// `expected_sha256` must come from the caller's validated official discovery,
/// not a digest inferred from `bytes`. Hosts are explicit DNS names, without
/// wildcards, URL syntax, IP addresses, trailing dots, or surrounding spaces.
/// DNS comparison is ASCII case-insensitive. Resource URLs use HTTPS on port
/// 443 with no userinfo, query, or fragment; redirects are not handled here.
///
/// Unknown header/encoding/kind/schema variants are rejected, not skipped.
/// The body is capped at 8 MiB and the index at 100,000 files. Paths, names,
/// strings, section lengths, padding, nesting, and exact EOF are validated.
pub fn decode(bytes: &[u8], expected_sha256: [u8; 32], allowed_hosts: &[&str]) -> Result<ResourceIndex, IndexError> {
    if bytes.len() > MAX_INDEX_BYTES {
        return Err(IndexError::TooLarge);
    }
    let actual: [u8; 32] = Sha256::digest(bytes).into();
    if actual != expected_sha256 {
        return Err(IndexError::HashMismatch);
    }
    let hosts = AllowedHosts::new(allowed_hosts)?;
    let mut tree = Tree::open(bytes)?;
    tree.container("resource_info")?;

    let mut files = Vec::new();
    let mut paths = BTreeSet::new();
    let mut revision = None;
    let empty_hash: [u8; 32] = Sha256::digest(b"").into();
    loop {
        match tree.next_kind()? {
            binary::CONTAINER => {
                if files.len() == MAX_FILES {
                    return Err(IndexError::Schema("too many files"));
                }
                tree.container("file")?;
                let path = tree.string("path")?;
                checked_path(&path)?;
                if !paths.insert(path.to_ascii_lowercase()) {
                    return Err(IndexError::PathAlias);
                }
                let file_revision = tree.u32("version")?;
                let stored_size = u64::from(tree.u32("size")?);
                let stored_sha256 = checked_hash(&tree.string("sum")?)?;
                if stored_size == 0 && stored_sha256 != empty_hash {
                    return Err(IndexError::Schema("empty file has a nonempty SHA-256"));
                }
                let url = hosts.url(&tree.string("url")?)?;
                tree.close()?;
                files.push(ResourceFile { path, revision: file_revision, stored_size, stored_sha256, url });
            }
            binary::U32 => {
                if revision.is_some() {
                    return Err(IndexError::Schema("unexpected node kind, name, or order"));
                }
                revision = Some(tree.u32("ri_ver")?);
            }
            binary::CLOSE => break,
            _ => return Err(IndexError::Schema("unexpected node kind, name, or order")),
        }
    }
    let Some(revision) = revision else {
        return Err(IndexError::Schema("missing file records"));
    };
    if files.is_empty() {
        return Err(IndexError::Schema("missing file records"));
    }
    tree.close()?;
    tree.finish()?;

    // A file and a descendant cannot both be installed, even with different case.
    for path in &paths {
        for (position, _) in path.match_indices('/') {
            if paths.contains(&path[..position]) {
                return Err(IndexError::PathAlias);
            }
        }
    }
    Ok(ResourceIndex { revision, files })
}

#[cfg(test)]
mod tests;
