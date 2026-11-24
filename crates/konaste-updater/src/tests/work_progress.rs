//! Observe the production hash path while its journal/lease are busy.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

fn observe_hash(phase: SessionPhase) {
    let directory = tempfile::tempdir().unwrap();
    let updater = Updater::open_fresh(directory.path(), DownloadPolicy::loopback_test()).unwrap();
    let body = vec![b'x'; 64 * 1024];
    let manifest = Manifest {
        version: "work-observer".into(),
        files: vec![ManifestFile {
            relative_path: "payload.bin".into(),
            size: body.len() as u64,
            sha256: hash_of(&body),
            url: Some("http://127.0.0.1:1/unused".into()),
            etag: None,
        }],
    };
    updater.prepare(manifest.clone(), None).unwrap();
    std::fs::write(directory.path().join(WORK_DIR_NAME).join("staging/payload.bin"), &body).unwrap();
    let handle = updater.handle();
    let hook_handle = handle.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let fired = AtomicBool::new(false);
        crate::pin::with_hash_hook(
            move |_| {
                let current = *hook_handle.shared.phase.lock().unwrap();
                if current == phase && !fired.swap(true, Ordering::SeqCst) {
                    entered_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(5)).expect("test must release hash");
                }
            },
            || match phase {
                SessionPhase::Preparing => updater.prepare(manifest, None).map(|_| ()),
                SessionPhase::Recovering => {
                    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
                    updater.pause();
                    assert_eq!(runtime.block_on(updater.run()).unwrap(), SessionPhase::Paused);
                    updater.resume();
                    updater.recover().map(|_| ())
                }
                _ => tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(updater.run()).map(|_| ()),
            },
        )
    });
    let reached = entered_rx.recv_timeout(Duration::from_secs(3));
    let (observed_tx, observed_rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let work = handle.work();
        let transfer = handle.try_transfer_progress();
        observed_tx.send((work, transfer)).unwrap();
    });
    let observed = observed_rx.recv_timeout(Duration::from_millis(300));
    // Release and join before asserting, including an expected-red blocked read.
    let _ = release_tx.send(());
    let ran = worker.join().expect("update worker panicked");
    reader.join().expect("observer panicked");
    assert!(reached.is_ok(), "{phase:?}: production hash gate was not reached");
    let (work, transfer) = observed.expect("work read blocked behind file hashing");
    assert_eq!(work.phase, phase);
    assert_eq!(work.current.as_deref(), Some("payload.bin"));
    assert!(work.files_total > work.files_completed, "{work:?}");
    if phase != SessionPhase::Preparing {
        assert!(transfer.is_none(), "busy journal must not produce fake zero totals");
    }
    ran.unwrap();
}

fn staged_fixture() -> (tempfile::TempDir, Manifest, u64) {
    let directory = tempfile::tempdir().unwrap();
    let updater = Updater::open_fresh(directory.path(), DownloadPolicy::loopback_test()).unwrap();
    let body = vec![b'r'; 1024 * 1024];
    let manifest = Manifest {
        version: "resume-reads".into(),
        files: (0..2)
            .map(|index| ManifestFile {
                relative_path: format!("payload-{index}.bin"),
                size: body.len() as u64,
                sha256: hash_of(&body),
                url: Some(format!("http://127.0.0.1:1/unused-{index}")),
                etag: None,
            })
            .collect(),
    };
    updater.prepare(manifest.clone(), None).unwrap();
    for file in &manifest.files {
        std::fs::write(directory.path().join(WORK_DIR_NAME).join("staging").join(&file.relative_path), &body).unwrap();
    }
    drop(updater);
    (directory, manifest, body.len() as u64 * 2)
}

fn count_hash_reads<T>(run: impl FnOnce() -> T) -> (T, u64) {
    let counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let counted = Arc::clone(&counter);
    let value = crate::pin::with_hash_hook(
        move |bytes| {
            counted.fetch_add(bytes, Ordering::Relaxed);
        },
        run,
    );
    (value, counter.load(Ordering::Relaxed))
}

#[test]
fn reopen_resume_does_not_hash_the_same_complete_stage_twice_before_run() {
    let (directory, manifest, bytes) = staged_fixture();
    let updater = Updater::open(directory.path(), DownloadPolicy::loopback_test()).unwrap();
    let now = std::time::Instant::now();
    let (prepared, prepare_reads) = count_hash_reads(|| updater.prepare(manifest, None));
    prepared.unwrap();
    let (recovery, recovery_reads) = count_hash_reads(|| updater.recover());
    assert!(recovery.unwrap().needs_run);
    println!(
        "reopen + fresh prepare + recover: prepare reads={prepare_reads}, recovery reads={recovery_reads}, staged bytes={bytes}, elapsed={:?}",
        now.elapsed()
    );
    assert_eq!(prepare_reads, bytes, "retained complete stages must still be hashed after reopen");
    assert_eq!(recovery_reads, 0, "immediate recovery repeated the just-completed stage verification");
    assert!(!directory.path().join("payload-0.bin").exists());
}

#[test]
fn a_stage_changed_after_prepare_is_still_rejected_before_any_destination_mutation() {
    let (directory, manifest, _) = staged_fixture();
    let updater = Updater::open(directory.path(), DownloadPolicy::loopback_test()).unwrap();
    updater.prepare(manifest, None).unwrap();
    std::fs::write(directory.path().join(WORK_DIR_NAME).join("staging/payload-0.bin"), vec![b'!'; 1024 * 1024]).unwrap();
    assert!(updater.recover().unwrap().needs_run);
    let mut mutations = 0;
    let error = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(updater.run_guarded(|| {
            mutations += 1;
            Ok(())
        }))
        .unwrap_err();
    assert!(matches!(error, UpdateError::Staging { .. }), "{error:?}");
    assert_eq!(mutations, 0);
    assert!(!directory.path().join("payload-0.bin").exists());
    assert!(!directory.path().join("payload-1.bin").exists());
}

#[test]
fn phase_changes_clear_stale_local_work_and_busy_samples_do_not_fabricate_totals() {
    let directory = tempfile::tempdir().unwrap();
    let updater = Updater::open_fresh(directory.path(), DownloadPolicy::loopback_test()).unwrap();
    let handle = updater.handle();
    updater.publish_work(crate::WorkSnapshot {
        phase: SessionPhase::Preparing,
        files_completed: 1,
        files_total: 2,
        bytes_completed: 123,
        bytes_total: 0,
        current: Some("old/file.bin".into()),
    });
    updater.set_phase(SessionPhase::Recovering);
    assert_eq!(handle.work(), crate::WorkSnapshot::empty(SessionPhase::Recovering));
    {
        let _journal = updater.shared.journal.lock().unwrap();
        assert!(handle.try_transfer_progress().is_none());
        assert_eq!(handle.work().current, None);
    }
    {
        let _phase = updater.shared.phase.lock().unwrap();
        assert!(handle.try_transfer_progress().is_none());
    }
    {
        let _transfers = updater.shared.transfers.lock().unwrap();
        assert!(handle.try_transfer_progress().is_none());
    }
    assert!(handle.try_transfer_progress().is_some());
}

#[test]
fn local_work_is_readable_during_prepare_verify_install_and_recovery() {
    for phase in [SessionPhase::Preparing, SessionPhase::Verifying, SessionPhase::Installing, SessionPhase::Recovering] {
        observe_hash(phase);
    }
}
