//! Official-package bootstrap for the Konaste titles whose launcher
//! runtime pair is published as an anonymous MSI.
//!
//! This crate downloads a **pinned** official installer, checks its
//! Authenticode signature, and statically extracts `libeacnet.dll` and
//! `avs2-core.dll` into an app-owned cache. It does not install the package,
//! run `msiexec`, execute any extracted file, write the registry, or touch a
//! game installation directory.
//!
//! # Consent
//!
//! Nothing in the research record grants redistribution or a third-party
//! right to download and extract these packages. [`acquire`] refuses to open
//! a connection unless `consent` is `true`. The caller (the desktop app) must
//! obtain that consent from the end user against the official download terms
//! before passing `true`. There is no production path that downloads by
//! default.
//!
//! # Pins
//!
//! Package URLs and SHA-256 digests are the literals recorded in
//! `docs/research/konaste-platform.md` §3 and
//! `docs/research/remaining-title-contracts.md`. A package whose digest differs
//! is [`BootstrapError::PackageChanged`]: a newer public installer is not
//! accepted automatically, because the launcher ABI is version-sensitive.
//! Publishing a pair here does not admit the title to discovery, install, or
//! play. INFINITAS stays out: its package has no separate `libeacnet.dll`.

mod cache;
mod catalog;
mod error;
mod extract;
mod fetch;
mod hashutil;
pub mod infinitas;
mod names;

#[cfg(windows)]
mod trust;

pub use cache::inspect;
pub use catalog::{OfficialPackage, PackageId, package_for};
pub use error::BootstrapError;
pub use names::GameFamily;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cache::publish_pair;
use crate::extract::extract_pair;
use crate::fetch::{download_pinned, download_pinned_url};
use crate::hashutil::sha256_file;

/// Hard cap on a bootstrap package. The largest recorded package is under 52 MiB.
pub const MAX_PACKAGE_BYTES: u64 = 64 * 1024 * 1024;

/// Whole-transfer deadline for one pinned download, including redirects.
pub const DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// How many redirects a pinned download may follow. Every hop is re-checked.
pub const MAX_REDIRECTS: usize = 3;

/// Bytes transferred so far, for an optional progress callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub received: u64,
    pub total: Option<u64>,
}

/// Where a cached pair came from. Digests and the signer name only — never a
/// token, cookie, or account identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// Literal official package URL the bytes were pinned against.
    pub package_url: String,
    /// Lowercase hex SHA-256 of the MSI bytes.
    pub package_sha256: String,
    /// Authenticode signer display name.
    ///
    /// Currently always `None`. `WinVerifyTrust` reports whether the signature
    /// is trusted, not the signer's name, and this crate does not read the
    /// certificate to supply one. A matching package SHA-256 is the pin that
    /// identifies the recorded official bytes; it is not a verified signer string.
    pub signer: Option<String>,
    pub libeacnet_bytes: u64,
    pub avs2_core_bytes: u64,
    /// Literal URL of a pinned `libeacnet.dll` that replaced the MSI member.
    ///
    /// Absent for families whose published DLL is the cabinet member. A
    /// present value is checked against the package pin; it is not taken from
    /// an editable file as authority.
    pub runtime_update_url: Option<String>,
    /// Lowercase hex SHA-256 of that replacement file.
    pub runtime_update_sha256: Option<String>,
}

/// The two launcher-side DLLs, published together under `cache_root`.
///
/// Paths always live in the app-owned cache. They are never a path inside a
/// game installation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimePair {
    pub family: GameFamily,
    pub libeacnet: PathBuf,
    pub avs2_core: PathBuf,
    pub package_sha256: String,
    /// Boxed so `PairStatus::Present` does not carry the overlay URL strings inline.
    /// Serde encodes a box as the inner value, so the cache JSON is unchanged.
    pub provenance: Box<Provenance>,
}

/// What [`inspect`] found for one family, without downloading anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairStatus {
    /// No complete pair is published for this family.
    Missing,
    /// A pair is present and still matches the recorded pin.
    Present(RuntimePair),
    /// A pair is present but must not be used. `reason` is safe to display.
    Stale { reason: String },
}

/// Download the pinned official MSI for `family` and publish its runtime pair.
///
/// Blocking. Async callers should run it on `spawn_blocking`.
///
/// `consent` must be `true`. `false` returns [`BootstrapError::ConsentRequired`]
/// and does not open a socket. `progress` is invoked with received byte counts
/// while the package is copied; it is not invoked for a cache hit.
///
/// A pair already cached for the same pin is returned without downloading.
/// Concurrent operations on the same family/cache fail with a cache error;
/// the OS-held lock is released on return or process exit. The cache root must
/// be an app-owned directory, not a game installation or an untrusted path.
pub fn acquire(
    family: GameFamily,
    cache_root: &Path,
    consent: bool,
    mut progress: Option<&mut dyn FnMut(Progress)>,
) -> Result<RuntimePair, BootstrapError> {
    if !consent {
        return Err(BootstrapError::ConsentRequired);
    }
    names::require_windows_x64()?;

    let _lock = cache::lock_family(cache_root, family)?;
    if let PairStatus::Present(pair) = cache::inspect_locked(family, cache_root)? {
        return Ok(pair);
    }

    let package = package_for(family);
    let stage = cache::stage_dir(cache_root, family)?;
    let msi_path = stage.join("package.msi");
    download_pinned(package, &msi_path, &mut |received, total| {
        report(&mut progress, received, total);
    })?;

    publish_verified_stage(family, cache_root, &stage, None, &mut progress)
}

/// Verify, extract, and publish a package file that is already on disk.
///
/// The file must hash to the recorded pin for `family` and pass Authenticode
/// before it is parsed. A different digest is [`BootstrapError::PackageChanged`].
/// An arbitrary or unsigned file is rejected. This does not download, and it
/// does not replace the consent requirement on [`acquire`].
///
/// Verify an on-disk MSI and, when `runtime_update` is `Some`, the local file
/// that replaces its `libeacnet.dll`.
///
/// `None` never opens a socket. A family whose pin requires a current
/// `libeacnet.dll` fails closed when that file is absent. The singular helper
/// is that `None` path.
fn acquire_from_verified_files(
    family: GameFamily,
    cache_root: &Path,
    msi_path: &Path,
    runtime_update: Option<&Path>,
) -> Result<RuntimePair, BootstrapError> {
    names::require_windows_x64()?;
    let package = package_for(family);
    if package.runtime_update.is_some() && runtime_update.is_none() {
        return Err(BootstrapError::cache("this family requires the pinned current libeacnet.dll; refusing to publish the MSI member"));
    }
    if package.runtime_update.is_none() && runtime_update.is_some() {
        return Err(BootstrapError::cache("this family has no pinned runtime update"));
    }
    let _lock = cache::lock_family(cache_root, family)?;
    let stage = cache::stage_dir(cache_root, family)?;
    // This entry accepts an external file, unlike acquire's existing stage.
    // Never recreate the stage after a download has written package.msi.
    std::fs::copy(msi_path, stage.join("package.msi"))?;
    if let Some(update) = runtime_update {
        cache::reject_reparse_path(update)?;
        std::fs::copy(update, stage.join("libeacnet.dll.update"))?;
    }
    publish_verified_stage(family, cache_root, &stage, runtime_update, &mut None)
}

fn publish_verified_stage(
    family: GameFamily,
    cache_root: &Path,
    stage: &Path,
    supplied_update: Option<&Path>,
    progress: &mut Option<&mut dyn FnMut(Progress)>,
) -> Result<RuntimePair, BootstrapError> {
    let package = package_for(family);
    cache::reject_reparse_path(stage)?;
    let staged_msi = stage.join("package.msi");
    cache::reject_reparse_path(&staged_msi)?;
    let digest = sha256_file(&staged_msi)?;
    if !digest.eq_ignore_ascii_case(package.sha256) {
        let _ = cache::remove_directory(stage);
        return Err(BootstrapError::PackageChanged { expected: package.sha256.to_string(), actual: digest });
    }

    let signer = verify_package(&staged_msi)?;
    let extracted = match extract_pair(&staged_msi, stage) {
        Ok(extracted) => extracted,
        Err(err) => {
            let _ = cache::remove_directory(stage);
            return Err(err);
        }
    };
    // The MSI check gates parsing. Each extracted DLL is checked again before
    // publish: a cabinet member that is not Authenticode-signed is discarded
    // with the stage and never renamed into the live pair.
    if let Err(err) = verify_package(&extracted.libeacnet).and_then(|_| verify_package(&extracted.avs2_core)) {
        let _ = cache::remove_directory(stage);
        return Err(err);
    }
    let extracted = match apply_runtime_update(package, stage, extracted, supplied_update, progress) {
        Ok(extracted) => extracted,
        Err(err) => {
            let _ = cache::remove_directory(stage);
            return Err(err);
        }
    };
    publish_pair(family, cache_root, package, &digest, signer.as_deref(), &extracted)
}

/// Replace the staged MSI `libeacnet.dll` only when it matches the bootstrap
/// pin and the sibling file matches the published pin and Authenticode.
///
/// The MSI member is checked first. A missing, wrong, or unsigned replacement
/// leaves that member unpublished and returns an error. Nothing here is executed.
fn apply_runtime_update(
    package: &OfficialPackage,
    stage: &Path,
    mut extracted: crate::extract::ExtractedPair,
    supplied_update: Option<&Path>,
    progress: &mut Option<&mut dyn FnMut(Progress)>,
) -> Result<crate::extract::ExtractedPair, BootstrapError> {
    let Some(update) = package.runtime_update else {
        return Ok(extracted);
    };
    if extracted.libeacnet_sha256 != update.bootstrap_sha256 || extracted.libeacnet_bytes != update.bootstrap_bytes {
        return Err(BootstrapError::PackageChanged { expected: update.bootstrap_sha256.to_string(), actual: extracted.libeacnet_sha256 });
    }
    let patch = stage.join("libeacnet.dll.update");
    cache::reject_reparse_path(&patch)?;
    // Production acquire has no local file. Download only after the extracted
    // member matched the bootstrap pin. The offline helper already copied one.
    if !patch.is_file() && supplied_update.is_none() {
        download_pinned_url(package.host, update.url, package.libeacnet_bytes, &patch, &mut |received, total| {
            report(progress, received, total)
        })?;
    }
    if !patch.is_file() {
        return Err(BootstrapError::cache("pinned current libeacnet.dll is missing from the stage"));
    }
    // Trust first so an unsigned stand-in is a signature failure, not a digest
    // mismatch. A signed file that is not the current pin still fails closed.
    verify_package(&patch)?;
    let patch_bytes = std::fs::metadata(&patch)?.len();
    let patch_sha = sha256_file(&patch)?;
    if patch_sha != package.libeacnet_sha256 || patch_bytes != package.libeacnet_bytes {
        return Err(BootstrapError::PackageChanged { expected: package.libeacnet_sha256.to_string(), actual: patch_sha });
    }
    // Windows rename does not replace an existing file. Copy onto the staged
    // member, then drop the sibling so publish sees only the current pin.
    std::fs::copy(&patch, &extracted.libeacnet)?;
    std::fs::remove_file(&patch)?;
    let replaced = sha256_file(&extracted.libeacnet)?;
    if replaced != package.libeacnet_sha256 {
        return Err(BootstrapError::cache("staged libeacnet.dll does not match the current pin after replacement"));
    }
    extracted.libeacnet_sha256 = replaced;
    extracted.libeacnet_bytes = package.libeacnet_bytes;
    Ok(extracted)
}

fn report(progress: &mut Option<&mut dyn FnMut(Progress)>, received: u64, total: Option<u64>) {
    if let Some(callback) = progress.as_mut() {
        callback(Progress { received, total });
    }
}

/// Authenticode-check a package before it is parsed.
///
/// On Windows this is `WinVerifyTrust` with the generic Authenticode action,
/// no UI, and no network revocation lookup. Other platforms cannot reach this
/// function: [`names::require_windows_x64`] fails first.
fn verify_package(path: &Path) -> Result<Option<String>, BootstrapError> {
    #[cfg(windows)]
    {
        trust::verify_authenticode(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(BootstrapError::UnsupportedPlatform)
    }
}

/// Crate-private entry used only by the ignored offline integration test.
///
/// Still requires the recorded digest and a trusted Authenticode result.
/// Not a way for the UI to accept an arbitrary file.
pub mod test_support {
    use std::path::Path;

    use crate::{BootstrapError, GameFamily, RuntimePair};

    pub fn acquire_from_verified_file(family: GameFamily, cache_root: &Path, msi_path: &Path) -> Result<RuntimePair, BootstrapError> {
        super::acquire_from_verified_files(family, cache_root, msi_path, None)
    }

    /// Offline entry. `update == None` does not open a socket. SDVX fails closed
    /// without the pinned current `libeacnet.dll`.
    pub fn acquire_from_verified_files(
        family: GameFamily,
        cache_root: &Path,
        msi_path: &Path,
        update: Option<&Path>,
    ) -> Result<RuntimePair, BootstrapError> {
        super::acquire_from_verified_files(family, cache_root, msi_path, update)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_refuses_without_consent() {
        let root = tempfile::tempdir().unwrap();
        let err = acquire(GameFamily::Bombergirl, root.path(), false, None).unwrap_err();
        assert!(matches!(err, BootstrapError::ConsentRequired));
    }

    #[test]
    fn downloaded_stage_is_verified_without_recreating_it() {
        let root = tempfile::tempdir().unwrap();
        let _lock = cache::lock_family(root.path(), GameFamily::Bombergirl).unwrap();
        let stage = cache::stage_dir(root.path(), GameFamily::Bombergirl).unwrap();
        std::fs::write(stage.join("package.msi"), b"synthetic-not-the-official-pin").unwrap();
        let err = publish_verified_stage(GameFamily::Bombergirl, root.path(), &stage, None, &mut None).unwrap_err();
        assert!(matches!(err, BootstrapError::PackageChanged { .. }), "{err}");
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    fn overlapping_public_cache_operations_are_refused_without_mutation() {
        let root = tempfile::tempdir().unwrap();
        let lock = cache::lock_family(root.path(), GameFamily::Bombergirl).unwrap();
        let stage = cache::stage_dir(root.path(), GameFamily::Bombergirl).unwrap();
        let marker = stage.join("in-progress");
        std::fs::write(&marker, b"owned-by-first-call").unwrap();
        let worker_root = root.path().to_path_buf();
        std::thread::spawn(move || {
            let err = acquire(GameFamily::Bombergirl, &worker_root, true, None).unwrap_err();
            assert!(matches!(err, BootstrapError::Cache(_)), "{err}");
            let err = inspect(GameFamily::Bombergirl, &worker_root).unwrap_err();
            assert!(matches!(err, BootstrapError::Cache(_)), "{err}");
        })
        .join()
        .unwrap();
        assert_eq!(std::fs::read(&marker).unwrap(), b"owned-by-first-call");
        assert!(cache::lock_family(root.path(), GameFamily::Sdvx).is_ok());
        drop(lock);
        // The lock file remains on disk but is not a stale marker lock.
        assert!(root.path().join("bombergirl/bootstrap.lock").is_file());
        assert!(matches!(inspect(GameFamily::Bombergirl, root.path()).unwrap(), PairStatus::Missing));
        assert_eq!(std::fs::read(&marker).unwrap(), b"owned-by-first-call");
    }

    #[cfg(not(all(windows, target_arch = "x86_64")))]
    #[test]
    fn acquire_refuses_this_platform_even_with_consent() {
        let root = tempfile::tempdir().unwrap();
        let err = acquire(GameFamily::Sdvx, root.path(), true, None).unwrap_err();
        assert!(matches!(err, BootstrapError::UnsupportedPlatform));
    }

    /// Families whose published `libeacnet.dll` is not the MSI member.
    /// Paths come from the environment. These tests are ignored unless run
    /// explicitly; they do not embed a machine path and do not skip quietly.
    #[cfg(all(windows, target_arch = "x86_64"))]
    fn overlay_cases() -> [(GameFamily, &'static str, &'static str); 2] {
        [
            (GameFamily::Sdvx, "KONASTE_OFFLINE_SDVX_MSI", "KONASTE_OFFLINE_SDVX_EACNET"),
            (GameFamily::Popn, "KONASTE_OFFLINE_POPN_MSI", "KONASTE_OFFLINE_POPN_EACNET"),
        ]
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    fn retained_overlay(msi_var: &str, dll_var: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let msi = std::env::var_os(msi_var).unwrap_or_else(|| panic!("{msi_var} is required"));
        let current = std::env::var_os(dll_var).unwrap_or_else(|| panic!("{dll_var} is required"));
        (msi.into(), current.into())
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    fn overlay_without_the_current_dll_does_not_publish_the_msi_member() {
        for (family, _, _) in overlay_cases() {
            let root = tempfile::tempdir().unwrap();
            let msi = root.path().join("not-a-retained-msi.msi");
            let err = test_support::acquire_from_verified_file(family, root.path(), &msi).unwrap_err();
            assert!(matches!(err, BootstrapError::Cache(_)), "{family:?} {err}");
            assert!(matches!(inspect(family, root.path()).unwrap(), PairStatus::Missing), "{family:?}");
            assert!(!root.path().join(family.as_str()).join("pair").exists());
        }
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    #[ignore = "needs KONASTE_OFFLINE_SDVX_MSI/EACNET and KONASTE_OFFLINE_POPN_MSI/EACNET; no network"]
    fn overlay_rejects_the_old_msi_member_as_the_current_dll() {
        for (family, msi_var, _) in overlay_cases() {
            let msi = std::path::PathBuf::from(std::env::var_os(msi_var).unwrap_or_else(|| panic!("{msi_var} is required")));
            verify_package(&msi).unwrap();
            let old_directory = tempfile::tempdir().unwrap();
            let old = extract_pair(&msi, old_directory.path()).unwrap();
            verify_package(&old.libeacnet).unwrap();
            let root = tempfile::tempdir().unwrap();
            let err = test_support::acquire_from_verified_files(family, root.path(), &msi, Some(&old.libeacnet)).unwrap_err();
            assert!(matches!(err, BootstrapError::PackageChanged { .. }), "{family:?} {err}");
            assert!(matches!(inspect(family, root.path()).unwrap(), PairStatus::Missing));
        }
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    #[ignore = "needs KONASTE_OFFLINE_SDVX_MSI and KONASTE_OFFLINE_POPN_MSI; no network"]
    fn overlay_rejects_an_unsigned_stand_in_before_publish() {
        for (family, msi_var, _) in overlay_cases() {
            let msi = std::path::PathBuf::from(std::env::var_os(msi_var).unwrap_or_else(|| panic!("{msi_var} is required")));
            let root = tempfile::tempdir().unwrap();
            let unsigned = root.path().join("unsigned.dll");
            // Not Authenticode-signed. Flipping the last byte of a real current
            // DLL can still return WinVerifyTrust 0, so that is a digest case.
            std::fs::write(&unsigned, b"MZ unsigned stand-in").unwrap();
            let err = test_support::acquire_from_verified_files(family, root.path(), &msi, Some(&unsigned)).unwrap_err();
            assert!(matches!(err, BootstrapError::Signature(_)), "{family:?} {err}");
            assert!(!root.path().join(family.as_str()).join("pair/libeacnet.dll").exists());
        }
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    #[ignore = "needs KONASTE_OFFLINE_SDVX_MSI/EACNET and KONASTE_OFFLINE_POPN_MSI/EACNET; no network"]
    fn overlay_failure_does_not_replace_a_published_current_pair() {
        for (family, msi_var, dll_var) in overlay_cases() {
            let (msi, current) = retained_overlay(msi_var, dll_var);
            let root = tempfile::tempdir().unwrap();
            let pair = test_support::acquire_from_verified_files(family, root.path(), &msi, Some(&current)).unwrap();
            let package = package_for(family);
            let published = std::fs::read(&pair.libeacnet).unwrap();
            assert_eq!(published.len() as u64, package.libeacnet_bytes);
            let wrong = root.path().join("mismatch.dll");
            std::fs::write(&wrong, &published[..published.len() - 1]).unwrap();
            let err = test_support::acquire_from_verified_files(family, root.path(), &msi, Some(&wrong)).unwrap_err();
            assert!(matches!(err, BootstrapError::Signature(_) | BootstrapError::PackageChanged { .. }), "{family:?} {err}");
            assert_eq!(std::fs::read(&pair.libeacnet).unwrap(), published);
            match inspect(family, root.path()).unwrap() {
                PairStatus::Present(found) => {
                    assert_eq!(found.provenance.runtime_update_sha256.as_deref(), Some(package.libeacnet_sha256));
                    assert_eq!(found.provenance.avs2_core_bytes, package.avs2_core_bytes);
                }
                other => panic!("{family:?} published current pair was replaced: {other:?}"),
            }
        }
    }
}
