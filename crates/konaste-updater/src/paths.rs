//! Install-relative path gate.
//!
//! Every manifest path, official-manifest path, and journal path is reduced to
//! a canonical relative form before it is joined onto the installation root.
//! Absolute, drive-qualified, UNC, traversal, ADS, reserved, and work-dir
//! paths never reach the filesystem as destinations.

use std::path::{Component, Path, PathBuf};

use crate::error::UpdateError;

/// Directory reserved inside an installation for staging, backups, and the journal.
pub const WORK_DIR_NAME: &str = ".konaste-hub";

const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4",
    "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A manifest path that has passed the gate and can be joined onto the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SafeRelative {
    /// Forward-slash form stored in the journal and shown to callers.
    display: String,
    parts: Vec<String>,
}

impl SafeRelative {
    pub(crate) fn display(&self) -> &str {
        &self.display
    }

    pub(crate) fn join_under(&self, root: &Path) -> PathBuf {
        let mut out = root.to_path_buf();
        for part in &self.parts {
            out.push(part);
        }
        out
    }
}

/// Reject anything that is not a relative file path inside the installation.
pub(crate) fn parse_relative(raw: &str) -> Result<SafeRelative, UpdateError> {
    if raw.is_empty() {
        return Err(UpdateError::path(raw, "path is empty"));
    }
    if raw.len() > 32_000 {
        return Err(UpdateError::path(raw, "path is unreasonably long"));
    }
    if raw.contains('\0') {
        return Err(UpdateError::path(raw, "path contains NUL"));
    }
    // Alternate data streams and Windows device syntax both use ':'.
    if raw.contains(':') {
        return Err(UpdateError::path(raw, "colon is not allowed (drive, ADS, or device)"));
    }
    if raw.contains("//") || raw.contains("\\\\") {
        return Err(UpdateError::path(raw, "empty path segment"));
    }

    let path = Path::new(raw);
    if path.is_absolute() {
        return Err(UpdateError::path(raw, "absolute paths are not allowed"));
    }

    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let text = part.to_string_lossy();
                if text.contains(':') || text.contains('/') || text.contains('\\') {
                    return Err(UpdateError::path(raw, "illegal character in a segment"));
                }
                // Non-ASCII case folding and 8.3 aliases are not an identity we
                // can prove. Supported titles use ASCII paths; reject the rest.
                if !text.is_ascii() || text.chars().any(|c| c.is_control() || c == '~') {
                    return Err(UpdateError::path(raw, "only ASCII path segments without '~' are accepted"));
                }
                if text.ends_with('.') || text.ends_with(' ') {
                    return Err(UpdateError::path(raw, "a segment must not end with a dot or space"));
                }
                let stem = text.split('.').next().unwrap_or("");
                if RESERVED_NAMES.iter().any(|name| stem.eq_ignore_ascii_case(name)) {
                    return Err(UpdateError::path(raw, "Windows reserved device name"));
                }
                if parts.is_empty() && text.eq_ignore_ascii_case(WORK_DIR_NAME) {
                    return Err(UpdateError::path(raw, "the updater work directory is reserved"));
                }
                parts.push(text.into_owned());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(UpdateError::path(raw, "parent traversal is not allowed"));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(UpdateError::path(raw, "rooted or prefixed paths are not allowed"));
            }
        }
    }

    if parts.is_empty() {
        return Err(UpdateError::path(raw, "path does not name a file"));
    }

    Ok(SafeRelative { display: parts.join("/"), parts })
}

/// Case-insensitive identity used to reject two manifest entries that would
/// land on the same Windows file.
pub(crate) fn identity_key(relative: &SafeRelative) -> String {
    relative.display.to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_nested_relative() {
        let parsed = parse_relative(r"data/bgm/track.bin").unwrap();
        assert_eq!(parsed.display(), "data/bgm/track.bin");
    }

    #[test]
    fn rejects_absolute_drive_unc_and_traversal() {
        for raw in [
            r"C:\game\a.bin",
            r"C:a.bin",
            r"\game\a.bin",
            r"\\server\share\a.bin",
            r"/etc/passwd",
            r"data/../../outside.bin",
            r"../outside.bin",
            r".konaste-hub/journal.json",
            r".KONASTE-HUB/secret",
            r"data/file.bin:stream",
            r"CON",
            r"aux.txt",
            r"data/NUL",
            r"data/com1.bin",
            r"foo ",
            r"foo.",
            "",
        ] {
            assert!(parse_relative(raw).is_err(), "accepted {raw}");
        }
    }

    #[test]
    fn duplicate_identity_ignores_case_and_separators() {
        let a = parse_relative(r"Data/File.BIN").unwrap();
        let b = parse_relative("data\\file.bin").unwrap();
        assert_eq!(identity_key(&a), identity_key(&b));
    }
}
