//! App-owned cache of one independently pinned runtime pair per family.
//!
//! The cache lock spans inspection, extraction, and paired publication. A
//! verified previous pair can be restored after an interrupted rename. Path
//! checks reject existing symlinks/reparse points, but do not pin directory
//! handles against concurrent hostile path replacement.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalog::OfficialPackage;
use crate::error::BootstrapError;
use crate::extract::ExtractedPair;
use crate::hashutil::sha256_file;
use crate::names::GameFamily;
use crate::{PairStatus, Provenance, RuntimePair};

const PAIR_DIR: &str = "pair";
const STAGING_DIR: &str = "staging";
const MANIFEST: &str = "pair.json";
const MAX_MANIFEST_BYTES: u64 = 16 * 1024;

type VerifySignature<'a> = dyn Fn(&Path) -> Result<Option<String>, BootstrapError> + 'a;

/// The OS owns this lock, so a crash does not leave a stale marker lock.
/// Never delete the lock file: every caller must lock the same file object.
#[derive(Debug)]
pub(crate) struct CacheLock {
    _file: File,
}

pub(crate) fn lock_family(cache_root: &Path, family: GameFamily) -> Result<CacheLock, BootstrapError> {
    let root = family_root(cache_root, family)?;
    fs::create_dir_all(&root)?;
    reject_reparse_path(&root)?;
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(root.join("bootstrap.lock"))?;
    if is_reparse(&file.metadata()?) {
        return Err(BootstrapError::cache("cache lock is a reparse point"));
    }
    fs2::FileExt::try_lock_exclusive(&file).map_err(|_| BootstrapError::cache("bootstrap for this title is already active"))?;
    Ok(CacheLock { _file: file })
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    family: GameFamily,
    package_url: String,
    package_sha256: String,
    signer: Option<String>,
    libeacnet_sha256: String,
    avs2_core_sha256: String,
    libeacnet_bytes: u64,
    avs2_core_bytes: u64,
    /// Must equal the current package pin on inspection.
    runtime_update_url: Option<String>,
    runtime_update_sha256: Option<String>,
}

pub(crate) fn stage_dir(cache_root: &Path, family: GameFamily) -> Result<PathBuf, BootstrapError> {
    let dir = family_root(cache_root, family)?.join(STAGING_DIR);
    remove_directory(&dir)?;
    fs::create_dir_all(&dir)?;
    reject_reparse_path(&dir)?;
    Ok(dir)
}

pub(crate) fn publish_pair(
    family: GameFamily,
    cache_root: &Path,
    package: &OfficialPackage,
    package_sha256: &str,
    signer: Option<&str>,
    extracted: &ExtractedPair,
) -> Result<RuntimePair, BootstrapError> {
    publish_pair_with(family, cache_root, package, package_sha256, signer, extracted, &crate::verify_package)
}

fn publish_pair_with(
    family: GameFamily,
    cache_root: &Path,
    package: &OfficialPackage,
    package_sha256: &str,
    signer: Option<&str>,
    extracted: &ExtractedPair,
    verify: &VerifySignature<'_>,
) -> Result<RuntimePair, BootstrapError> {
    if package.family != family
        || package_sha256 != package.sha256
        || signer.is_some()
        || extracted.libeacnet_sha256 != package.libeacnet_sha256
        || extracted.avs2_core_sha256 != package.avs2_core_sha256
        || extracted.libeacnet_bytes != package.libeacnet_bytes
        || extracted.avs2_core_bytes != package.avs2_core_bytes
    {
        return Err(BootstrapError::cache("extracted runtime does not match the independent package pins"));
    }
    let root = family_root(cache_root, family)?;
    // Restore a verified backup before anything can discard it.
    let _ = inspect_locked_with(family, cache_root, package, verify)?;
    let incoming = root.join("incoming");
    remove_directory(&incoming)?;
    fs::create_dir_all(&incoming)?;
    reject_reparse_path(&extracted.libeacnet)?;
    reject_reparse_path(&extracted.avs2_core)?;
    let libeacnet = incoming.join("libeacnet.dll");
    let avs2 = incoming.join("avs2-core.dll");
    fs::copy(&extracted.libeacnet, &libeacnet)?;
    fs::copy(&extracted.avs2_core, &avs2)?;
    let lib_hash = sha256_file(&libeacnet)?;
    let avs_hash = sha256_file(&avs2)?;
    if lib_hash != package.libeacnet_sha256 || avs_hash != package.avs2_core_sha256 {
        remove_directory(&incoming)?;
        return Err(BootstrapError::cache("copied runtime does not match the independent package pins"));
    }
    let manifest = Manifest {
        family,
        package_url: package.url.to_string(),
        package_sha256: package.sha256.to_string(),
        signer: None,
        libeacnet_sha256: lib_hash,
        avs2_core_sha256: avs_hash,
        libeacnet_bytes: package.libeacnet_bytes,
        avs2_core_bytes: package.avs2_core_bytes,
        runtime_update_url: package.runtime_update.map(|update| update.url.to_string()),
        runtime_update_sha256: package.runtime_update.map(|_| package.libeacnet_sha256.to_string()),
    };
    write_manifest(&incoming.join(MANIFEST), &manifest)?;
    match inspect_directory(&incoming, family, package, verify)? {
        PairStatus::Present(_) => {}
        _ => {
            return Err(BootstrapError::cache("incoming pair failed independent validation"));
        }
    }
    for path in [&libeacnet, &avs2] {
        reject_reparse_path(path)?;
        OpenOptions::new().read(true).write(true).open(path)?.sync_all()?;
    }
    let live = root.join(PAIR_DIR);
    let backup = root.join("previous");
    remove_directory(&backup)?;
    if exists(&live)? {
        rename_directory(&live, &backup)?;
    }
    if let Err(err) = rename_directory(&incoming, &live) {
        if exists(&backup)? && !exists(&live)? {
            let _ = inspect_locked_with(family, cache_root, package, verify);
        }
        return Err(err);
    }
    // Validate at the returned path too, rather than returning unchecked JSON.
    let pair = match inspect_directory(&live, family, package, verify)? {
        PairStatus::Present(pair) => pair,
        _ => {
            return Err(BootstrapError::cache("published pair failed independent validation"));
        }
    };
    let _ = remove_directory(&backup);
    let _ = remove_directory(&root.join(STAGING_DIR));
    Ok(pair)
}

/// Inspect or restore a pinned pair without downloading. Both cached DLLs
/// must match independent catalog pins and pass Authenticode again.
/// Refuses while another operation owns this family's cache lock.
pub fn inspect(family: GameFamily, cache_root: &Path) -> Result<PairStatus, BootstrapError> {
    let _lock = lock_family(cache_root, family)?;
    inspect_locked(family, cache_root)
}

pub(crate) fn inspect_locked(family: GameFamily, cache_root: &Path) -> Result<PairStatus, BootstrapError> {
    inspect_locked_with(family, cache_root, crate::catalog::package_for(family), &crate::verify_package)
}

fn inspect_locked_with(
    family: GameFamily,
    cache_root: &Path,
    package: &OfficialPackage,
    verify: &VerifySignature<'_>,
) -> Result<PairStatus, BootstrapError> {
    let root = family_root(cache_root, family)?;
    let live = root.join(PAIR_DIR);
    if exists(&live)? {
        return inspect_directory(&live, family, package, verify);
    }
    let backup = root.join("previous");
    match inspect_directory(&backup, family, package, verify)? {
        PairStatus::Present(_) => {
            rename_directory(&backup, &live)?;
            inspect_directory(&live, family, package, verify)
        }
        PairStatus::Stale { reason } => Ok(PairStatus::Stale { reason: format!("previous pair was not restored: {reason}") }),
        PairStatus::Missing => Ok(PairStatus::Missing),
    }
}

fn inspect_directory(
    directory: &Path,
    family: GameFamily,
    package: &OfficialPackage,
    verify: &VerifySignature<'_>,
) -> Result<PairStatus, BootstrapError> {
    reject_existing_tree(directory)?;
    if !exists(directory)? {
        return Ok(PairStatus::Missing);
    }
    let stale = |reason: &str| Ok(PairStatus::Stale { reason: reason.to_string() });
    if !directory.is_dir() || !directory.join(MANIFEST).is_file() {
        return stale("cached pair has no complete manifest");
    }
    let manifest = match read_manifest(&directory.join(MANIFEST)) {
        Ok(manifest) => manifest,
        Err(_) => return stale("pair manifest is unreadable"),
    };
    if package.family != family
        || manifest.family != family
        || manifest.package_sha256 != package.sha256
        || manifest.package_url != package.url
        || manifest.signer.is_some()
        || manifest.libeacnet_sha256 != package.libeacnet_sha256
        || manifest.avs2_core_sha256 != package.avs2_core_sha256
        || manifest.libeacnet_bytes != package.libeacnet_bytes
        || manifest.avs2_core_bytes != package.avs2_core_bytes
        || manifest.runtime_update_url.as_deref() != package.runtime_update.map(|update| update.url)
        || manifest.runtime_update_sha256.as_deref() != package.runtime_update.map(|_| package.libeacnet_sha256)
    {
        return stale("cached provenance does not match the independent package pins");
    }
    let libeacnet = directory.join("libeacnet.dll");
    let avs2 = directory.join("avs2-core.dll");
    for (path, size, digest) in
        [(&libeacnet, package.libeacnet_bytes, package.libeacnet_sha256), (&avs2, package.avs2_core_bytes, package.avs2_core_sha256)]
    {
        reject_reparse_path(path)?;
        let metadata = match fs::metadata(path) {
            Ok(metadata) if metadata.is_file() && metadata.len() == size => metadata,
            _ => return stale("cached runtime size does not match the independent package pin"),
        };
        if is_reparse(&metadata) {
            return Err(BootstrapError::cache("cached runtime is a reparse point"));
        }
        match sha256_file(path) {
            Ok(actual) if actual == digest => {}
            _ => return stale("cached DLL bytes do not match the independent package pins"),
        }
        match verify(path) {
            Ok(None) => {}
            _ => return stale("cached runtime signature was rejected"),
        }
    }
    Ok(PairStatus::Present(pair_from(directory, &manifest)))
}

fn pair_from(live: &Path, manifest: &Manifest) -> RuntimePair {
    RuntimePair {
        family: manifest.family,
        libeacnet: live.join("libeacnet.dll"),
        avs2_core: live.join("avs2-core.dll"),
        package_sha256: manifest.package_sha256.clone(),
        provenance: Box::new(Provenance {
            package_url: manifest.package_url.clone(),
            package_sha256: manifest.package_sha256.clone(),
            signer: None,
            libeacnet_bytes: manifest.libeacnet_bytes,
            avs2_core_bytes: manifest.avs2_core_bytes,
            runtime_update_url: manifest.runtime_update_url.clone(),
            runtime_update_sha256: manifest.runtime_update_sha256.clone(),
        }),
    }
}

fn family_root(cache_root: &Path, family: GameFamily) -> Result<PathBuf, BootstrapError> {
    if !cache_root.is_absolute() || cache_root.components().any(|part| matches!(part, Component::ParentDir)) {
        return Err(BootstrapError::cache("cache root must be an absolute app-owned directory"));
    }
    reject_reparse_path(cache_root)?;
    let root = cache_root.join(family.as_str());
    reject_existing_tree(&root)?;
    Ok(root)
}

fn exists(path: &Path) -> Result<bool, BootstrapError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_reparse(&metadata) => Err(BootstrapError::cache("cache path is a symlink or reparse point")),
        Ok(_) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

pub(crate) fn reject_reparse_path(path: &Path) -> Result<(), BootstrapError> {
    let ancestors: Vec<_> = path.ancestors().collect();
    for ancestor in ancestors.into_iter().rev() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if is_reparse(&metadata) => {
                return Err(BootstrapError::cache("cache path is a symlink or reparse point"));
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => break,
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}

fn reject_existing_tree(path: &Path) -> Result<(), BootstrapError> {
    reject_reparse_path(path)?;
    let mut pending = vec![path.to_path_buf()];
    let mut checked = 0_usize;
    while let Some(entry) = pending.pop() {
        let metadata = match fs::symlink_metadata(&entry) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err.into()),
        };
        if is_reparse(&metadata) {
            return Err(BootstrapError::cache("cache tree contains a symlink or reparse point"));
        }
        checked += 1;
        if checked > 4096 {
            return Err(BootstrapError::cache("cache tree has too many entries"));
        }
        if metadata.is_dir() {
            for child in fs::read_dir(&entry)? {
                pending.push(child?.path());
            }
        }
    }
    Ok(())
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub(crate) fn remove_directory(path: &Path) -> Result<(), BootstrapError> {
    reject_existing_tree(path)?;
    if exists(path)? {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn rename_directory(from: &Path, to: &Path) -> Result<(), BootstrapError> {
    reject_existing_tree(from)?;
    reject_existing_tree(to)?;
    fs::rename(from, to)?;
    Ok(())
}

fn write_manifest(path: &Path, manifest: &Manifest) -> Result<(), BootstrapError> {
    reject_reparse_path(path)?;
    let json = serde_json::to_vec_pretty(manifest).map_err(|err| BootstrapError::cache(format!("manifest encode failed: {err}")))?;
    let mut file = File::create(path)?;
    file.write_all(&json)?;
    file.sync_all()?;
    Ok(())
}

fn read_manifest(path: &Path) -> Result<Manifest, BootstrapError> {
    reject_reparse_path(path)?;
    let file = File::open(path)?;
    let mut buf = Vec::new();
    file.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(BootstrapError::cache("pair manifest is too large"));
    }
    serde_json::from_slice(&buf).map_err(|err| BootstrapError::cache(format!("manifest decode failed: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::BOMBERGIRL;
    use crate::extract::synth;

    const SYNTHETIC: OfficialPackage = OfficialPackage {
        libeacnet_sha256: "e5039f30b7140dde5fdf7b3aec84b67a83fccc11a135aa3961ad9947f433b031",
        avs2_core_sha256: "405d07d05f1788b0f07064cee505f9732a20707609628b72802481b9352729e3",
        libeacnet_bytes: 12,
        avs2_core_bytes: 9,
        ..BOMBERGIRL
    };

    fn unsigned_fixture_only(_: &Path) -> Result<Option<String>, BootstrapError> {
        Ok(None)
    }

    fn published(root: &Path) -> RuntimePair {
        let made = synth::package(b"eacnet-cache", b"avs-cache", None);
        let stage = stage_dir(root, GameFamily::Bombergirl).unwrap();
        let msi = stage.join("package.msi");
        fs::write(&msi, made.bytes).unwrap();
        let extracted = crate::extract::extract_pair(&msi, &stage).unwrap();
        publish_pair_with(GameFamily::Bombergirl, root, &SYNTHETIC, SYNTHETIC.sha256, None, &extracted, &unsigned_fixture_only).unwrap()
    }

    fn inspect_fixture(root: &Path) -> PairStatus {
        let _lock = lock_family(root, GameFamily::Bombergirl).unwrap();
        inspect_locked_with(GameFamily::Bombergirl, root, &SYNTHETIC, &unsigned_fixture_only).unwrap()
    }

    #[test]
    fn inspect_validates_a_published_pair_and_rechecks_signatures() {
        let root = tempfile::tempdir().unwrap();
        let pair = published(root.path());
        let calls = std::cell::Cell::new(0);
        let check = |_: &Path| {
            calls.set(calls.get() + 1);
            Ok(None)
        };
        let _lock = lock_family(root.path(), GameFamily::Bombergirl).unwrap();
        match inspect_locked_with(GameFamily::Bombergirl, root.path(), &SYNTHETIC, &check).unwrap() {
            PairStatus::Present(found) => assert_eq!(found, pair),
            other => panic!("expected present, got {other:?}"),
        }
        assert_eq!(calls.get(), 2);
        assert!(matches!(inspect(GameFamily::Sdvx, root.path()).unwrap(), PairStatus::Missing));
    }

    #[test]
    fn cached_signature_failure_is_stale() {
        let root = tempfile::tempdir().unwrap();
        published(root.path());
        let check = |_: &Path| Err(BootstrapError::signature("synthetic rejection"));
        let _lock = lock_family(root.path(), GameFamily::Bombergirl).unwrap();
        assert!(matches!(inspect_locked_with(GameFamily::Bombergirl, root.path(), &SYNTHETIC, &check).unwrap(), PairStatus::Stale { .. }));
    }

    #[test]
    fn edited_manifest_hashes_cannot_authorize_replacement_dlls() {
        let root = tempfile::tempdir().unwrap();
        let pair = published(root.path());
        fs::write(&pair.libeacnet, b"forged-cache").unwrap();
        let path = pair.libeacnet.parent().unwrap().join(MANIFEST);
        let mut manifest = read_manifest(&path).unwrap();
        manifest.libeacnet_sha256 = sha256_file(&pair.libeacnet).unwrap();
        manifest.libeacnet_bytes = 12;
        write_manifest(&path, &manifest).unwrap();
        match inspect_fixture(root.path()) {
            PairStatus::Stale { reason } => {
                assert!(reason.contains("independent package pins"), "{reason}")
            }
            other => panic!("expected stale, got {other:?}"),
        }
        assert!(matches!(inspect(GameFamily::Bombergirl, root.path()).unwrap(), PairStatus::Stale { .. }));
    }

    #[test]
    fn tampered_dll_is_stale_not_present() {
        let root = tempfile::tempdir().unwrap();
        let pair = published(root.path());
        fs::write(&pair.libeacnet, b"same-sizebad").unwrap();
        assert!(matches!(inspect_fixture(root.path()), PairStatus::Stale { .. }));
    }

    #[test]
    fn cached_provenance_must_match_every_pin() {
        for field in [
            "family",
            "package_url",
            "package_sha256",
            "signer",
            "avs2_core_bytes",
            "avs2_core_sha256",
            "runtime_update_url",
            "runtime_update_sha256",
        ] {
            let root = tempfile::tempdir().unwrap();
            let pair = published(root.path());
            let path = pair.libeacnet.parent().unwrap().join(MANIFEST);
            let mut manifest = read_manifest(&path).unwrap();
            match field {
                "family" => manifest.family = GameFamily::Sdvx,
                "package_url" => manifest.package_url.push_str("?forged"),
                "package_sha256" => manifest.package_sha256 = "00".repeat(32),
                "signer" => manifest.signer = Some("unverified display name".to_string()),
                "avs2_core_bytes" => manifest.avs2_core_bytes += 1,
                "avs2_core_sha256" => manifest.avs2_core_sha256 = "00".repeat(32),
                "runtime_update_url" => manifest.runtime_update_url = Some("https://example.invalid/libeacnet.dll".to_string()),
                "runtime_update_sha256" => manifest.runtime_update_sha256 = Some("00".repeat(32)),
                _ => unreachable!(),
            }
            write_manifest(&path, &manifest).unwrap();
            assert!(matches!(inspect_fixture(root.path()), PairStatus::Stale { .. }), "{field}");
        }
    }

    #[test]
    fn interrupted_publish_restores_only_a_verified_previous_pair() {
        let root = tempfile::tempdir().unwrap();
        let pair = published(root.path());
        let family = root.path().join("bombergirl");
        fs::rename(family.join(PAIR_DIR), family.join("previous")).unwrap();
        fs::create_dir(family.join("incoming")).unwrap();
        fs::write(family.join("incoming/libeacnet.dll"), b"partial incoming").unwrap();
        match inspect_fixture(root.path()) {
            PairStatus::Present(found) => assert_eq!(found, pair),
            other => panic!("expected restored pair, got {other:?}"),
        }
        assert!(!family.join("previous").exists());
        assert_eq!(fs::read(pair.libeacnet).unwrap(), b"eacnet-cache");
        assert_eq!(fs::read(family.join("incoming/libeacnet.dll")).unwrap(), b"partial incoming");
    }

    #[test]
    fn invalid_previous_pair_is_never_restored_or_removed() {
        let root = tempfile::tempdir().unwrap();
        let pair = published(root.path());
        let family = root.path().join("bombergirl");
        fs::rename(family.join(PAIR_DIR), family.join("previous")).unwrap();
        let invalid = family.join("previous/libeacnet.dll");
        fs::write(&invalid, b"tampered").unwrap();
        assert!(matches!(inspect_fixture(root.path()), PairStatus::Stale { .. }));
        assert!(!pair.libeacnet.exists());
        assert_eq!(fs::read(invalid).unwrap(), b"tampered");
    }

    #[test]
    fn publish_failure_does_not_replace_a_good_pair() {
        let root = tempfile::tempdir().unwrap();
        let pair = published(root.path());
        let good = fs::read(&pair.libeacnet).unwrap();
        let made = synth::package(b"new-eacnet", b"new-avs", None);
        let stage = stage_dir(root.path(), GameFamily::Bombergirl).unwrap();
        let msi = stage.join("package.msi");
        fs::write(&msi, made.bytes).unwrap();
        let extracted = crate::extract::extract_pair(&msi, &stage).unwrap();
        let err =
            publish_pair_with(GameFamily::Bombergirl, root.path(), &SYNTHETIC, SYNTHETIC.sha256, None, &extracted, &unsigned_fixture_only)
                .unwrap_err();
        assert!(matches!(err, BootstrapError::Cache(_)), "{err}");
        assert_eq!(fs::read(&pair.libeacnet).unwrap(), good);
        assert!(matches!(inspect_fixture(root.path()), PairStatus::Present(_)));
    }

    #[cfg(any(windows, unix))]
    #[test]
    fn reparse_cache_paths_are_rejected_before_target_mutation() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let marker = outside.path().join("keep");
        fs::write(&marker, b"untouched").unwrap();
        let family = root.path().join("bombergirl");
        fs::create_dir(&family).unwrap();
        let link = family.join(STAGING_DIR);
        #[cfg(windows)]
        {
            let output =
                std::process::Command::new("cmd.exe").args(["/c", "mklink", "/J"]).arg(&link).arg(outside.path()).output().unwrap();
            assert!(output.status.success(), "junction fixture failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        assert!(matches!(stage_dir(root.path(), GameFamily::Bombergirl), Err(BootstrapError::Cache(_))));
        assert!(matches!(inspect(GameFamily::Bombergirl, root.path()), Err(BootstrapError::Cache(_))));
        assert_eq!(fs::read(&marker).unwrap(), b"untouched");
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1);
        #[cfg(windows)]
        fs::remove_dir(&link).unwrap();
        #[cfg(unix)]
        fs::remove_file(&link).unwrap();
    }
}
