//! The index supplies no ETag. A real response validator must still survive an
//! interrupted body and a new updater session. Loopback only, synthetic bytes.
use std::{
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use konaste_updater::{DownloadPolicy, Manifest, ManifestFile, Sha256, UpdateError, Updater};
use sha2::{Digest, Sha256 as Hasher};

const TOTAL: usize = 64 * 1024;
const PREFIX: usize = 4096;

struct Server {
    url: String,
    resumed: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<(bool, String)>>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/object", listener.local_addr().unwrap());
        let resumed = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let phase = Arc::clone(&resumed);
        let stopped = Arc::clone(&stop);
        let captured = Arc::clone(&requests);
        let worker = thread::spawn(move || {
            let mut first = true;
            while !stopped.load(Ordering::SeqCst) {
                let mut stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(_) => break,
                };
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
                let Some(request) = request(&mut stream) else {
                    continue;
                };
                let is_resume = phase.load(Ordering::SeqCst);
                captured.lock().unwrap().push((is_resume, request));
                if first {
                    first = false;
                    let header = format!("HTTP/1.1 200 OK\r\nContent-Length: {TOTAL}\r\nETag: \"version-1\"\r\nConnection: close\r\n\r\n");
                    let _ = stream.write_all(header.as_bytes());
                    let _ = stream.write_all(&vec![b'A'; PREFIX]);
                    let _ = stream.flush();
                    thread::sleep(Duration::from_millis(25));
                    let _ = stream.shutdown(Shutdown::Both);
                } else {
                    let _ = stream.write_all(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                }
            }
        });
        Self { url, resumed, stop, requests, worker: Some(worker) }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}

fn request(stream: &mut TcpStream) -> Option<String> {
    let mut bytes = Vec::new();
    while !bytes.windows(4).any(|value| value == b"\r\n\r\n") {
        if bytes.len() >= 8192 {
            return None;
        }
        let mut chunk = [0; 1024];
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    String::from_utf8(bytes).ok().map(|value| value.to_ascii_lowercase())
}

fn manifest(url: &str) -> Manifest {
    Manifest {
        version: "synthetic-validator".into(),
        files: vec![ManifestFile {
            relative_path: "data/object.bin".into(),
            size: TOTAL as u64,
            sha256: Sha256::parse(&hex::encode(Hasher::digest(vec![b'A'; TOTAL]))).unwrap(),
            url: Some(url.into()),
            etag: None,
        }],
    }
}

#[tokio::test]
async fn changed_source_never_appends_an_unbound_old_prefix() {
    let root = tempfile::tempdir().unwrap();
    let server = Server::start();
    {
        let updater = Updater::open(root.path(), DownloadPolicy::loopback_test()).unwrap();
        updater.prepare(manifest(&server.url), None).unwrap();
        assert!(matches!(updater.run().await, Err(UpdateError::Download { .. })));
    }
    server.resumed.store(true, Ordering::SeqCst);
    let updater = Updater::open(root.path(), DownloadPolicy::loopback_test()).unwrap();
    let mut changed = manifest(&server.url);
    changed.version = "synthetic-new-resource".into();
    changed.files[0].sha256 = Sha256::parse(&hex::encode(Hasher::digest(vec![b'B'; TOTAL]))).unwrap();
    updater.prepare(changed, None).unwrap();
    assert!(matches!(updater.run().await, Err(UpdateError::Download { .. })));
    let requests = server.requests.lock().unwrap().clone();
    let resumed = requests.iter().find(|(resumed, _)| *resumed).expect("new source must request content");
    assert!(!resumed.1.contains("\r\nrange:"), "a prefix belonging to another expected hash is not resumable content for the new source");
    assert!(!resumed.1.contains("\r\nif-range:"));
    assert!(!root.path().join("data/object.bin").exists());
}

#[tokio::test]
async fn response_validator_survives_partial_body_and_fresh_source_rebind() {
    let root = tempfile::tempdir().unwrap();
    let server = Server::start();
    {
        let updater = Updater::open(root.path(), DownloadPolicy::loopback_test()).unwrap();
        updater.prepare(manifest(&server.url), None).unwrap();
        assert!(matches!(updater.run().await, Err(UpdateError::Download { .. })));
        assert!(updater.snapshot().resumable);
        assert!(!root.path().join("data/object.bin").exists());
    }
    server.resumed.store(true, Ordering::SeqCst);
    let updater = Updater::open(root.path(), DownloadPolicy::loopback_test()).unwrap();
    // Like a fresh official index, this still has no caller-supplied ETag.
    updater.prepare(manifest(&server.url), None).unwrap();
    assert!(matches!(updater.run().await, Err(UpdateError::Download { .. })));
    let requests = server.requests.lock().unwrap().clone();
    let resumed = requests.iter().find(|(resumed, _)| *resumed).expect("reopened session must request content");
    assert!(resumed.1.contains(&format!("\r\nrange: bytes={PREFIX}-\r\n")), "the retained partial must survive reopening");
    assert!(
        resumed.1.contains("\r\nif-range: \"version-1\"\r\n"),
        "learn the real response ETag before partial writes, then persist it across a fresh source bind"
    );
    assert!(!resumed.1.contains("\r\ncookie:"));
    assert!(!resumed.1.contains("\r\nauthorization:"));
    assert!(!root.path().join("data/object.bin").exists());
}
