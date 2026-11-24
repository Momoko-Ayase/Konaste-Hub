//! Read-only installation evidence for a single fresh official index.
//!
//! Discovery owns freshness and authenticity of `InstallationPin`. This module
//! does not infer a PE version from an index revision, trust an installed marker
//! without comparing its digest, or open an updater transaction. It pins both
//! files and the directory chain against writes/rename while the evidence lives.

use std::{
    fs::File,
    path::{Path, PathBuf},
};

use konaste_bootstrap::GameFamily;
#[cfg(all(windows, target_arch = "x86_64"))]
use sha2::{Digest, Sha256};
#[cfg(all(windows, target_arch = "x86_64"))]
use std::{io::Read, os::windows::ffi::OsStrExt};
use thiserror::Error;
#[cfg(all(windows, target_arch = "x86_64"))]
use windows_sys::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};

#[cfg(all(windows, target_arch = "x86_64"))]
use crate::official::{file_information, pin_file, pin_root};

// The authenticated SDVX index is 6,158,980 bytes; keep a bounded 8 MiB cap.
const MAX_INDEX_BYTES: u64 = 8 * 1024 * 1024;
#[cfg(test)]
const MODULE_PATH: &str = "modules/bomber.exe";

/// These are final installed-file digests, supplied by validated fresh discovery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstallationPin {
    pub index_sha256: [u8; 32],
    pub module_sha256: [u8; 32],
    pub module_bytes: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum InstallationError {
    #[error("installation verification is not supported for this title")]
    Unsupported,
    #[error("the installation is incomplete")]
    Missing,
    #[error("the installed index is not the current official index")]
    Outdated,
    #[error("the installed game could not be verified")]
    Invalid,
}

/// Privately constructed evidence, valid only while its read-only pins are held.
/// No launch entitlement is implied. This type is neither serializable nor Clone.
pub struct InstalledGame {
    family: GameFamily,
    root: PathBuf,
    version: String,
    digest: [u8; 32],
    _module: File,
    _index: File,
    _directories: Vec<File>,
}

impl InstalledGame {
    pub fn family(&self) -> GameFamily {
        self.family
    }
    pub fn product_version(&self) -> &str {
        &self.version
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub(crate) fn module_sha256(&self) -> [u8; 32] {
        self.digest
    }
}

pub fn inspect_installation(family: GameFamily, root: &Path, expected: &InstallationPin) -> Result<InstalledGame, InstallationError> {
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        inspect_with(family, root, expected, product_version)
    }
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        let _ = (family, root, expected);
        Err(InstallationError::Unsupported)
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn inspect_with(
    family: GameFamily,
    root: &Path,
    expected: &InstallationPin,
    read_version: impl FnOnce(&Path) -> Option<String>,
) -> Result<InstalledGame, InstallationError> {
    let title = crate::title(family).map_err(|_| InstallationError::Unsupported)?;
    if expected.module_bytes == 0 || expected.module_bytes > u32::MAX as u64 {
        return Err(InstallationError::Invalid);
    }
    if !root.is_dir() {
        return Err(InstallationError::Missing);
    }
    // Pin the complete module directory chain before resolving canonical
    // spelling or opening either leaf. Existing reparse paths are rejected.
    let module_path = root.join(title.module_path);
    let directories = pin_root(module_path.parent().ok_or(InstallationError::Invalid)?).map_err(|_| InstallationError::Invalid)?;
    let root = root.canonicalize().map_err(|_| InstallationError::Invalid)?;
    let module_path = root.join(title.module_path);
    let index_path = root.join(title.index_path);
    if !module_path.is_file() || !index_path.is_file() {
        return Err(InstallationError::Missing);
    }
    let mut index = pin_file(&index_path).map_err(|_| InstallationError::Invalid)?;
    let index_info = file_information(&index).map_err(|_| InstallationError::Invalid)?;
    let index_size = (u64::from(index_info.nFileSizeHigh) << 32) | u64::from(index_info.nFileSizeLow);
    if index_info.nNumberOfLinks != 1 || index_size == 0 || index_size > MAX_INDEX_BYTES {
        return Err(InstallationError::Invalid);
    }
    if hash(&mut index, index_size)? != expected.index_sha256 {
        return Err(InstallationError::Outdated);
    }
    let mut module = pin_file(&module_path).map_err(|_| InstallationError::Invalid)?;
    let module_info = file_information(&module).map_err(|_| InstallationError::Invalid)?;
    let module_size = (u64::from(module_info.nFileSizeHigh) << 32) | u64::from(module_info.nFileSizeLow);
    if module_info.nNumberOfLinks != 1 || module_size != expected.module_bytes {
        return Err(InstallationError::Invalid);
    }
    let digest = hash(&mut module, module_size)?;
    if digest != expected.module_sha256 {
        return Err(InstallationError::Invalid);
    }
    // The named file remains pinned while the Windows resource reader opens it.
    // The value belongs to these authenticated bytes, not to `ri_ver`.
    let version = read_version(&module_path).filter(|value| title.valid_game_version(value)).ok_or(InstallationError::Invalid)?;
    Ok(InstalledGame { family, root, version, digest, _module: module, _index: index, _directories: directories })
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn hash(file: &mut File, expected_bytes: u64) -> Result<[u8; 32], InstallationError> {
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let read = file.read(&mut buffer).map_err(|_| InstallationError::Invalid)?;
        if read == 0 {
            break;
        }
        total = total.checked_add(read as u64).ok_or(InstallationError::Invalid)?;
        if total > expected_bytes {
            return Err(InstallationError::Invalid);
        }
        digest.update(&buffer[..read]);
    }
    if total != expected_bytes {
        return Err(InstallationError::Invalid);
    }
    Ok(digest.finalize().into())
}

// Never convert an index revision into a PE version or accept format directives.
pub(crate) fn valid_game_version(family: GameFamily, value: &str) -> bool {
    crate::title(family).is_ok_and(|title| title.valid_game_version(value))
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn product_version(path: &Path) -> Option<String> {
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    if name[..name.len() - 1].contains(&0) {
        return None;
    }
    // SAFETY: the fixed resource-reader calls receive owned, bounded buffers.
    let length = unsafe { GetFileVersionInfoSizeW(name.as_ptr(), std::ptr::null_mut()) };
    if length == 0 || length > 1024 * 1024 {
        return None;
    }
    let mut data = vec![0_u8; length as usize];
    if unsafe { GetFileVersionInfoW(name.as_ptr(), 0, length, data.as_mut_ptr().cast()) } == 0 {
        return None;
    }
    let key: Vec<u16> = "\\StringFileInfo\\041104b0\\ProductVersion".encode_utf16().chain(Some(0)).collect();
    let mut value = std::ptr::null_mut();
    let mut count = 0;
    if unsafe { VerQueryValueW(data.as_ptr().cast(), key.as_ptr(), &mut value, &mut count) } == 0
        || value.is_null()
        || count == 0
        || count > 256
    {
        return None;
    }
    let text = unsafe { std::slice::from_raw_parts(value.cast::<u16>(), count as usize) };
    let text = String::from_utf16(text).ok()?;
    Some(text.trim_end_matches('\0').to_string())
}

#[cfg(all(test, windows, target_arch = "x86_64"))]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, InstallationPin) {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("modules")).unwrap();
        std::fs::write(directory.path().join("ri.bin"), b"synthetic-index").unwrap();
        std::fs::write(directory.path().join(MODULE_PATH), b"synthetic-module").unwrap();
        let expected = InstallationPin {
            index_sha256: Sha256::digest(b"synthetic-index").into(),
            module_sha256: Sha256::digest(b"synthetic-module").into(),
            module_bytes: 16,
        };
        (directory, expected)
    }

    #[test]
    fn matching_files_keep_the_real_pe_version_separate_from_the_index() {
        let (directory, expected) = fixture();
        let game = inspect_with(GameFamily::Bombergirl, directory.path(), &expected, |_| Some("QBG:J:A:A:2026041400".into())).unwrap();
        assert_eq!(game.product_version(), "QBG:J:A:A:2026041400");
        assert_eq!(game.module_sha256(), expected.module_sha256);
        assert!(std::fs::write(directory.path().join(MODULE_PATH), b"changed").is_err());
        assert!(std::fs::write(directory.path().join("ri.bin"), b"changed").is_err());
        drop(game);
        std::fs::write(directory.path().join(MODULE_PATH), b"changed").unwrap();
    }

    #[test]
    fn mismatched_index_or_module_never_reaches_a_version_reader() {
        let (directory, mut expected) = fixture();
        let original = expected;
        expected.index_sha256 = [0; 32];
        assert!(matches!(
            inspect_with(GameFamily::Bombergirl, directory.path(), &expected, |_| panic!("outdated index must stop")),
            Err(InstallationError::Outdated)
        ));
        expected = original;
        expected.module_sha256 = [0; 32];
        assert!(matches!(
            inspect_with(GameFamily::Bombergirl, directory.path(), &expected, |_| panic!("mismatched module must stop")),
            Err(InstallationError::Invalid)
        ));
        expected = original;
        expected.module_bytes += 1;
        assert!(matches!(
            inspect_with(GameFamily::Bombergirl, directory.path(), &expected, |_| panic!("wrong size must stop")),
            Err(InstallationError::Invalid)
        ));
    }

    #[test]
    fn empty_oversized_missing_and_hardlinked_markers_are_rejected() {
        let (directory, expected) = fixture();
        let index = directory.path().join("ri.bin");
        std::fs::write(&index, b"").unwrap();
        assert!(inspect_installation(GameFamily::Bombergirl, directory.path(), &expected).is_err());
        std::fs::OpenOptions::new().write(true).open(&index).unwrap().set_len(MAX_INDEX_BYTES + 1).unwrap();
        assert!(inspect_installation(GameFamily::Bombergirl, directory.path(), &expected).is_err());
        std::fs::write(&index, b"synthetic-index").unwrap();
        std::fs::hard_link(&index, directory.path().join("marker-alias")).unwrap();
        assert!(inspect_installation(GameFamily::Bombergirl, directory.path(), &expected).is_err());
        std::fs::remove_file(&index).unwrap();
        assert!(matches!(inspect_installation(GameFamily::Bombergirl, directory.path(), &expected), Err(InstallationError::Missing)));
    }

    #[test]
    fn product_versions_cannot_be_guessed_truncated_or_used_as_format_strings() {
        for version in ["QBG:J:A:A:2026091500", "QBG:J:A:A:2026041400"] {
            assert!(valid_game_version(GameFamily::Bombergirl, version));
        }
        for version in [
            "2026091500",
            "QBG:J:C:A:2026091500%",
            "QBG:J:A:A:%s",
            "QBG:J:A:A:20260915000",
            "QBG:J:A:A:2026091500\0",
            "QBG:J:A:A:2026091500\n",
            "KFC:J:A:A:2026091500",
        ] {
            assert!(!valid_game_version(GameFamily::Bombergirl, version));
        }
        let (directory, expected) = fixture();
        assert!(matches!(inspect_with(GameFamily::Bombergirl, directory.path(), &expected, |_| { None }), Err(InstallationError::Invalid)));
    }

    #[test]
    fn gitadora_requires_a_mode_before_settings_or_worker_admission() {
        let title = crate::title(GameFamily::Gitadora).unwrap();
        assert!(title.launch_supported());
        let root = tempfile::tempdir().unwrap();
        let module = root.path().join(title.module_path);
        std::fs::create_dir_all(module.parent().unwrap()).unwrap();
        std::fs::write(&module, b"synthetic-module").unwrap();
        std::fs::write(root.path().join(title.index_path), b"synthetic-index").unwrap();
        let pin = InstallationPin {
            index_sha256: Sha256::digest(b"synthetic-index").into(),
            module_sha256: Sha256::digest(b"synthetic-module").into(),
            module_bytes: 16,
        };
        let game = inspect_with(GameFamily::Gitadora, root.path(), &pin, |_| Some("U32:J:A:A:2026010100".into())).unwrap();
        let missing = root.path().join("not-a-worker-or-cache");
        assert!(matches!(
            crate::prepare_launch(&missing, &missing, game, zeroize::Zeroizing::new("synthetic-token".into()), None,),
            Err(crate::RuntimeError::GameModeRequired)
        ));
        std::fs::write(module, b"lease released without a game").unwrap();
        assert!(!missing.exists());
    }

    #[test]
    fn admitted_installations_bind_their_layout_and_pe_product_family() {
        for family in GameFamily::ALL {
            let Ok(title) = crate::title(family) else {
                continue;
            };
            let root = tempfile::tempdir().unwrap();
            let module = root.path().join(title.module_path);
            let index = root.path().join(title.index_path);
            std::fs::create_dir_all(module.parent().unwrap()).unwrap();
            std::fs::create_dir_all(index.parent().unwrap()).unwrap();
            std::fs::write(&module, b"synthetic-module").unwrap();
            std::fs::write(&index, b"synthetic-index").unwrap();
            let expected = InstallationPin {
                index_sha256: Sha256::digest(b"synthetic-index").into(),
                module_sha256: Sha256::digest(b"synthetic-module").into(),
                module_bytes: 16,
            };
            let version = format!("{}:J:A:A:2026010100", title.product_code);
            let game = inspect_with(family, root.path(), &expected, |path| {
                assert_eq!(path, root.path().canonicalize().unwrap().join(title.module_path));
                Some(version.clone())
            })
            .unwrap();
            assert_eq!(game.family(), family);
            assert_eq!(game.product_version(), version);
            assert!(std::fs::write(&module, b"changed").is_err());
            drop(game);
            assert!(matches!(
                inspect_with(family, root.path(), &expected, |_| Some("ZZZ:J:A:A:2026010100".into())),
                Err(InstallationError::Invalid)
            ));
        }
    }
}
