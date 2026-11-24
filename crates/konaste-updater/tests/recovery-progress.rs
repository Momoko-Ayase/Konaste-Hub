//! Repeated interruption through the real public updater interface. Each
//! recovery checkpoints the completed prefix and starts a bounded new log.
use konaste_updater::{
    DownloadPolicy, Manifest, ManifestFile, OfficialManifest, SessionPhase, Sha256, UpdateError, Updater, WORK_DIR_NAME,
};
use sha2::{Digest, Sha256 as Hasher};

const FILES: usize = 24;

#[tokio::test]
async fn repeated_recovery_does_not_replay_or_accumulate_old_log_generations() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let entries: Vec<_> = (0..FILES)
        .map(|index| {
            let body = format!("synthetic-{index}").into_bytes();
            let path = format!("data/{index:02}.bin");
            let file = ManifestFile {
                relative_path: path.clone(),
                size: body.len() as u64,
                sha256: Sha256::parse(&hex::encode(Hasher::digest(&body))).unwrap(),
                url: Some(format!("http://127.0.0.1:1/unused/{index}")),
                etag: None,
            };
            (file, path, body)
        })
        .collect();
    let manifest = Manifest { version: "interrupted-progress".into(), files: entries.iter().map(|(file, _, _)| file.clone()).collect() };
    let official = OfficialManifest { path: "ri.bin".into(), bytes: b"exact synthetic index".to_vec() };
    let updater = Updater::open_fresh(root, DownloadPolicy::loopback_test()).unwrap();
    for (_, path, body) in &entries {
        let path = root.join(WORK_DIR_NAME).join("staging").join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    updater.prepare(manifest.clone(), Some(official.clone())).unwrap();
    let result = updater
        .run_guarded(|| {
            if root.join("data/00.bin").is_file() {
                return Err(UpdateError::operation_rejected("synthetic interruption"));
            }
            Ok(())
        })
        .await;
    assert!(matches!(result, Err(UpdateError::OperationRejected(_))));
    drop(updater);
    for target in 2..=12 {
        let updater = Updater::open(root, DownloadPolicy::loopback_test()).unwrap();
        updater.prepare(manifest.clone(), Some(official.clone())).unwrap();
        let result = updater.recover_guarded(|| {
            let completed = entries.iter().filter(|(_, path, _)| root.join(path).is_file()).count();
            if completed >= target {
                return Err(UpdateError::operation_rejected("synthetic interruption"));
            }
            Ok(())
        });
        assert!(matches!(result, Err(UpdateError::OperationRejected(_))));
        drop(updater);
        assert_eq!(entries.iter().filter(|(_, path, _)| root.join(path).is_file()).count(), target);
        assert!(
            std::fs::metadata(root.join(WORK_DIR_NAME).join("install.log")).unwrap().len() <= 512,
            "each recovery must retire the previous log, not accumulate it"
        );
        assert!(!root.join("ri.bin").exists(), "official index cannot precede the remaining content");
        for (_, path, body) in entries.iter().take(target) {
            assert_eq!(std::fs::read(root.join(path)).unwrap(), *body);
        }
    }
    let updater = Updater::open(root, DownloadPolicy::loopback_test()).unwrap();
    updater.prepare(manifest, Some(official.clone())).unwrap();
    assert!(!updater.recover().unwrap().needs_run);
    assert_eq!(updater.snapshot().phase, SessionPhase::Idle);
    for (_, path, body) in entries {
        assert_eq!(std::fs::read(root.join(path)).unwrap(), body);
    }
    assert_eq!(std::fs::read(root.join("ri.bin")).unwrap(), official.bytes);
    assert!(!root.join(WORK_DIR_NAME).join("install.log").exists());
    assert!(!root.join(WORK_DIR_NAME).join("journal.json").exists());
}
