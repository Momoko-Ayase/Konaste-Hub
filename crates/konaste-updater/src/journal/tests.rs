//! Crash and version cases across the journal / install.log seam.
//!
//! Durability is the real checkpoint and log on a temp volume. These tests do
//! not reimplement record checksums; they only truncate or flip stored bytes.

use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::path::PathBuf;

use sha2::{Digest, Sha256};

use super::download::{self, DownloadLog};
use super::progress::{MAX_BYTES, NAME, ProgressLog};
use super::{FilePhase, JOURNAL_FILE, Journal, JournalPhase, WorkPaths, load_from, remove, store};
use crate::UpdateError;
use crate::manifest::{Manifest, ManifestFile, OfficialManifest};
use crate::paths::WORK_DIR_NAME;
use crate::pin::{self, Lease};
use crate::policy::DownloadPolicy;

const HEADER: usize = 40;
const RECORD: usize = 56;

fn policy() -> DownloadPolicy {
    DownloadPolicy::official(["cdn.example.test"]).unwrap()
}

fn digest() -> String {
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into()
}

fn fixture() -> (PathBuf, WorkPaths, Lease) {
    let root = tempfile::tempdir().unwrap().keep();
    let paths = WorkPaths::under(root.clone());
    let lease = Lease::open(&root, WORK_DIR_NAME).unwrap();
    (root, paths, lease)
}

fn checked(files: u64) -> crate::manifest::CheckedManifest {
    let files = (0..files)
        .map(|index| ManifestFile {
            relative_path: format!("data/file{index}.bin"),
            sha256: crate::manifest::Sha256::parse(&digest()).unwrap(),
            size: 64,
            url: Some("https://cdn.example.test/file.bin".into()),
            etag: None,
        })
        .collect();
    Manifest { version: "1.2.3".into(), files }
        .check(Some(OfficialManifest { path: "ri.bin".into(), bytes: b"official".to_vec() }))
        .unwrap()
}

fn installing(files: u64) -> Journal {
    let mut journal = Journal::from_checked(&checked(files));
    journal.phase = JournalPhase::Installing;
    for file in &mut journal.files {
        file.phase = FilePhase::Staged;
        file.received = file.size;
    }
    journal
}

fn publish(paths: &WorkPaths, lease: &mut Lease, journal: &Journal) {
    store(paths, journal, lease).unwrap();
}

fn reload(lease: &Lease) -> Journal {
    load_from(&lease.work().unwrap(), &policy()).unwrap().expect("journal")
}

fn read_named(lease: &Lease, name: &str) -> Option<Vec<u8>> {
    let work = lease.work().unwrap();
    let file = pin::open_file_read(&work, OsStr::new(name)).unwrap()?;
    Some(pin::read_exact(file.handle(), file.size()).unwrap())
}

/// Publish `bytes` as `name` through the pinned work directory.
fn write_named(lease: &mut Lease, name: &str, bytes: &[u8]) {
    let temp = format!("{name}.writing");
    let work = lease.work().unwrap();
    if !pin::child_missing(&work, OsStr::new(&temp)).unwrap() {
        pin::delete_file(&work, OsStr::new(&temp)).unwrap();
    }
    let handle = pin::create_file(&work, OsStr::new(&temp)).unwrap();
    pin::write_all(&handle, bytes).unwrap();
    pin::flush(&handle).unwrap();
    pin::rename_into(&handle, &work, OsStr::new(name), true).unwrap();
}

fn log_path(root: &std::path::Path) -> PathBuf {
    root.join(WORK_DIR_NAME).join(NAME)
}

/// The lease holds the log with read-only sharing, so byte surgery happens
/// after every handle is dropped.
fn poke_log(root: &std::path::Path, edit: impl FnOnce(&mut Vec<u8>)) {
    let path = log_path(root);
    let mut bytes = fs::read(&path).unwrap();
    edit(&mut bytes);
    fs::write(&path, bytes).unwrap();
}

#[test]
fn crash_between_records_recovers_completed_intents() {
    let (root, paths, mut lease) = fixture();
    let journal = installing(2);
    publish(&paths, &mut lease, &journal);
    let mut log = ProgressLog::open(&mut lease).unwrap();
    let mut installed = journal.files[0].clone();
    installed.phase = FilePhase::Replacing;
    installed.had_previous = true;
    log.record_file(0, &installed).unwrap();
    installed.phase = FilePhase::Installed;
    log.record_file(0, &installed).unwrap();
    let mut replacing = journal.files[1].clone();
    replacing.phase = FilePhase::Replacing;
    replacing.had_previous = false;
    log.record_file(1, &replacing).unwrap();
    drop(log);
    drop(lease);

    let lease = Lease::open(&root, WORK_DIR_NAME).unwrap();
    let loaded = reload(&lease);
    assert_eq!(loaded.phase, JournalPhase::Installing);
    assert_eq!(loaded.files[0].phase, FilePhase::Installed);
    assert!(loaded.files[0].had_previous);
    assert_eq!(loaded.files[0].received, loaded.files[0].size);
    assert_eq!(loaded.files[1].phase, FilePhase::Replacing);
    assert!(!loaded.files[1].had_previous);
    assert_eq!(loaded.files[1].received, loaded.files[1].size);
}

#[test]
fn torn_final_record_keeps_only_complete_prefix() {
    let prefixes = [0_usize, 1, 7, RECORD / 2, RECORD - 1];
    for extra in prefixes {
        let (root, paths, mut lease) = fixture();
        let journal = installing(1);
        publish(&paths, &mut lease, &journal);
        let mut log = ProgressLog::open(&mut lease).unwrap();
        let mut first = journal.files[0].clone();
        first.phase = FilePhase::Replacing;
        first.had_previous = true;
        log.record_file(0, &first).unwrap();
        first.phase = FilePhase::Installed;
        first.had_previous = false;
        log.record_file(0, &first).unwrap();
        drop(log);
        drop(lease);

        poke_log(&root, |bytes| bytes.truncate(HEADER + RECORD + extra));
        let lease = Lease::open(&root, WORK_DIR_NAME).unwrap();
        let loaded = reload(&lease);
        assert_eq!(loaded.files[0].phase, FilePhase::Replacing, "prefix length {extra}");
        assert!(loaded.files[0].had_previous, "prefix length {extra}");
    }
}

#[test]
fn corrupt_complete_record_is_refused() {
    let (root, paths, mut lease) = fixture();
    let journal = installing(1);
    publish(&paths, &mut lease, &journal);
    let mut log = ProgressLog::open(&mut lease).unwrap();
    let mut file = journal.files[0].clone();
    file.phase = FilePhase::Installed;
    log.record_file(0, &file).unwrap();
    drop(log);
    drop(lease);

    poke_log(&root, |bytes| {
        let bit = bytes.len() - 1;
        bytes[bit] ^= 0x01;
    });
    let lease = Lease::open(&root, WORK_DIR_NAME).unwrap();
    let err = load_from(&lease.work().unwrap(), &policy()).unwrap_err();
    assert!(matches!(err, UpdateError::Journal(_)), "{err}");
}

#[test]
fn record_rejects_bad_index_and_received_before_any_write() {
    let (_root, paths, mut lease) = fixture();
    let journal = installing(1);
    publish(&paths, &mut lease, &journal);
    let mut log = ProgressLog::open(&mut lease).unwrap();
    let missing = log.record_file(1, &journal.files[0]);
    assert!(matches!(missing, Err(UpdateError::Journal(_))), "{missing:?}");

    let mut over = journal.files[0].clone();
    over.received = over.size + 1;
    let oversized = log.record_file(0, &over);
    assert!(matches!(oversized, Err(UpdateError::Journal(_))), "{oversized:?}");

    let mut resized = journal.files[0].clone();
    resized.size = resized.size.saturating_sub(1);
    resized.received = resized.size;
    let wrong_size = log.record_file(0, &resized);
    assert!(matches!(wrong_size, Err(UpdateError::Journal(_))), "{wrong_size:?}");
    drop(log);

    let after = read_named(&lease, NAME).unwrap();
    assert_eq!(after.len(), HEADER);

    let loaded = reload(&lease);
    assert_eq!(loaded.files[0].phase, FilePhase::Staged);
    assert_eq!(loaded.files[0].received, loaded.files[0].size);
}

#[test]
fn oversized_progress_log_is_refused() {
    let (root, paths, mut lease) = fixture();
    publish(&paths, &mut lease, &installing(1));
    drop(ProgressLog::open(&mut lease).unwrap());
    drop(lease);

    let log = OpenOptions::new().write(true).open(log_path(&root)).unwrap();
    log.set_len(MAX_BYTES + 1).unwrap();
    drop(log);

    let lease = Lease::open(&root, WORK_DIR_NAME).unwrap();
    let err = load_from(&lease.work().unwrap(), &policy()).unwrap_err();
    assert!(matches!(err, UpdateError::Journal(_)), "{err}");
}

#[test]
fn new_checkpoint_retires_log_and_restored_old_log_is_ignored() {
    let (_root, paths, mut lease) = fixture();
    let journal = installing(1);
    publish(&paths, &mut lease, &journal);
    let base = read_named(&lease, super::JOURNAL_FILE).unwrap();
    let mut log = ProgressLog::open(&mut lease).unwrap();
    let mut file = journal.files[0].clone();
    file.phase = FilePhase::Replacing;
    file.had_previous = true;
    log.record_file(0, &file).unwrap();
    drop(log);
    let old_log = read_named(&lease, NAME).unwrap();
    assert_eq!(&old_log[8..HEADER], Sha256::digest(&base).as_slice());

    // Distinct from the retired record, so a wrongly replayed log cannot pass.
    let mut next = journal.clone();
    next.phase = JournalPhase::Committed;
    next.files[0].phase = FilePhase::Installed;
    next.files[0].had_previous = false;
    next.files[0].received = next.files[0].size;
    publish(&paths, &mut lease, &next);
    assert!(read_named(&lease, NAME).is_none());

    write_named(&mut lease, NAME, &old_log);
    let loaded = reload(&lease);
    assert_eq!(loaded.phase, JournalPhase::Committed);
    assert_eq!(loaded.files[0].phase, FilePhase::Installed);
    assert!(!loaded.files[0].had_previous);
    assert_eq!(loaded.files[0].received, loaded.files[0].size);
}

#[test]
fn equivalent_full_checkpoint_does_not_need_the_log() {
    let (_root, paths, mut lease) = fixture();
    let mut journal = installing(2);
    journal.phase = JournalPhase::Committed;
    journal.files[0].phase = FilePhase::Installed;
    journal.files[0].had_previous = true;
    journal.files[0].received = journal.files[0].size;
    journal.files[1].phase = FilePhase::Replacing;
    journal.files[1].had_previous = true;
    journal.files[1].received = 0;
    publish(&paths, &mut lease, &journal);

    let loaded = reload(&lease);
    assert_eq!(loaded.version, 3);
    assert_eq!(loaded.manifest_version, journal.manifest_version);
    assert_eq!(loaded.phase, JournalPhase::Committed);
    assert_eq!(loaded.files[0].phase, FilePhase::Installed);
    assert!(loaded.files[0].had_previous);
    assert_eq!(loaded.files[0].received, loaded.files[0].size);
    assert_eq!(loaded.files[1].phase, FilePhase::Replacing);
    assert!(loaded.files[1].had_previous);
    assert_eq!(loaded.files[1].received, 0);
    assert!(loaded.files.iter().zip(&journal.files).all(|(got, want)| {
        got.relative == want.relative
            && got.sha256 == want.sha256
            && got.size == want.size
            && got.url == want.url
            && got.etag == want.etag
            && got.phase == want.phase
            && got.received == want.received
            && got.had_previous == want.had_previous
    }));
    let official = loaded.official.as_ref().unwrap();
    let expected = journal.official.as_ref().unwrap();
    assert_eq!(official.relative, expected.relative);
    assert_eq!(official.sha256, expected.sha256);
    assert_eq!(official.size, expected.size);
    assert_eq!(official.phase, expected.phase);
    assert_eq!(official.had_previous, expected.had_previous);
    assert!(read_named(&lease, NAME).is_none());
}

#[test]
fn remove_clears_checkpoint_and_log() {
    let (_root, paths, mut lease) = fixture();
    let journal = installing(1);
    publish(&paths, &mut lease, &journal);
    let mut log = ProgressLog::open(&mut lease).unwrap();
    log.record_file(0, &journal.files[0]).unwrap();
    drop(log);
    assert!(read_named(&lease, NAME).is_some());
    assert!(read_named(&lease, super::JOURNAL_FILE).is_some());

    remove(&paths, &lease).unwrap();
    assert!(read_named(&lease, NAME).is_none());
    assert!(read_named(&lease, super::JOURNAL_FILE).is_none());
    assert!(load_from(&lease.work().unwrap(), &policy()).unwrap().is_none());
}

#[test]
fn hardlinked_progress_log_is_refused_without_an_outside_write() {
    let (root, paths, mut lease) = fixture();
    publish(&paths, &mut lease, &installing(1));
    drop(ProgressLog::open(&mut lease).unwrap());
    drop(lease);
    let path = log_path(&root);
    let alias = root.join(WORK_DIR_NAME).join("install.log.alias");
    fs::hard_link(&path, &alias).unwrap();

    let lease = Lease::open(&root, WORK_DIR_NAME).unwrap();
    let err = load_from(&lease.work().unwrap(), &policy()).unwrap_err();
    assert!(matches!(err, UpdateError::PathRejected { .. }), "{err}");
    let still = fs::read(&path).unwrap();
    assert_eq!(still.len(), HEADER);
    assert_eq!(&still[..8], b"KHUPWAL1");
}

fn downloading(files: u64) -> Journal {
    let mut journal = Journal::from_checked(&checked(files));
    journal.phase = JournalPhase::Downloading;
    journal
}

fn download_path(root: &std::path::Path) -> PathBuf {
    root.join(WORK_DIR_NAME).join(download::NAME)
}

fn poke_download(root: &std::path::Path, edit: impl FnOnce(&mut Vec<u8>)) {
    let path = download_path(root);
    let mut bytes = fs::read(&path).unwrap();
    edit(&mut bytes);
    fs::write(&path, bytes).unwrap();
}

#[test]
fn only_the_current_journal_version_loads() {
    for version in [1, 2, 4] {
        let (_root, _paths, mut lease) = fixture();
        let mut journal = downloading(1);
        journal.version = version;
        write_named(&mut lease, JOURNAL_FILE, &serde_json::to_vec_pretty(&journal).unwrap());
        let err = load_from(&lease.work().unwrap(), &policy()).unwrap_err();
        assert!(matches!(err, UpdateError::Journal(_)), "version {version}: {err}");
    }
}

#[test]
fn download_records_keep_etag_before_received_and_ignore_a_torn_tail() {
    let (root, paths, mut lease) = fixture();
    let journal = downloading(2);
    publish(&paths, &mut lease, &journal);
    let mut log = DownloadLog::open(&mut lease).unwrap();
    let mut first = journal.files[0].clone();
    first.phase = FilePhase::Partial;
    log.record_etag(0, "\"source-v1\"", &first).unwrap();
    first.received = first.size;
    first.phase = FilePhase::Staged;
    log.record_received(0, &first).unwrap();
    let mut second = journal.files[1].clone();
    second.phase = FilePhase::Partial;
    log.record_etag(1, "\"source-v2\"", &second).unwrap();
    drop(log);
    drop(lease);

    poke_download(&root, |bytes| bytes.extend_from_slice(&[0xab; 17]));
    let lease = Lease::open(&root, WORK_DIR_NAME).unwrap();
    let loaded = reload(&lease);
    assert_eq!(loaded.files[0].etag.as_deref(), Some("\"source-v1\""));
    assert_eq!(loaded.files[0].phase, FilePhase::Staged);
    assert_eq!(loaded.files[0].received, loaded.files[0].size);
    assert_eq!(loaded.files[1].etag.as_deref(), Some("\"source-v2\""));
    assert_eq!(loaded.files[1].phase, FilePhase::Partial);
    assert_eq!(loaded.files[1].received, 0);
    assert_eq!(loaded.files[0].url, journal.files[0].url);
    assert_eq!(loaded.files[0].sha256, journal.files[0].sha256);
}

#[test]
fn corrupt_download_record_is_refused() {
    let (root, paths, mut lease) = fixture();
    publish(&paths, &mut lease, &downloading(1));
    let journal = downloading(1);
    let mut log = DownloadLog::open(&mut lease).unwrap();
    let mut file = journal.files[0].clone();
    file.phase = FilePhase::Partial;
    log.record_etag(0, "\"v1\"", &file).unwrap();
    drop(log);
    drop(lease);

    poke_download(&root, |bytes| {
        let bit = bytes.len() - 1;
        bytes[bit] ^= 0x01;
    });
    let lease = Lease::open(&root, WORK_DIR_NAME).unwrap();
    let err = load_from(&lease.work().unwrap(), &policy()).unwrap_err();
    assert!(matches!(err, UpdateError::Journal(_)), "{err}");
}

#[test]
fn download_record_rejects_bad_index_etag_and_received_before_any_write() {
    let (_root, paths, mut lease) = fixture();
    let journal = downloading(1);
    publish(&paths, &mut lease, &journal);
    let mut log = DownloadLog::open(&mut lease).unwrap();
    let missing = log.record_etag(1, "\"v1\"", &journal.files[0]);
    assert!(matches!(missing, Err(UpdateError::Journal(_))), "{missing:?}");

    let controls = log.record_etag(0, "\"bad\netag\"", &journal.files[0]);
    assert!(matches!(controls, Err(UpdateError::Journal(_))), "{controls:?}");

    let mut over = journal.files[0].clone();
    over.received = over.size + 1;
    let oversized = log.record_received(0, &over);
    assert!(matches!(oversized, Err(UpdateError::Journal(_))), "{oversized:?}");
    drop(log);

    let after = read_named(&lease, download::NAME).unwrap();
    assert_eq!(after.len(), 40);
    assert_eq!(reload(&lease).files[0].etag, None);
}

#[test]
fn new_checkpoint_retires_download_log_and_old_log_is_ignored() {
    let (_root, paths, mut lease) = fixture();
    let journal = downloading(1);
    publish(&paths, &mut lease, &journal);
    let mut log = DownloadLog::open(&mut lease).unwrap();
    let mut file = journal.files[0].clone();
    file.phase = FilePhase::Partial;
    log.record_etag(0, "\"old\"", &file).unwrap();
    drop(log);
    let old_log = read_named(&lease, download::NAME).unwrap();

    let mut next = journal.clone();
    next.files[0].etag = Some("\"checkpoint\"".into());
    next.files[0].phase = FilePhase::Staged;
    next.files[0].received = next.files[0].size;
    publish(&paths, &mut lease, &next);
    assert!(read_named(&lease, download::NAME).is_none());

    write_named(&mut lease, download::NAME, &old_log);
    let loaded = reload(&lease);
    assert_eq!(loaded.files[0].etag.as_deref(), Some("\"checkpoint\""));
    assert_eq!(loaded.files[0].phase, FilePhase::Staged);
    assert_eq!(loaded.files[0].received, loaded.files[0].size);
}

#[test]
fn download_etag_above_the_cap_is_refused_before_any_write() {
    let (_root, paths, mut lease) = fixture();
    let journal = downloading(1);
    publish(&paths, &mut lease, &journal);
    let mut log = DownloadLog::open(&mut lease).unwrap();
    let huge = format!("\"{}\"", "e".repeat(512));
    let refused = log.record_etag(0, &huge, &journal.files[0]);
    assert!(matches!(refused, Err(UpdateError::Journal(_))), "{refused:?}");
    drop(log);
    assert_eq!(read_named(&lease, download::NAME).unwrap().len(), 40);
    assert_eq!(reload(&lease).files[0].etag, None);
}

#[test]
fn download_log_above_its_byte_cap_is_refused() {
    let (root, paths, mut lease) = fixture();
    publish(&paths, &mut lease, &downloading(1));
    drop(DownloadLog::open(&mut lease).unwrap());
    drop(lease);

    let log = OpenOptions::new().write(true).open(download_path(&root)).unwrap();
    log.set_len(download::MAX_BYTES + 1).unwrap();
    drop(log);

    let lease = Lease::open(&root, WORK_DIR_NAME).unwrap();
    let err = load_from(&lease.work().unwrap(), &policy()).unwrap_err();
    assert!(matches!(err, UpdateError::Journal(_)), "{err}");
}

#[test]
fn hardlinked_download_log_is_refused_without_an_outside_write() {
    let (root, paths, mut lease) = fixture();
    publish(&paths, &mut lease, &downloading(1));
    drop(DownloadLog::open(&mut lease).unwrap());
    drop(lease);
    let path = download_path(&root);
    fs::hard_link(&path, root.join(WORK_DIR_NAME).join("download.log.alias")).unwrap();

    let lease = Lease::open(&root, WORK_DIR_NAME).unwrap();
    let err = load_from(&lease.work().unwrap(), &policy()).unwrap_err();
    assert!(matches!(err, UpdateError::PathRejected { .. }), "{err}");
    let still = fs::read(&path).unwrap();
    assert_eq!(&still[..8], b"KHUDWAL1");
}
