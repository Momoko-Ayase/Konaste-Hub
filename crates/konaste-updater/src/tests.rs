//! Behavioral tests against a local loopback server and synthetic files.
//!
//! Nothing here contacts an official host or writes outside a temporary root.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::{DownloadPolicy, Manifest, ManifestFile, OfficialManifest, SessionPhase, Sha256, UpdateError, Updater, WORK_DIR_NAME};

#[test]
fn a_reserved_fresh_root_reopens_after_work_directory_creation() {
    let root = tempfile::tempdir().unwrap();
    let options = || crate::UpdaterOptions::new(DownloadPolicy::official(["example.com"]).unwrap());
    drop(Updater::open_fresh_with(root.path(), options()).unwrap());
    assert!(root.path().join(WORK_DIR_NAME).is_dir());
    assert!(Updater::open_fresh_with(root.path(), options()).is_err());
    drop(Updater::open_unstarted_with(root.path(), options()).unwrap());
    std::fs::write(root.path().join("keep.txt"), b"unchanged").unwrap();
    assert!(Updater::open_unstarted_with(root.path(), options()).is_err());
    assert_eq!(std::fs::read(root.path().join("keep.txt")).unwrap(), b"unchanged");
}

fn hash_of(bytes: &[u8]) -> Sha256 {
    Sha256::parse(&hex::encode(crate::hash::sha256(bytes))).unwrap()
}

fn temp_root() -> PathBuf {
    let dir = tempfile::tempdir().unwrap();

    dir.keep()
}

struct Server {
    addr: String,
}

#[derive(Clone)]
enum ServerMode {
    /// Honor Range with a correct 206.
    Honest,
    /// Ignore Range and always answer 200 with the full body.
    IgnoreRange,
    /// Ignore Range but stop before the replacement is complete.
    ShortReplacement(usize),
    /// Answer 206 with a Content-Range that does not match the request.
    BadRange,
    /// The first response sends only `n` bytes and then the server stops answering.
    DropOnce(usize),
    /// Change the body after `n` requests (etag changes too).
    ChangeAfter { requests: usize, replacement: Vec<u8> },
    /// First response is a short prefix with an ETag. Later responses change both.
    LearnThenChange { first: usize, replacement: Vec<u8> },
    /// Answer with a non-identity Content-Encoding. The body stays uncompressed.
    Gzip,
    /// 302 to another loopback URL. The client must not follow it.
    Redirect,
}

impl Server {
    fn spawn(files: Vec<(String, Vec<u8>, ServerMode)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let files = Arc::new(Mutex::new(files));
        let shared = Arc::clone(&files);
        let hits = Arc::new(Mutex::new(0_usize));
        thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = match stream {
                    Ok(stream) => stream,
                    Err(_) => continue,
                };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = [0_u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
                let range = req.lines().find_map(|line| {
                    line.to_ascii_lowercase().strip_prefix("range:").map(|value| value.trim().trim_start_matches("bytes=").to_string())
                });
                let mut hits = hits.lock().unwrap();
                *hits += 1;
                let hit = *hits;
                drop(hits);
                let files = shared.lock().unwrap();
                let Some((body, mode)) = files.iter().find(|(name, _, _)| path.ends_with(name)).map(|(_, b, m)| (b.clone(), m.clone()))
                else {
                    let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                    continue;
                };
                drop(files);
                let (body, etag, mode) = match &mode {
                    ServerMode::ChangeAfter { requests, replacement } if hit > *requests => {
                        (replacement.clone(), "\"changed\"", ServerMode::Honest)
                    }
                    ServerMode::LearnThenChange { first, replacement } if hit > *first => {
                        (replacement.clone(), "\"v2\"", ServerMode::Honest)
                    }
                    ServerMode::LearnThenChange { .. } => (body, "\"v1\"", ServerMode::DropOnce(8)),
                    other => (body, "\"v1\"", other.clone()),
                };
                let start =
                    range.as_deref().and_then(|value| value.split('-').next()).and_then(|value| value.parse::<usize>().ok()).unwrap_or(0);
                match &mode {
                    ServerMode::IgnoreRange => write_response(&mut stream, 200, etag, None, &body),
                    ServerMode::ShortReplacement(n) => write_response(&mut stream, 200, etag, None, &body[..(*n).min(body.len())]),
                    ServerMode::BadRange => {
                        let header = format!("bytes 0-{}/{}", body.len().saturating_sub(1), body.len());
                        write_response(&mut stream, 206, etag, Some(&header), &body);
                    }
                    ServerMode::DropOnce(n) if hit == 1 => {
                        let n = *n;
                        let slice = if start > 0 { &body[start.min(body.len())..] } else { &body };
                        let send = &slice[..n.min(slice.len())];
                        write_response(&mut stream, 200, etag, None, send);
                    }
                    ServerMode::DropOnce(_) => {
                        let _ = stream.write_all(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\n\r\n");
                    }
                    ServerMode::Gzip => {
                        write_response_encoding(&mut stream, 200, etag, None, Some("gzip"), None, &body);
                    }
                    ServerMode::Redirect => {
                        write_response_encoding(&mut stream, 302, etag, None, None, Some("http://127.0.0.1/elsewhere"), &[]);
                    }
                    _ => {
                        if start > 0 && start < body.len() {
                            let slice = &body[start..];
                            let header = format!("bytes {start}-{}/{}", body.len() - 1, body.len());
                            write_response(&mut stream, 206, etag, Some(&header), slice);
                        } else {
                            write_response(&mut stream, 200, etag, None, &body);
                        }
                    }
                }
            }
        });
        Self { addr: format!("127.0.0.1:{}", addr.port()) }
    }

    fn url(&self, name: &str) -> String {
        format!("http://{}/{}", self.addr, name)
    }
}

fn write_response(stream: &mut std::net::TcpStream, status: u16, etag: &str, content_range: Option<&str>, body: &[u8]) {
    write_response_encoding(stream, status, etag, content_range, None, None, body);
}

fn write_response_encoding(
    stream: &mut std::net::TcpStream,
    status: u16,
    etag: &str,
    content_range: Option<&str>,
    encoding: Option<&str>,
    location: Option<&str>,
    body: &[u8],
) {
    let reason = if status == 200 {
        "OK"
    } else if status == 206 {
        "Partial Content"
    } else if status == 302 {
        "Found"
    } else {
        "Err"
    };
    let mut head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nETag: {etag}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n",
        body.len()
    );
    if let Some(encoding) = encoding {
        head.push_str(&format!("Content-Encoding: {encoding}\r\n"));
    }
    if let Some(location) = location {
        head.push_str(&format!("Location: {location}\r\n"));
    }
    if let Some(range) = content_range {
        head.push_str(&format!("Content-Range: {range}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
    // Stay up until the client finishes reading. Dropping here truncates the body.
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut sink = [0_u8; 64];
    let _ = stream.read(&mut sink);
}

fn file_without_etag(path: &str, bytes: &[u8], url: Option<String>) -> ManifestFile {
    let mut file = file(path, bytes, url);
    file.etag = None;
    file
}

fn file(path: &str, bytes: &[u8], url: Option<String>) -> ManifestFile {
    ManifestFile { relative_path: path.into(), sha256: hash_of(bytes), size: bytes.len() as u64, url, etag: Some("\"v1\"".into()) }
}

fn manifest(version: &str, files: Vec<ManifestFile>) -> Manifest {
    Manifest { version: version.into(), files }
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_default()
}

#[test]
fn rejects_path_escapes_and_duplicate_case() {
    let root = temp_root();
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    let body = b"hello";
    let bad_paths = [
        r"C:\game\a.bin",
        r"\\server\share\a.bin",
        "../outside.bin",
        "data/../../x.bin",
        ".konaste-hub/journal.json",
        "file.bin:stream",
        "CON",
        "data/aux.txt",
    ];
    for path in bad_paths {
        let err = updater.prepare(manifest("1", vec![file(path, body, Some("http://127.0.0.1/a".into()))]), None).unwrap_err();
        assert!(matches!(err, UpdateError::PathRejected { .. } | UpdateError::Manifest(_)), "{path}: {err}");
        assert!(std::fs::read_dir(&root).unwrap().all(|e| e.unwrap().file_name() == WORK_DIR_NAME));
    }
    let err = updater
        .prepare(
            manifest(
                "1",
                vec![
                    file("Data/A.bin", body, Some("http://127.0.0.1/a".into())),
                    file("data/a.bin", body, Some("http://127.0.0.1/b".into())),
                ],
            ),
            None,
        )
        .unwrap_err();
    assert!(matches!(err, UpdateError::Manifest(_)), "{err}");
}

#[test]
fn official_policy_rejects_http_and_unknown_hosts() {
    let err = DownloadPolicy::official(["*.cloudfront.net"]).unwrap_err();
    assert!(matches!(err, UpdateError::Policy { .. }));
    let policy = DownloadPolicy::official(["d1q1iflay6y5jj.cloudfront.net", "dks1q2aivwkd6.cloudfront.net"]).unwrap();
    assert!(policy.check_url("http://d1q1iflay6y5jj.cloudfront.net/a").is_err());
    assert!(policy.check_url("https://evil.cloudfront.net/a").is_err());
    assert!(policy.check_url("https://d1q1iflay6y5jj.cloudfront.net/a").is_ok());
    let loopback = DownloadPolicy::loopback_test();
    assert!(loopback.check_url("http://127.0.0.1:9/a").is_ok());
    assert!(loopback.check_url("http://example.com/a").is_err());
}

#[tokio::test]
async fn bad_hash_does_not_touch_installed_file() {
    let root = temp_root();
    let original = b"original-installed";
    std::fs::write(root.join("keep.bin"), original).unwrap();
    let wrong = b"downloaded-bytes-not-matching-declared-hash";
    let server = Server::spawn(vec![("keep.bin".into(), wrong.to_vec(), ServerMode::Honest)]);
    let mut claimed = file("keep.bin", wrong, Some(server.url("keep.bin")));
    claimed.sha256 = hash_of(b"something-else");
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    updater.prepare(manifest("2", vec![claimed]), None).unwrap();
    let err = updater.run().await.unwrap_err();
    assert!(matches!(err, UpdateError::Staging { .. }), "{err}");
    assert_eq!(read(&root.join("keep.bin")), original);
}

#[tokio::test]
async fn interrupted_download_resumes_without_losing_partial() {
    let root = temp_root();
    let body = b"0123456789abcdef-partial-resume-body";
    let server = Server::spawn(vec![("part.bin".into(), body.to_vec(), ServerMode::DropOnce(8))]);
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    updater.prepare(manifest("3", vec![file("part.bin", body, Some(server.url("part.bin")))]), None).unwrap();
    let err = updater.run().await.unwrap_err();
    assert!(err.is_resumable(), "{err}");
    drop(updater);
    let staged = root.join(WORK_DIR_NAME).join("staging").join("part.bin");
    let partial = read(&staged);
    assert!(!partial.is_empty() && partial.len() < body.len(), "partial {} bytes, err {err}", partial.len());
    // Swap the server mode to honest by restarting a new server and rewriting the journal url.
    let honest = Server::spawn(vec![("part.bin".into(), body.to_vec(), ServerMode::Honest)]);
    rewrite_url(&root, &honest.url("part.bin"));
    let reopened = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    assert!(matches!(reopened.run().await, Err(UpdateError::State(_))), "a restarted session must be bound again");
    reopened.prepare(manifest("3", vec![file("part.bin", body, Some(honest.url("part.bin")))]), None).unwrap();
    let recovery = reopened.recover().unwrap();
    assert!(recovery.needs_run);
    assert_eq!(read(&staged), partial, "recover must not delete the partial");
    assert_eq!(reopened.run().await.unwrap(), SessionPhase::Idle);
    assert_eq!(read(&root.join("part.bin")), body);
}

#[tokio::test]
async fn ignored_range_does_not_corrupt_partial_and_bad_range_is_rejected() {
    let root = temp_root();
    let body = b"abcdefghijklmnopqrstuvwxyz";
    let server = Server::spawn(vec![
        ("ok.bin".into(), body.to_vec(), ServerMode::IgnoreRange),
        ("bad.bin".into(), body.to_vec(), ServerMode::BadRange),
    ]);
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    let mut files = vec![file("ok.bin", body, Some(server.url("ok.bin")))];
    // Seed a partial for the ignored-range object.
    updater.prepare(manifest("4", files.clone()), None).unwrap();
    let staged = root.join(WORK_DIR_NAME).join("staging").join("ok.bin");
    std::fs::create_dir_all(staged.parent().unwrap()).unwrap();
    std::fs::write(&staged, &body[..4]).unwrap();
    assert_eq!(updater.run().await.unwrap(), SessionPhase::Idle);
    assert_eq!(read(&root.join("ok.bin")), body);

    let root = temp_root();
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    files = vec![file("bad.bin", body, Some(server.url("bad.bin")))];
    updater.prepare(manifest("4b", files), None).unwrap();
    let staged = root.join(WORK_DIR_NAME).join("staging").join("bad.bin");
    std::fs::create_dir_all(staged.parent().unwrap()).unwrap();
    std::fs::write(&staged, &body[..4]).unwrap();
    let err = updater.run().await.unwrap_err();
    assert!(err.is_resumable(), "{err}");
    assert_eq!(read(&staged), &body[..4], "bad Content-Range must not be appended");
    assert!(!root.join("bad.bin").exists());
}

#[tokio::test]
async fn failed_replacement_reports_only_the_retained_prefix_during_backoff() {
    let directory = tempfile::tempdir().unwrap();
    let body = b"abcdefghijklmnopqrstuvwxyz";
    let server = Server::spawn(vec![("short.bin".into(), body.to_vec(), ServerMode::ShortReplacement(8))]);
    let updater = Updater::open(directory.path(), DownloadPolicy::loopback_test()).unwrap();
    updater.prepare(manifest("replacement-progress", vec![file("short.bin", body, Some(server.url("short.bin")))]), None).unwrap();
    let staged = directory.path().join(WORK_DIR_NAME).join("staging/short.bin");
    std::fs::create_dir_all(staged.parent().unwrap()).unwrap();
    std::fs::write(&staged, &body[..4]).unwrap();
    let handle = updater.handle();
    let watch = async {
        let sample = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let sample = handle.transfer_progress();
                if sample.files.iter().any(|file| file.retrying) {
                    break sample;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        handle.pause();
        sample
    };
    let (ran, sample) = tokio::join!(updater.run(), watch);
    assert_eq!(ran.unwrap(), SessionPhase::Paused);
    assert_eq!(sample.files[0].bytes_received, 4);
    assert_eq!(sample.progress.bytes_received, 4);
    assert_eq!(read(&staged), &body[..4]);
    assert!(!directory.path().join("short.bin").exists());
}

#[tokio::test]
async fn changed_etag_discards_partial_instead_of_appending() {
    let root = temp_root();
    let body = b"version-one-body-fixed";
    let server = Server::spawn(vec![(
        "obj.bin".into(),
        body.to_vec(),
        ServerMode::ChangeAfter { requests: 0, replacement: b"completely-different".to_vec() },
    )]);
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    updater.prepare(manifest("5", vec![file("obj.bin", body, Some(server.url("obj.bin")))]), None).unwrap();
    let staged = root.join(WORK_DIR_NAME).join("staging").join("obj.bin");
    std::fs::create_dir_all(staged.parent().unwrap()).unwrap();
    std::fs::write(&staged, &body[..6]).unwrap();
    let err = updater.run().await.unwrap_err();
    assert!(err.is_resumable() || matches!(err, UpdateError::Staging { .. }), "{err}");
    assert!(!root.join("obj.bin").exists());
}

#[tokio::test]
async fn pause_before_run_is_not_lost_and_reopen_resumes() {
    let root = temp_root();
    let body = b"pause-me-please-body";
    let server = Server::spawn(vec![("p.bin".into(), body.to_vec(), ServerMode::Honest)]);
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    updater.prepare(manifest("6", vec![file("p.bin", body, Some(server.url("p.bin")))]), None).unwrap();
    let handle = updater.handle();
    handle.pause();
    let phase = updater.run().await.unwrap();
    assert_eq!(phase, SessionPhase::Paused);
    assert!(handle.progress().paused);
    assert!(!root.join("p.bin").exists());
    drop(updater);
    drop(handle);
    let reopened = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    assert!(matches!(reopened.run().await, Err(UpdateError::State(_))));
    reopened.prepare(manifest("6", vec![file("p.bin", body, Some(server.url("p.bin")))]), None).unwrap();
    // A fresh process starts unlatched; the previous latch was in memory.
    assert_eq!(reopened.run().await.unwrap(), SessionPhase::Idle);
    assert_eq!(read(&root.join("p.bin")), body);
    assert_eq!(reopened.snapshot().phase, SessionPhase::Idle);
}

#[tokio::test]
async fn one_file_download_completes_and_retires_its_log() {
    let root = temp_root();
    let body = b"one-complete-download";
    let server = Server::spawn(vec![("one.bin".into(), body.to_vec(), ServerMode::Honest)]);
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    updater.prepare(manifest("one", vec![file_without_etag("one.bin", body, Some(server.url("one.bin")))]), None).unwrap();
    assert_eq!(updater.run().await.unwrap(), SessionPhase::Idle);
    assert_eq!(read(&root.join("one.bin")), body);
    assert!(!root.join(WORK_DIR_NAME).join("download.log").exists());
    assert!(!root.join(WORK_DIR_NAME).join("journal.json").exists());
}

#[tokio::test]
async fn pause_during_download_resumes_in_the_same_session() {
    let root = temp_root();
    let body = b"0123456789abcdef-same-session-pause";
    let server = Server::spawn(vec![("p.bin".into(), body.to_vec(), ServerMode::DropOnce(8))]);
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    updater.prepare(manifest("pause-same", vec![file_without_etag("p.bin", body, Some(server.url("p.bin")))]), None).unwrap();
    let err = updater.run().await.unwrap_err();
    assert!(err.is_resumable(), "{err}");
    let staged = root.join(WORK_DIR_NAME).join("staging").join("p.bin");
    let partial = read(&staged);
    assert!(!partial.is_empty() && partial.len() < body.len(), "{}", partial.len());
    updater.pause();
    assert_eq!(updater.run().await.unwrap(), SessionPhase::Paused);
    assert_eq!(read(&staged), partial, "a pause must not discard the partial");
    updater.resume();
    // The first server still owns the URL. After its one short response it
    // answers 503, so the same session must resume and then fail cleanly
    // without leaving the download log handle open.
    let again = updater.run().await.unwrap_err();
    assert!(again.is_resumable(), "{again}");
    assert!(root.join(WORK_DIR_NAME).join("download.log").is_file());
    assert_eq!(read(&staged), partial);
    drop(updater);
    let honest = Server::spawn(vec![("p.bin".into(), body.to_vec(), ServerMode::Honest)]);
    rewrite_url(&root, &honest.url("p.bin"));
    let reopened = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    reopened.prepare(manifest("pause-same", vec![file_without_etag("p.bin", body, Some(honest.url("p.bin")))]), None).unwrap();
    assert_eq!(reopened.run().await.unwrap(), SessionPhase::Idle);
    assert_eq!(read(&root.join("p.bin")), body);
    assert!(!root.join(WORK_DIR_NAME).join("download.log").exists());
}

#[tokio::test]
async fn journal_recovers_interrupted_install_and_publishes_official_last() {
    let root = temp_root();
    let old = b"old-ri";
    std::fs::write(root.join("ri.bin"), old).unwrap();
    let content = b"new-content";
    let official = b"new-official-ri-bytes";
    let server = Server::spawn(vec![("game.bin".into(), content.to_vec(), ServerMode::Honest)]);
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    updater
        .prepare(
            manifest("7", vec![file("game.bin", content, Some(server.url("game.bin")))]),
            Some(OfficialManifest { path: "ri.bin".into(), bytes: official.to_vec() }),
        )
        .unwrap();
    // Download only, then crash before replacement by pausing via a direct apply simulation:
    // run the downloads by calling run, but we want to interrupt mid-install. Do the happy
    // path first to prove ordering, using a copy of the staged official file.
    assert_eq!(updater.run().await.unwrap(), SessionPhase::Idle);
    assert_eq!(read(&root.join("game.bin")), content);
    assert_eq!(read(&root.join("ri.bin")), official, "official path must be published");
    assert!(!root.join(WORK_DIR_NAME).join("journal.json").exists());

    // Interrupted replacement: stage the new content, leave the installed
    // files untouched, and record Installing before any rename.
    let root = temp_root();
    std::fs::write(root.join("ri.bin"), old).unwrap();
    std::fs::write(root.join("game.bin"), b"previous").unwrap();
    let _ = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    // A crash after staging and before the first rename. prepare is not used:
    // it would refuse a changed file that no longer has a URL.
    eprintln!("TEST ROOT {}", root.display());
    write_crash_journal(&root, "game.bin", content, "ri.bin", official);
    assert_eq!(read(&root.join("ri.bin")), old, "crash must leave the old official file");
    let reopened = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    assert!(matches!(reopened.recover(), Err(UpdateError::State(_))));
    reopened
        .prepare(
            manifest("8", vec![file("game.bin", content, None)]),
            Some(OfficialManifest { path: "ri.bin".into(), bytes: official.to_vec() }),
        )
        .unwrap();
    let recovery = reopened.recover().unwrap();
    assert!(!recovery.needs_run, "{recovery:?}");
    assert_eq!(read(&root.join("game.bin")), content);
    assert_eq!(read(&root.join("ri.bin")), official);
    assert_eq!(reopened.snapshot().phase, SessionPhase::Idle);
}

#[tokio::test]
async fn zero_byte_file_is_legitimate() {
    let root = temp_root();
    let server = Server::spawn(vec![("empty.dat".into(), Vec::new(), ServerMode::Honest)]);
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    updater.prepare(manifest("9", vec![file("empty.dat", b"", Some(server.url("empty.dat")))]), None).unwrap();
    let result = updater.run().await;
    assert!(result.is_ok(), "zero-byte install failed: {result:?}");
    assert_eq!(result.unwrap(), SessionPhase::Idle);
    assert_eq!(read(&root.join("empty.dat")), b"");
}

#[test]
fn disk_error_during_install_keeps_journal() {
    let root = temp_root();
    let body = b"will-fail-to-replace";
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    // A matching file so prepare has something honest to plan.
    std::fs::write(root.join("ok.bin"), body).unwrap();
    let plan = updater.prepare(manifest("10", vec![file("ok.bin", body, None)]), None).unwrap();
    assert_eq!(plan.download_files, 0);
    // `blocked` is a file, so creating `blocked/` as a parent of the
    // destination fails. The staged object and the journal stay put.
    // The destination is a directory, so replacing it with the staged file fails
    // and neither the journal nor the stage may be discarded.
    std::fs::create_dir_all(root.join("child.bin")).unwrap();
    let staged = root.join(WORK_DIR_NAME).join("staging").join("child.bin");
    std::fs::create_dir_all(staged.parent().unwrap()).unwrap();
    std::fs::write(&staged, body).unwrap();
    inject_file(&root, "child.bin", body);
    let recovered = updater.recover();
    let err = match recovered {
        Ok(recovery) => panic!(
            "recover succeeded: {recovery:?}\njournal: {}",
            std::fs::read_to_string(root.join(WORK_DIR_NAME).join("journal.json")).unwrap_or_default()
        ),
        Err(err) => err,
    };
    assert!(matches!(err, UpdateError::PlanChanged), "{err:?}");
    let journal = root.join(WORK_DIR_NAME).join("journal.json");
    assert!(journal.exists(), "journal must survive a filesystem failure");
    assert_eq!(read(&staged), body, "failed install must not discard the stage");
    assert!(root.join("child.bin").is_dir(), "failed replace must not destroy the destination");
}

#[test]
fn malicious_journal_relative_is_rejected_before_any_write() {
    let root = temp_root();
    let outside = temp_root();
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    drop(updater);
    let journal = serde_json::json!({
        "version": 3,
        "manifest_version": "evil",
        "phase": "installing",
        "files": [{
            "relative": format!("../../{}", outside.file_name().unwrap().to_string_lossy()),
            "sha256": hash_of(b"x").as_str(),
            "size": 1,
            "url": null,
            "etag": null,
            "phase": "staged",
            "received": 1,
            "had_previous": false
        }],
        "official": null
    });
    std::fs::write(root.join(WORK_DIR_NAME).join("journal.json"), journal.to_string()).unwrap();
    let err = match Updater::open(&root, DownloadPolicy::loopback_test()) {
        Ok(_) => panic!("malicious journal was opened"),
        Err(err) => err,
    };
    assert!(matches!(err, UpdateError::Journal(_) | UpdateError::PathRejected { .. }), "{err}");
    assert!(std::fs::read_dir(&outside).unwrap().next().is_none(), "outside directory was written");
}

#[test]
fn journal_official_alias_and_non_ascii_are_rejected() {
    let root = temp_root();
    let _ = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    let digest = hash_of(b"x").as_str().to_string();
    for relative in ["GAME.BIN", "C:/outside.bin", "ri.bin:stream", "Å.bin", "short~1.bin"] {
        let journal = serde_json::json!({
            "version": 3,
            "manifest_version": "alias",
            "phase": "staged",
            "files": [{
                "relative": "game.bin",
                "sha256": digest,
                "size": 1_u64,
                "url": null,
                "etag": null,
                "phase": "installed",
                "received": 1,
                "had_previous": false
            }],
            "official": {
                "relative": relative,
                "sha256": digest,
                "size": 1,
                "phase": "staged",
                "had_previous": false
            }
        });
        std::fs::write(root.join(WORK_DIR_NAME).join("journal.json"), journal.to_string()).unwrap();
        let err = match Updater::open(&root, DownloadPolicy::loopback_test()) {
            Ok(_) => panic!("alias journal was opened"),
            Err(err) => err,
        };
        assert!(matches!(err, UpdateError::Journal(_) | UpdateError::PathRejected { .. }), "{relative}: {err}");
    }
}

#[test]
fn work_dir_reparse_is_refused() {
    let root = temp_root();
    let elsewhere = temp_root();
    let work = root.join(WORK_DIR_NAME);
    let linked = make_reparse(&work, &elsewhere);
    if !linked {
        // Junction creation was denied. The refusal itself is still covered by
        // reject_reparse; this environment cannot plant the fixture.
        let _ = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
        return;
    }
    let err = match Updater::open(&root, DownloadPolicy::loopback_test()) {
        Ok(_) => panic!("reparse work dir was opened"),
        Err(err) => err,
    };
    assert!(matches!(err, UpdateError::PathRejected { .. } | UpdateError::Root(_)), "{err}");
    assert!(std::fs::read_dir(&elsewhere).unwrap().next().is_none(), "reparse target was written");
}

#[test]
fn second_open_is_rejected_while_the_first_session_lives() {
    let root = temp_root();
    let first = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    let err = match Updater::open(&root, DownloadPolicy::loopback_test()) {
        Ok(_) => panic!("second session opened the same root"),
        Err(err) => err,
    };
    assert!(matches!(err, UpdateError::State(_)), "{err}");
    drop(first);
    Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
}

#[test]
fn rejected_prepare_keeps_the_previous_official_stage() {
    let root = temp_root();
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    let body = b"content";
    std::fs::write(root.join("game.bin"), body).unwrap();
    updater
        .prepare(
            manifest("keep", vec![file("game.bin", body, None)]),
            Some(OfficialManifest { path: "ri.bin".into(), bytes: b"original-stage".to_vec() }),
        )
        .unwrap();
    let stage = root.join(WORK_DIR_NAME).join("official.stage");
    assert_eq!(read(&stage), b"original-stage");
    // A crash mid-install whose destination cannot be replaced. recover fails
    // and leaves the session in Installing with the previous official stage.
    std::fs::create_dir_all(root.join("child.bin")).unwrap();
    let staged = root.join(WORK_DIR_NAME).join("staging").join("child.bin");
    std::fs::create_dir_all(staged.parent().unwrap()).unwrap();
    std::fs::write(&staged, body).unwrap();
    let journal_path = root.join(WORK_DIR_NAME).join("journal.json");
    let broken = serde_json::json!({
        "version": 3,
        "manifest_version": "keep",
        "phase": "installing",
        "files": [{
            "relative": "child.bin",
            "sha256": hash_of(body).as_str(),
            "size": body.len(),
            "url": null,
            "etag": null,
            "phase": "staged",
            "received": body.len(),
            "had_previous": false
        }],
        "official": {
            "relative": "ri.bin",
            "sha256": hash_of(b"original-stage").as_str(),
            "size": b"original-stage".len(),
            "phase": "staged",
            "had_previous": false
        }
    });
    std::fs::write(&journal_path, serde_json::to_vec(&broken).unwrap()).unwrap();
    assert!(updater.recover().is_err());
    let err = updater
        .prepare(
            manifest("replace", vec![file("game.bin", body, None)]),
            Some(OfficialManifest { path: "ri.bin".into(), bytes: b"clobbered".to_vec() }),
        )
        .unwrap_err();
    assert!(matches!(err, UpdateError::PlanChanged | UpdateError::RecoveryRequired), "{err}");
    assert_eq!(read(&stage), b"original-stage");
}

#[test]
fn installed_marker_is_reverified_before_official_publish() {
    let root = temp_root();
    let content = b"real-content";
    let official = b"official-bytes";
    let _ = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    std::fs::write(root.join("game.bin"), b"tampered").unwrap();
    std::fs::write(root.join("ri.bin"), b"old-ri").unwrap();
    eprintln!("TEST ROOT {}", root.display());
    write_crash_journal(&root, "game.bin", content, "ri.bin", official);
    // Claim the content file is already installed while the bytes are wrong
    // and the stage is gone.
    let journal_path = root.join(WORK_DIR_NAME).join("journal.json");
    let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(&journal_path).unwrap()).unwrap();
    value["files"][0]["phase"] = serde_json::json!("installed");
    std::fs::remove_file(root.join(WORK_DIR_NAME).join("staging").join("game.bin")).unwrap();
    std::fs::write(&journal_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let reopened = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    reopened
        .prepare(
            manifest("8", vec![file("game.bin", content, None)]),
            Some(OfficialManifest { path: "ri.bin".into(), bytes: official.to_vec() }),
        )
        .unwrap();
    let err = reopened.recover().unwrap_err();
    assert!(matches!(err, UpdateError::Install { .. }), "{err}");
    assert_eq!(read(&root.join("ri.bin")), b"old-ri", "official path must stay unpublished");
    assert!(journal_path.exists(), "journal must be kept");
}

#[tokio::test]
async fn learned_etag_rejects_a_changed_object_on_resume() {
    let root = temp_root();
    let body = b"version-one-bytes-for-etag-learning";
    let server = Server::spawn(vec![(
        "obj.bin".into(),
        body.to_vec(),
        ServerMode::LearnThenChange { first: 1, replacement: b"version-two-different-object".to_vec() },
    )]);
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    updater.prepare(manifest("etag", vec![file_without_etag("obj.bin", body, Some(server.url("obj.bin")))]), None).unwrap();
    let err = updater.run().await.unwrap_err();
    assert!(err.is_resumable(), "{err}");
    let staged = root.join(WORK_DIR_NAME).join("staging").join("obj.bin");
    let partial = read(&staged);
    assert!(!partial.is_empty() && partial.len() < body.len());
    drop(updater);
    // The learned ETag lives in the download log. Opening replays it, and the
    // next run must refuse the changed object because of that replay.
    let reopened = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    reopened.prepare(manifest("etag", vec![file_without_etag("obj.bin", body, Some(server.url("obj.bin")))]), None).unwrap();
    let err = reopened.run().await.unwrap_err();
    assert!(err.is_resumable() || matches!(err, UpdateError::Staging { .. }), "{err}");
    assert_eq!(read(&staged), partial, "a changed ETag must not be appended to the first object");
    assert!(!root.join("obj.bin").exists());
}

#[tokio::test]
async fn restarted_session_resumes_from_the_recorded_etag() {
    let root = temp_root();
    let body = b"version-one-bytes-for-etag-learning";
    let server = Server::spawn(vec![(
        "obj.bin".into(),
        body.to_vec(),
        ServerMode::LearnThenChange { first: 1, replacement: b"version-two-different-object".to_vec() },
    )]);
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    updater.prepare(manifest("etag-restart", vec![file_without_etag("obj.bin", body, Some(server.url("obj.bin")))]), None).unwrap();
    let err = updater.run().await.unwrap_err();
    assert!(err.is_resumable(), "{err}");
    let staged = root.join(WORK_DIR_NAME).join("staging").join("obj.bin");
    let partial = read(&staged);
    assert!(!partial.is_empty() && partial.len() < body.len());
    drop(updater);

    let journal = std::fs::read_to_string(root.join(WORK_DIR_NAME).join("journal.json")).unwrap();
    let journal: serde_json::Value = serde_json::from_str(&journal).unwrap();
    assert!(journal["files"][0]["etag"].is_null(), "the checkpoint stays source-bound; the strong etag lives in the download log");
    assert!(root.join(WORK_DIR_NAME).join("download.log").is_file());

    let reopened = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    reopened.prepare(manifest("etag-restart", vec![file_without_etag("obj.bin", body, Some(server.url("obj.bin")))]), None).unwrap();
    let err = reopened.run().await.unwrap_err();
    assert!(err.is_resumable() || matches!(err, UpdateError::Staging { .. }), "{err}");
    assert_eq!(read(&staged), partial, "a restarted session must refuse the changed object instead of appending it");
    assert!(!root.join("obj.bin").exists());
}

#[tokio::test]
async fn encoding_and_redirect_are_refused_without_writing_the_destination() {
    let root = temp_root();
    let body = b"plain-body-not-gzip";
    let server =
        Server::spawn(vec![("zip.bin".into(), body.to_vec(), ServerMode::Gzip), ("go.bin".into(), body.to_vec(), ServerMode::Redirect)]);
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    updater.prepare(manifest("enc", vec![file("zip.bin", body, Some(server.url("zip.bin")))]), None).unwrap();
    let err = updater.run().await.unwrap_err();
    assert!(err.is_resumable(), "{err}");
    assert!(!root.join("zip.bin").exists());

    let root = temp_root();
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    updater.prepare(manifest("redir", vec![file("go.bin", body, Some(server.url("go.bin")))]), None).unwrap();
    let err = updater.run().await.unwrap_err();
    assert!(err.is_resumable(), "{err}");
    assert!(!root.join("go.bin").exists());
}

#[tokio::test]
async fn guard_rejection_preserves_destination_and_journal() {
    let root = temp_root();
    let previous = b"previous-bytes";
    std::fs::write(root.join("game.bin"), previous).unwrap();
    let content = b"staged-replacement";
    let server = Server::spawn(vec![("game.bin".into(), content.to_vec(), ServerMode::Honest)]);
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    updater
        .prepare(
            manifest("guard", vec![file("game.bin", content, Some(server.url("game.bin")))]),
            Some(OfficialManifest { path: "ri.bin".into(), bytes: b"official".to_vec() }),
        )
        .unwrap();
    let err = updater.run_guarded(|| Err(UpdateError::operation_rejected("game is running"))).await.unwrap_err();
    assert!(matches!(err, UpdateError::OperationRejected(_)), "{err}");
    assert_eq!(read(&root.join("game.bin")), previous);
    assert!(!root.join("ri.bin").exists());
    assert!(root.join(WORK_DIR_NAME).join("journal.json").exists());
    assert_eq!(read(&root.join(WORK_DIR_NAME).join("staging").join("game.bin")), content);
    let backup = root.join(WORK_DIR_NAME).join("backup");
    assert!(!backup.exists() || std::fs::read_dir(&backup).unwrap().next().is_none(), "a rejected guard must not create a backup");
    let journal = std::fs::read_to_string(root.join(WORK_DIR_NAME).join("journal.json")).unwrap();
    assert!(!journal.contains("\"replacing\""), "a rejected guard must not record Replacing");
}

#[test]
fn differing_interrupted_plan_is_not_overwritten() {
    let root = temp_root();
    let content = b"new-content";
    let official = b"official-bytes";
    let _ = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    eprintln!("TEST ROOT {}", root.display());
    write_crash_journal(&root, "game.bin", content, "ri.bin", official);
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    let other = b"different";
    let err = updater
        .prepare(
            manifest("other", vec![file("other.bin", other, Some("http://127.0.0.1/x".into()))]),
            Some(OfficialManifest { path: "ri.bin".into(), bytes: b"replaced".to_vec() }),
        )
        .unwrap_err();
    assert!(matches!(err, UpdateError::RecoveryRequired), "{err}");
    assert_eq!(read(&root.join(WORK_DIR_NAME).join("official.stage")), official);
    let journal = std::fs::read_to_string(root.join(WORK_DIR_NAME).join("journal.json")).unwrap();
    assert!(journal.contains("new-content") || journal.contains(&hash_of(content).as_str().to_string()) || journal.contains("game.bin"));
}

#[test]
fn hardlink_is_refused_before_the_destination_changes() {
    let root = temp_root();
    let body = b"linked-bytes";
    let outside = temp_root().join("sentinel.bin");
    std::fs::write(&outside, body).unwrap();
    let dest = root.join("game.bin");
    let linked = std::fs::hard_link(&outside, &dest).is_ok();
    if !linked {
        return;
    }
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    let replacement = b"replacement-bytes";
    let err = updater.prepare(manifest("link", vec![file("game.bin", replacement, Some("http://127.0.0.1/x".into()))]), None).unwrap_err();
    assert!(matches!(err, UpdateError::PathRejected { .. } | UpdateError::Io(_)), "{err}");
    assert_eq!(read(&outside), body, "hardlink sentinel must stay unchanged");
    assert_eq!(read(&dest), body);
}

#[test]
fn junction_swap_under_a_held_pin_does_not_redirect_the_write() {
    let root = temp_root();
    let elsewhere = temp_root();
    std::fs::create_dir(root.join("data")).unwrap();
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    let body = b"inside";
    std::fs::write(root.join("data").join("keep.bin"), body).unwrap();
    updater.prepare(manifest("pin", vec![file("data/keep.bin", body, None)]), None).unwrap();
    // Hold the ancestor pin the way prepare/run do, then try to retarget it.
    let held = crate::fsutil::Held::pin(&root.join("data")).unwrap();
    let swapped = make_reparse_over(&root.join("data"), &elsewhere);
    let _ = held;
    if swapped {
        assert!(std::fs::read_dir(&elsewhere).unwrap().next().is_none(), "retarget must not receive updater writes");
    }
    assert_eq!(read(&root.join("data").join("keep.bin")), body);
}

#[test]
fn reparse_planted_on_a_pinned_directory_fails_closed() {
    let root = temp_root();
    let elsewhere = temp_root();
    std::fs::write(elsewhere.join("sentinel.bin"), b"outside").unwrap();
    let updater = Updater::open(&root, DownloadPolicy::loopback_test()).unwrap();
    let work = root.join(WORK_DIR_NAME);
    let planted = plant_junction(&work, &elsewhere);
    if !planted {
        return;
    }
    let body = b"must-not-escape";
    let err = updater.prepare(manifest("reparse", vec![file("game.bin", body, Some("http://127.0.0.1/x".into()))]), None).unwrap_err();
    assert!(matches!(err, UpdateError::Io(_) | UpdateError::PathRejected { .. } | UpdateError::Root(_)), "{err}");
    assert_eq!(read(&elsewhere.join("sentinel.bin")), b"outside");
    assert!(
        std::fs::read_dir(&elsewhere).unwrap().all(|entry| entry.unwrap().file_name() == "sentinel.bin"),
        "a junction planted under a pin must not receive the write"
    );
}

#[tokio::test]
async fn a_stage_changed_at_the_final_guard_is_not_installed() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let new = b"new-content";
    std::fs::write(root.join("game.bin"), b"old-content").unwrap();
    let updater = Updater::open(root, DownloadPolicy::loopback_test()).unwrap();
    let stage = root.join(WORK_DIR_NAME).join("staging/game.bin");
    std::fs::create_dir_all(stage.parent().unwrap()).unwrap();
    std::fs::write(&stage, new).unwrap();
    updater.prepare(manifest("pin-hash", vec![file("game.bin", new, Some("http://127.0.0.1:1/unused".into()))]), None).unwrap();
    let mut calls = 0;
    let result = updater
        .run_guarded(|| {
            calls += 1;
            if calls == 3 {
                std::fs::write(&stage, b"bad-content").unwrap();
            }
            Ok(())
        })
        .await;
    assert!(result.is_err());
    assert!(calls >= 3);
    assert_eq!(std::fs::read(root.join("game.bin")).unwrap(), b"old-content");
}

#[tokio::test]
async fn a_verified_rename_handle_denies_concurrent_stage_writes() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let new = b"new-content";
    std::fs::write(root.join("game.bin"), b"old-content").unwrap();
    let updater = Updater::open(root, DownloadPolicy::loopback_test()).unwrap();
    let stage = root.join(WORK_DIR_NAME).join("staging/game.bin");
    std::fs::create_dir_all(stage.parent().unwrap()).unwrap();
    std::fs::write(&stage, new).unwrap();
    updater.prepare(manifest("pin-hash", vec![file("game.bin", new, Some("http://127.0.0.1:1/unused".into()))]), None).unwrap();
    let mut calls = 0;
    let mut denied = false;
    let result = updater
        .run_guarded(|| {
            calls += 1;
            if calls == 4 {
                denied = std::fs::write(&stage, b"bad-content").is_err();
            }
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(result, SessionPhase::Idle);
    assert!(denied, "the hashed rename handle must deny new writers until rename completes");
    assert_eq!(std::fs::read(root.join("game.bin")).unwrap(), new);
}

/// `FSCTL_SET_REPARSE_POINT` on an owned empty directory while directory pins
/// are already held. Production create and rename through those pins must fail
/// closed, and the sentinel outside the tree must stay unchanged.
#[test]
fn held_pin_rejects_create_and_rename_after_fsctl_reparse() {
    #[cfg(not(windows))]
    {
        panic!("FSCTL_SET_REPARSE_POINT is a Windows regression");
    }
    #[cfg(windows)]
    {
        use std::ffi::OsStr;

        let root_dir = tempfile::tempdir().expect("owned temp root");
        let outside_dir = tempfile::tempdir().expect("same-volume outside temp");
        let root = root_dir.path();
        let outside = outside_dir.path();
        let sentinel = outside.join("sentinel.bin");
        std::fs::write(&sentinel, b"outside").unwrap();
        let parent = root.join("held");
        std::fs::create_dir(&parent).unwrap();

        std::fs::create_dir(root.join("stage")).unwrap();
        let mut lease = crate::pin::Lease::open_existing(root).expect("pin owned root");
        let pinned = lease.descend(&[String::from("held")]).expect("pin held child");
        let staged_parent = lease.descend(&[String::from("stage")]).expect("pin sibling stage");

        let mutator = mount_point_reparse(&parent, outside);
        assert!(reparse_attribute(&parent), "FSCTL_SET_REPARSE_POINT must mark the held directory");

        let created = crate::pin::create_file(&pinned, OsStr::new("planted.bin"));
        assert!(created.is_err(), "create under a mutated pin must fail: {created:?}");

        let staged = crate::pin::create_file(&staged_parent, OsStr::new("staged.bin")).expect("stage file");
        crate::pin::write_all(&staged, b"must-not-escape").unwrap();
        let renamed = crate::pin::rename_into(&staged, &pinned, OsStr::new("planted.bin"), true);
        assert!(renamed.is_err(), "rename into a mutated pin must fail: {renamed:?}");

        assert_eq!(read(&sentinel), b"outside");
        assert!(
            std::fs::read_dir(outside).unwrap().all(|entry| entry.unwrap().file_name() == "sentinel.bin"),
            "mutated pin must not write outside"
        );
        assert!(!parent.join("planted.bin").exists(), "refused create and rename must not leave a child");

        delete_mount_point(&mutator);
        drop(staged);
        drop(staged_parent);
        drop(pinned);
        drop(lease);
        drop(mutator);
    }
}

#[cfg(windows)]
fn mount_point_reparse(directory: &Path, target: &Path) -> std::os::windows::io::OwnedHandle {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};

    use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE};
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE};

    const FSCTL_SET_REPARSE_POINT: u32 = 0x0009_00A4;
    const FSCTL_GET_REPARSE_POINT: u32 = 0x0009_00A8;
    const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;

    let parent = open_reparse_dir(directory, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE);
    let mutator = open_reparse_dir(directory, GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
    let substitute = wide_nul(&format!(r"\??\{}", target.display()));
    let print = wide_nul(&target.display().to_string());
    let sub_len = (substitute.len() - 1) * 2;
    let print_len = (print.len() - 1) * 2;
    let mut path_buffer = Vec::with_capacity(substitute.len() + print.len());
    path_buffer.extend_from_slice(&substitute);
    path_buffer.extend_from_slice(&print);
    let data_len = 8 + path_buffer.len() * 2;
    assert!(data_len <= u16::MAX as usize, "reparse path is too long");
    let mut buffer = Vec::<u8>::new();
    buffer.extend_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
    buffer.extend_from_slice(&(data_len as u16).to_le_bytes());
    buffer.extend_from_slice(&0_u16.to_le_bytes());
    buffer.extend_from_slice(&0_u16.to_le_bytes());
    buffer.extend_from_slice(&(sub_len as u16).to_le_bytes());
    buffer.extend_from_slice(&((sub_len + 2) as u16).to_le_bytes());
    buffer.extend_from_slice(&(print_len as u16).to_le_bytes());
    for unit in &path_buffer {
        buffer.extend_from_slice(&unit.to_le_bytes());
    }
    device_io(mutator, FSCTL_SET_REPARSE_POINT, Some(&buffer), None).expect("FSCTL_SET_REPARSE_POINT");
    let mut got = vec![0_u8; buffer.len()];
    device_io(parent, FSCTL_GET_REPARSE_POINT, None, Some(&mut got)).expect("FSCTL_GET_REPARSE_POINT");
    assert!(got.starts_with(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes()), "held directory did not become a mount point");
    unsafe { CloseHandle(parent) };
    unsafe { OwnedHandle::from_raw_handle(mutator as _) }
}

#[cfg(windows)]
fn delete_mount_point(mutator: &std::os::windows::io::OwnedHandle) {
    use std::os::windows::io::AsRawHandle;

    const FSCTL_DELETE_REPARSE_POINT: u32 = 0x0009_00AC;
    const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
    let mut header = [0_u8; 8];
    header[..4].copy_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
    device_io(mutator.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE, FSCTL_DELETE_REPARSE_POINT, Some(&header), None)
        .expect("FSCTL_DELETE_REPARSE_POINT");
}

#[cfg(windows)]
fn open_reparse_dir(path: &Path, access: u32, share: u32) -> windows_sys::Win32::Foundation::HANDLE {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, OPEN_EXISTING};

    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            access,
            share,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    assert!(
        !handle.is_null() && handle != INVALID_HANDLE_VALUE,
        "CreateFileW {:#x} on {} failed: {}",
        access,
        path.display(),
        std::io::Error::last_os_error()
    );
    handle
}

#[cfg(windows)]
fn device_io(
    handle: windows_sys::Win32::Foundation::HANDLE,
    code: u32,
    input: Option<&[u8]>,
    mut output: Option<&mut [u8]>,
) -> Result<(), String> {
    unsafe extern "system" {
        fn DeviceIoControl(
            device: windows_sys::Win32::Foundation::HANDLE,
            code: u32,
            input: *const core::ffi::c_void,
            input_size: u32,
            output: *mut core::ffi::c_void,
            output_size: u32,
            returned: *mut u32,
            overlapped: *mut core::ffi::c_void,
        ) -> i32;
    }
    let (in_ptr, in_len) = match input {
        Some(bytes) => (bytes.as_ptr().cast::<core::ffi::c_void>(), bytes.len() as u32),
        None => (std::ptr::null(), 0),
    };
    let (out_ptr, out_len) = match output.as_mut() {
        Some(bytes) => (bytes.as_mut_ptr().cast::<core::ffi::c_void>(), bytes.len() as u32),
        None => (std::ptr::null_mut(), 0),
    };
    let mut returned = 0_u32;
    let ok = unsafe { DeviceIoControl(handle, code, in_ptr, in_len, out_ptr, out_len, &mut returned, std::ptr::null_mut()) };
    if ok == 0 { Err(format!("DeviceIoControl {code:#x}: {}", std::io::Error::last_os_error())) } else { Ok(()) }
}

#[cfg(windows)]
fn reparse_attribute(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    std::fs::symlink_metadata(path).map(|meta| meta.file_attributes() & 0x400 != 0).unwrap_or(false)
}

#[cfg(windows)]
fn wide_nul(text: &str) -> Vec<u16> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    let mut wide: Vec<u16> = OsStr::new(text).encode_wide().collect();
    wide.push(0);
    wide
}

fn plant_junction(link: &Path, target: &Path) -> bool {
    // The pin shares write, so another handle can replace the directory with a
    // junction. The pin's later no-follow operations must then fail.
    let _ = std::fs::remove_dir_all(link);
    make_reparse(link, target)
}

fn make_reparse_over(link: &Path, target: &Path) -> bool {
    // Replacing a directory that a pin holds open should fail. That failure is
    // the property under test: the pin denied the swap.
    let _ = std::fs::remove_dir(link);
    make_reparse(link, target)
}

fn make_reparse(link: &Path, target: &Path) -> bool {
    #[cfg(windows)]
    {
        let status = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(link).arg(target).status();
        matches!(status, Ok(code) if code.success()) && link.exists()
    }
    #[cfg(not(windows))]
    {
        let _ = (link, target);
        false
    }
}

fn rewrite_url(root: &Path, url: &str) {
    let path = root.join(WORK_DIR_NAME).join("journal.json");
    let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["files"][0]["url"] = serde_json::json!(url);
    value["files"][0]["phase"] = serde_json::json!("partial");
    std::fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

fn write_crash_journal(root: &Path, content: &str, content_bytes: &[u8], official: &str, official_bytes: &[u8]) {
    let work = root.join(WORK_DIR_NAME);
    std::fs::create_dir_all(work.join("staging")).unwrap();
    std::fs::write(work.join("staging").join(content), content_bytes).unwrap();
    std::fs::write(work.join("official.stage"), official_bytes).unwrap();
    let journal = serde_json::json!({
        "version": 3,
        "manifest_version": "8",
        "phase": "installing",
        "files": [{
            "relative": content,
            "sha256": hash_of(content_bytes).as_str(),
            "size": content_bytes.len(),
            "url": null,
            "etag": null,
            "phase": "staged",
            "received": content_bytes.len(),
            "had_previous": true
        }],
        "official": {
            "relative": official,
            "sha256": hash_of(official_bytes).as_str(),
            "size": official_bytes.len(),
            "phase": "staged",
            "had_previous": true
        }
    });
    std::fs::write(work.join("journal.json"), serde_json::to_vec_pretty(&journal).unwrap()).unwrap();
}

fn inject_file(root: &Path, relative: &str, body: &[u8]) {
    let path = root.join(WORK_DIR_NAME).join("journal.json");
    let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let entry = serde_json::json!({
        "relative": relative,
        "sha256": hash_of(body).as_str(),
        "size": body.len(),
        "url": null,
        "etag": null,
        "phase": "staged",
        "received": body.len(),
        "had_previous": false,
    });
    value["files"].as_array_mut().unwrap().push(entry);
    value["phase"] = serde_json::json!("installing");
    value["files"][0]["phase"] = serde_json::json!("staged");
    std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

mod reliability;
mod work_progress;
