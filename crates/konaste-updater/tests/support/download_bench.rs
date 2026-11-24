//! Download-phase fixture shared by the scaling test and timing example.
//!
//! It measures the production [`Updater`] against a loopback server of synthetic
//! bodies. `run_guarded` refuses the first installation mutation, so destination
//! copies are not part of the write counters. Nothing contacts an official host.

use std::{
    ffi::c_void,
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

use konaste_updater::{DownloadPolicy, Manifest, ManifestFile, Sha256, UpdateError, Updater, UpdaterOptions, WORK_DIR_NAME};
use sha2::{Digest, Sha256 as Hasher};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Default)]
#[repr(C)]
struct IoCounters {
    read_operations: u64,
    write_operations: u64,
    other_operations: u64,
    read_bytes: u64,
    write_bytes: u64,
    other_bytes: u64,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut c_void;
    fn GetProcessIoCounters(process: *mut c_void, counters: *mut IoCounters) -> i32;
}

fn io_counters() -> Result<IoCounters> {
    let mut counters = IoCounters::default();
    // SAFETY: the pseudo-handle is the current process; counters is a writable
    // IO_COUNTERS record with the six documented u64 fields.
    if unsafe { GetProcessIoCounters(GetCurrentProcess(), &mut counters) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(counters)
}

#[derive(Debug)]
pub struct Measurement {
    pub files: usize,
    pub elapsed: Duration,
    pub written: u64,
    pub writes: u64,
    pub journal_bytes: u64,
    pub staged_bytes: u64,
}

/// One strong ETag, identical body, no range. Connection closes after each file.
struct Server {
    addr: String,
}

impl Server {
    fn spawn(files: usize, body: &'static [u8]) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = match stream {
                    Ok(stream) => stream,
                    Err(_) => continue,
                };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buf = [0_u8; 2048];
                let n = stream.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    continue;
                }
                let req = String::from_utf8_lossy(&buf[..n]);
                let path = req.split_whitespace().nth(1).unwrap_or("/");
                let known = path
                    .rsplit_once('/')
                    .and_then(|(_, name)| name.strip_suffix(".bin"))
                    .and_then(|name| name.parse::<usize>().ok())
                    .is_some_and(|index| index < files);
                if !known {
                    let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                    continue;
                }
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"synthetic-v1\"\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body);
                let _ = stream.flush();
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut sink = [0_u8; 64];
                let _ = stream.read(&mut sink);
            }
        });
        Ok(Self { addr: format!("127.0.0.1:{}", addr.port()) })
    }

    fn url(&self, index: usize) -> String {
        format!("http://{addr}/{index:06}.bin", addr = self.addr)
    }
}

/// Download `files` synthetic objects, then refuse installation before any copy.
///
/// The manifest supplies no ETag, so the production path persists the strong
/// response ETag before the first partial byte and records completion after it.
pub async fn measure(files: usize) -> Result<Measurement> {
    if !(1..=4096).contains(&files) {
        return Err("file count must be 1..=4096".into());
    }
    let directory = tempfile::tempdir()?;
    let root = directory.path().to_path_buf();
    let body: &'static [u8] = b"synthetic-download-body-00000000000000000000000000000000";
    let server = Server::spawn(files, body)?;
    let sum = Sha256::parse(&hex::encode(Hasher::digest(body)))?;
    let entries = (0..files)
        .map(|index| ManifestFile {
            relative_path: format!("data/{index:06}.bin"),
            sha256: sum.clone(),
            size: body.len() as u64,
            url: Some(server.url(index)),
            etag: None,
        })
        .collect();
    let updater = Updater::open_fresh_with(&root, UpdaterOptions::new(DownloadPolicy::loopback_test()).parallel(2)?)?;
    updater.prepare(Manifest { version: "download-scaling".into(), files: entries }, None)?;

    let work = root.join(WORK_DIR_NAME);
    let staging = work.join("staging");
    let journal = work.join("journal.json");

    let before = io_counters()?;
    let started = Instant::now();
    // Called once, immediately before the first destination replacement. The
    // staged-phase checkpoint is inside this measurement; installation is not.
    let stopped = updater.run_guarded(|| Err(UpdateError::operation_rejected("download-bench-stop"))).await;
    let elapsed = started.elapsed();
    let after = io_counters()?;
    match stopped {
        Err(UpdateError::OperationRejected(reason)) if reason == "download-bench-stop" => {}
        other => return Err(format!("installation guard did not stop the run: {other:?}").into()),
    }
    if root.join("data").exists() {
        return Err("installation started after the guard refused it".into());
    }
    let mut staged_bytes = 0_u64;
    for index in 0..files {
        let staged = std::fs::read(staging.join(format!("data/{index:06}.bin")))?;
        if staged != body {
            return Err(format!("staged bytes differ for file {index}").into());
        }
        staged_bytes += staged.len() as u64;
    }
    // The checkpoint stays source-bound. Strong ETags live in download.log and
    // are visible only after the same replay a restarted session performs.
    drop(updater);
    let reopened = Updater::open(&root, DownloadPolicy::loopback_test())?;
    reopened.prepare(
        Manifest {
            version: "download-scaling".into(),
            files: (0..files)
                .map(|index| ManifestFile {
                    relative_path: format!("data/{index:06}.bin"),
                    sha256: sum.clone(),
                    size: body.len() as u64,
                    url: Some(server.url(index)),
                    etag: None,
                })
                .collect(),
        },
        None,
    )?;
    // recover replays the log into the journal, then checkpoints. It does not
    // install; a fully staged plan still needs run, which is outside this count.
    reopened.recover()?;
    let text = std::fs::read_to_string(&journal)?;
    let document: serde_json::Value = serde_json::from_str(&text)?;
    let recorded = document["files"]
        .as_array()
        .ok_or("journal has no files")?
        .iter()
        .filter(|file| file["etag"] == "\"synthetic-v1\"" && file["phase"] == "staged")
        .count();
    if recorded != files {
        return Err(format!("replay recorded {recorded} of {files} strong etags").into());
    }
    Ok(Measurement {
        files,
        elapsed,
        written: after.write_bytes - before.write_bytes,
        writes: after.write_operations - before.write_operations,
        journal_bytes: std::fs::metadata(&journal)?.len(),
        staged_bytes,
    })
}
