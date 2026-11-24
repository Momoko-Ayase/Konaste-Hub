//! Installation-only fixture shared by the regression test and timing example.
//! It measures the real Updater interface with already-staged synthetic files.
use std::{
    ffi::c_void,
    time::{Duration, Instant},
};

use konaste_updater::{DownloadPolicy, Manifest, ManifestFile, OfficialManifest, SessionPhase, Sha256, Updater, WORK_DIR_NAME};
use sha2::{Digest, Sha256 as Hasher};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

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
}

pub async fn measure(files: usize) -> Result<Measurement> {
    let directory = tempfile::tempdir()?;
    let root = directory.path();
    let updater = Updater::open_fresh(root, DownloadPolicy::loopback_test())?;
    let staging = root.join(WORK_DIR_NAME).join("staging/data");
    std::fs::create_dir_all(&staging)?;
    let body = [b'x'; 64];
    let sum = Sha256::parse(&hex::encode(Hasher::digest(body)))?;
    let mut entries = Vec::with_capacity(files);
    for index in 0..files {
        let name = format!("{index:06}.bin");
        std::fs::write(staging.join(&name), body)?;
        entries.push(ManifestFile {
            relative_path: format!("data/{name}"),
            sha256: sum.clone(),
            size: body.len() as u64,
            url: Some(format!("http://127.0.0.1:1/unused/{name}")),
            etag: None,
        });
    }
    updater.prepare(
        Manifest { version: "offline-install-benchmark".into(), files: entries },
        Some(OfficialManifest { path: "ri.bin".into(), bytes: b"synthetic official index".to_vec() }),
    )?;
    let before = io_counters()?;
    let started = Instant::now();
    if updater.run().await? != SessionPhase::Idle {
        return Err("installation did not complete".into());
    }
    let elapsed = started.elapsed();
    let after = io_counters()?;
    for index in 0..files {
        if std::fs::read(root.join(format!("data/{index:06}.bin")))? != body {
            return Err("installed bytes differ".into());
        }
    }
    if std::fs::read(root.join("ri.bin"))? != b"synthetic official index" {
        return Err("official index was not published".into());
    }
    Ok(Measurement {
        files,
        elapsed,
        written: after.write_bytes - before.write_bytes,
        writes: after.write_operations - before.write_operations,
    })
}
