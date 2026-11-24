//! The pinned INFINITAS launcher MSI. It is not a `GameFamily` runtime pair
//! and does not include the game data. This module does not run anything.

use std::path::Path;

use crate::{BootstrapError, Progress, fetch, hashutil, names, verify_package};

pub const MSI_BYTES: u64 = 44_212_224;
const MSI_HOST: &str = "d1rc4pwxnc0pe0.cloudfront.net";
const MSI_URL: &str = "https://d1rc4pwxnc0pe0.cloudfront.net/v2/installer/infinitas_installer_2022060800.msi";
const MSI_SHA256: &str = "9de019339a24e36ae54283bab6cb5602922a4ceae7d1a29d847c28f01e195dc2";

/// Downloads the one recorded MSI. `scratch` must be a fresh, app-owned directory.
/// The caller must hold a read-only file pin and call [`verify`] before using it.
pub fn download(destination: &Path, consent: bool, mut progress: impl FnMut(Progress)) -> Result<(), BootstrapError> {
    if !consent {
        return Err(BootstrapError::ConsentRequired);
    }
    names::require_windows_x64()?;
    fetch::download_pinned_url(MSI_HOST, MSI_URL, MSI_BYTES, destination, &mut |received, _| {
        progress(Progress { received, total: Some(MSI_BYTES) });
    })
}

/// Verify exact bytes and the Windows Authenticode policy. Never runs the MSI.
/// A failed signature is not accepted even with a matching hash.
pub fn verify(msi: &Path) -> Result<(), BootstrapError> {
    names::require_windows_x64()?;
    crate::cache::reject_reparse_path(msi)?;
    let metadata = std::fs::metadata(msi)?;
    if !metadata.is_file() || metadata.len() != MSI_BYTES {
        return Err(BootstrapError::fetch("INFINITAS MSI size differs from its pin"));
    }
    let actual = hashutil::sha256_file(msi)?;
    if actual != MSI_SHA256 {
        return Err(BootstrapError::PackageChanged { expected: MSI_SHA256.into(), actual });
    }
    verify_package(msi)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_consent_does_not_download_or_touch_the_destination() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("launcher.msi");
        let error = download(&path, false, |_| panic!("unexpected download progress")).unwrap_err();
        assert!(matches!(error, BootstrapError::ConsentRequired));
        assert!(!path.exists());
        assert!(!path.with_extension("msi.partial").exists());
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    #[ignore = "downloads the public 44 MB MSI into a temporary directory; never executes it"]
    fn fresh_infinitas_msi_download_matches_size_hash_and_signature() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("infinitas.msi");
        let mut received = 0;
        download(&path, true, |progress| received = progress.received).unwrap();
        assert_eq!(received, MSI_BYTES);
        verify(&path).unwrap();
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    #[ignore = "needs KONASTE_OFFLINE_INFINITAS_MSI; read-only, no execution"]
    fn retained_infinitas_msi_matches_the_size_hash_and_windows_signature() {
        let path = std::env::var_os("KONASTE_OFFLINE_INFINITAS_MSI").expect("set the retained MSI path");
        verify(Path::new(&path)).unwrap();
    }
}
