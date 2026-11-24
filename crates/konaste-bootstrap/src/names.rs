//! Titles whose bootstrap package is a recorded public MSI.
//!
//! INFINITAS is omitted. Its recorded package has no `libeacnet.dll`, so it is
//! not this crate's runtime pair. No URL is invented for a title that lacks a
//! retained, hash-checked package.

use serde::{Deserialize, Serialize};

use crate::error::BootstrapError;

/// A Konaste title with a pinned official launcher package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GameFamily {
    Bombergirl,
    Sdvx,
    Ddr,
    Popn,
    Gitadora,
    Nostalgia,
}

impl GameFamily {
    pub const ALL: [Self; 6] = [Self::Bombergirl, Self::Sdvx, Self::Ddr, Self::Popn, Self::Gitadora, Self::Nostalgia];

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|family| family.as_str() == value)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bombergirl => "bombergirl",
            Self::Sdvx => "sdvx",
            Self::Ddr => "ddr",
            Self::Popn => "popn",
            Self::Gitadora => "gitadora",
            Self::Nostalgia => "nostalgia",
        }
    }
}

/// The runtime pair is only meaningful on the platform the packages target.
pub(crate) fn require_windows_x64() -> Result<(), BootstrapError> {
    if cfg!(all(windows, target_arch = "x86_64")) { Ok(()) } else { Err(BootstrapError::UnsupportedPlatform) }
}

/// Exact long File.FileName values and output filenames for the runtime pair.
/// The corresponding validated File.File keys identify the cabinet members.
pub(crate) const WANTED_FILES: [&str; 2] = ["libeacnet.dll", "avs2-core.dll"];

/// The output DLL name must be one of the two literal runtime filenames.
/// No directory, no `..`, no alternate stream.
pub(crate) fn is_exact_runtime_name(name: &str) -> bool {
    if name.len() != name.trim().len() {
        return false;
    }
    if name.contains(['/', '\\', ':', '\0']) || name.contains("..") {
        return false;
    }
    WANTED_FILES.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_two_literal_names_pass() {
        assert!(is_exact_runtime_name("libeacnet.dll"));
        assert!(is_exact_runtime_name("avs2-core.dll"));
        for rejected in [
            "Libeacnet.dll",
            "libeacnet.dll ",
            "../libeacnet.dll",
            "dir/libeacnet.dll",
            "dir\\libeacnet.dll",
            "libeacnet.dll:stream",
            "libeacnet.dll\0",
            "..",
            "launcher.exe",
            "",
        ] {
            assert!(!is_exact_runtime_name(rejected), "{rejected}");
        }
    }

    #[cfg(not(all(windows, target_arch = "x86_64")))]
    #[test]
    fn this_platform_is_unsupported() {
        assert!(matches!(require_windows_x64(), Err(BootstrapError::UnsupportedPlatform)));
    }
}
