//! Offline check of the already-downloaded official MSIs.
//!
//! Ignored by default. It does not fetch, and it does not embed a machine path.
//! Point the environment variables at the trusted local copies, then run:
//!
//! ```text
//! cargo test -p konaste-bootstrap --test offline_official -- --ignored --nocapture
//! ```
//!
//! `KONASTE_OFFLINE_BG_MSI` is the Bombergirl installer.
//! `KONASTE_OFFLINE_SDVX_MSI` is the SDVX installer.
//! `KONASTE_OFFLINE_SDVX_EACNET` is the pinned current SDVX `libeacnet.dll`.
//! SDVX requires both. The MSI alone is not published.
//! `KONASTE_OFFLINE_DDR_MSI` is the DDR installer.
//! `KONASTE_OFFLINE_POPN_MSI` is the pop'n installer.
//! `KONASTE_OFFLINE_POPN_EACNET` is the pinned current pop'n `libeacnet.dll`.
//! pop'n requires both. The MSI alone is not published.
//! `KONASTE_OFFLINE_GITADORA_MSI` is the GITADORA installer.
//! `KONASTE_OFFLINE_NOSTALGIA_MSI` is the NOSTALGIA installer.
//! A variable that is unset skips that title. The test copies each MSI into a
//! temp cache, checks the recorded SHA-256 and Authenticode, and extracts the
//! two DLLs. It does not run the MSI or either DLL.

#![cfg(all(windows, target_arch = "x86_64"))]

use std::path::PathBuf;

use konaste_bootstrap::{GameFamily, PairStatus, inspect};

#[test]
#[ignore = "needs KONASTE_OFFLINE_*_MSI; no network"]
fn pinned_local_msis_extract_the_runtime_pair() {
    let cases = [
        ("KONASTE_OFFLINE_BG_MSI", GameFamily::Bombergirl),
        ("KONASTE_OFFLINE_SDVX_MSI", GameFamily::Sdvx),
        ("KONASTE_OFFLINE_DDR_MSI", GameFamily::Ddr),
        ("KONASTE_OFFLINE_POPN_MSI", GameFamily::Popn),
        ("KONASTE_OFFLINE_GITADORA_MSI", GameFamily::Gitadora),
        ("KONASTE_OFFLINE_NOSTALGIA_MSI", GameFamily::Nostalgia),
    ];
    let mut ran = 0_u32;
    for (var, family) in cases {
        let Some(path) = std::env::var_os(var) else {
            eprintln!("{var} unset; skipping {family:?}");
            continue;
        };
        let source = PathBuf::from(path);
        assert!(source.is_file(), "{var} is not a file: {}", source.display());
        let update_var = match family {
            GameFamily::Sdvx => Some("KONASTE_OFFLINE_SDVX_EACNET"),
            GameFamily::Popn => Some("KONASTE_OFFLINE_POPN_EACNET"),
            GameFamily::Bombergirl | GameFamily::Ddr | GameFamily::Gitadora | GameFamily::Nostalgia => None,
        };
        let update = update_var.map(|update_var| {
            let update = std::env::var_os(update_var).unwrap_or_else(|| panic!("{var} is set but {update_var} is not"));
            let update = PathBuf::from(update);
            assert!(update.is_file(), "{update_var} is not a file");
            update
        });
        let cache = tempfile::tempdir().expect("temp cache");
        let pair = konaste_bootstrap::test_support::acquire_from_verified_files(family, cache.path(), &source, update.as_deref())
            .unwrap_or_else(|err| panic!("{family:?} from {}: {err}", source.display()));

        assert_eq!(pair.package_sha256, konaste_bootstrap::package_for(family).sha256);
        // WinVerifyTrust does not return a signer name. The pin is the digest.
        assert!(pair.provenance.signer.is_none());
        assert!(pair.libeacnet.is_file());
        assert!(pair.avs2_core.is_file());
        assert!(pair.libeacnet.starts_with(cache.path()));
        assert!(pair.avs2_core.starts_with(cache.path()));
        assert!(pair.provenance.libeacnet_bytes > 0);
        assert!(pair.provenance.avs2_core_bytes > 0);
        use sha2::Digest;
        let package = konaste_bootstrap::package_for(family);
        assert_eq!(hex::encode(sha2::Sha256::digest(std::fs::read(&pair.libeacnet).unwrap())), package.libeacnet_sha256);
        assert_eq!(hex::encode(sha2::Sha256::digest(std::fs::read(&pair.avs2_core).unwrap())), package.avs2_core_sha256);
        assert_eq!(pair.provenance.libeacnet_bytes, package.libeacnet_bytes);
        assert_eq!(pair.provenance.avs2_core_bytes, package.avs2_core_bytes);
        assert_eq!(pair.provenance.runtime_update_url.as_deref(), package.runtime_update.map(|update| update.url));
        assert_eq!(pair.provenance.runtime_update_sha256.as_deref(), package.runtime_update.map(|_| package.libeacnet_sha256));
        match inspect(family, cache.path()).expect("inspect") {
            PairStatus::Present(found) => assert_eq!(found, pair),
            other => panic!("published pair did not inspect clean: {other:?}"),
        }
        let family_root = pair.libeacnet.parent().unwrap().parent().unwrap();
        std::fs::rename(family_root.join("pair"), family_root.join("previous")).unwrap();
        match inspect(family, cache.path()).expect("restore verified previous") {
            PairStatus::Present(found) => assert_eq!(found, pair),
            other => panic!("verified previous pair was not restored: {other:?}"),
        }
        assert!(!family_root.join("previous").exists());
        assert!(pair.libeacnet.is_file());
        assert!(pair.avs2_core.is_file());
        ran += 1;
    }
    assert!(ran > 0, "set at least one KONASTE_OFFLINE_*_MSI");
}
