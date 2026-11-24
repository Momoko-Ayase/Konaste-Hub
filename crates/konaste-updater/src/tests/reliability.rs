//! Transport reliability against the production updater on loopback.
//!
//! The clock is the real one. Bodies are a few kilobytes, so a correct fetch
//! finishes in well under a second and a stalled fetch fails in a few seconds.
//! Nothing here opens an official host.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use super::{file, manifest, read, temp_root};
use crate::{DownloadPolicy, SessionPhase, Updater, UpdaterOptions, WORK_DIR_NAME};

const BODY_LEN: usize = 6 * 1024;

fn body() -> Vec<u8> {
    (0..BODY_LEN).map(|n| (n % 251) as u8).collect()
}

fn open_parallel(root: &std::path::Path) -> Updater {
    // One pace unit is 1ms in production. Here it is 1/50 ms, so the 15s
    // read-idle deadline is 300ms and five backoff waits stay under a few
    // seconds. Finer than this and tokio's sleep granularity stops scaling.
    Updater::open_with(
        root,
        UpdaterOptions::new(DownloadPolicy::loopback_test()).parallel(2).unwrap().pace(Duration::from_nanos(1_000_000 / 50)),
    )
    .unwrap()
}

/// What one accepted connection does. `hits` counts connections, not chunks.
#[derive(Clone)]
enum Fault {
    /// Write `bytes` of a correct 200/206, then drop the socket.
    Cut { bytes: usize },
    /// Correct body, `chunk` bytes every `gap`.
    Drip { gap: Duration, chunk: usize },
    /// Correct headers, then no body bytes until the client gives up.
    Stall,
    /// Status with `Retry-After`, then close. No body. The value is sent as-is.
    Status { code: u16, retry_after: Option<String> },
    /// Accept the connection and send nothing, including no status line.
    Silent,
    /// Full correct body.
    Honest,
}

struct Script {
    steps: Vec<Fault>,
    /// Connections past the script get this.
    rest: Fault,
}

struct Origin {
    addr: String,
    hits: Arc<AtomicU64>,
    ranges: Arc<Mutex<Vec<u64>>>,
}

fn serve(bytes: Vec<u8>, script: Script) -> Origin {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicU64::new(0));
    let ranges = Arc::new(Mutex::new(Vec::new()));
    let hits_t = Arc::clone(&hits);
    let ranges_t = Arc::clone(&ranges);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_nodelay(true);
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = [0_u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            if n == 0 {
                continue;
            }
            let req = String::from_utf8_lossy(&buf[..n]);
            let start = req
                .lines()
                .find_map(|line| {
                    let lower = line.to_ascii_lowercase();
                    let rest = lower.strip_prefix("range:")?.trim().strip_prefix("bytes=")?;
                    rest.split('-').next()?.parse::<u64>().ok()
                })
                .unwrap_or(0);
            ranges_t.lock().unwrap().push(start);
            let hit = hits_t.fetch_add(1, Ordering::SeqCst);
            let fault = script.steps.get(hit as usize).cloned().unwrap_or_else(|| script.rest.clone());
            // One thread per connection. A stalled answer sleeps; a serial
            // accept loop would then refuse the retry the client is testing.
            let bytes = bytes.clone();
            let mut stream = stream;
            thread::spawn(move || {
                let _ = answer(&mut stream, &bytes, start, &fault);
            });
        }
    });
    Origin { addr: format!("127.0.0.1:{}", addr.port()), hits, ranges }
}

fn answer(stream: &mut TcpStream, bytes: &[u8], start: u64, fault: &Fault) -> std::io::Result<()> {
    let start = start as usize;
    let etag = "\"v1\"";
    match fault {
        Fault::Status { code, retry_after } => {
            let mut head = format!("HTTP/1.1 {code} Unavailable\r\nContent-Length: 0\r\nETag: \"error-page\"\r\nConnection: close\r\n");
            if let Some(value) = retry_after {
                head.push_str(&format!("Retry-After: {value}\r\n"));
            }
            head.push_str("\r\n");
            stream.write_all(head.as_bytes())?;
            stream.flush()
        }
        Fault::Silent => {
            // The request is already read. Hold the socket with no response so
            // a pause has to win against the header wait, not a body wait.
            thread::sleep(Duration::from_secs(30));
            Ok(())
        }
        Fault::Stall => {
            let head = headers(200, etag, bytes.len(), None);
            stream.write_all(head.as_bytes())?;
            stream.flush()?;
            // Stay open and silent. The client's read deadline must notice.
            thread::sleep(Duration::from_secs(30));
            Ok(())
        }
        Fault::Cut { bytes: n } => {
            let (status, range, slice) = slice_of(bytes, start);
            let send = &slice[..(*n).min(slice.len())];
            stream.write_all(headers(status, etag, send.len(), range.as_deref()).as_bytes())?;
            stream.write_all(send)?;
            stream.flush()?;
            let _ = stream.shutdown(std::net::Shutdown::Both);
            Ok(())
        }
        Fault::Drip { gap, chunk } => {
            let (status, range, slice) = slice_of(bytes, start);
            stream.write_all(headers(status, etag, slice.len(), range.as_deref()).as_bytes())?;
            stream.flush()?;
            for piece in slice.chunks((*chunk).max(1)) {
                stream.write_all(piece)?;
                stream.flush()?;
                thread::sleep(*gap);
            }
            Ok(())
        }
        Fault::Honest => {
            let (status, range, slice) = slice_of(bytes, start);
            stream.write_all(headers(status, etag, slice.len(), range.as_deref()).as_bytes())?;
            stream.write_all(slice)?;
            stream.flush()
        }
    }
}

fn slice_of(bytes: &[u8], start: usize) -> (u16, Option<String>, &[u8]) {
    if start > 0 && start < bytes.len() {
        let header = format!("bytes {start}-{}/{}", bytes.len() - 1, bytes.len());
        (206, Some(header), &bytes[start..])
    } else {
        (200, None, bytes)
    }
}

fn headers(status: u16, etag: &str, len: usize, range: Option<&str>) -> String {
    let reason = if status == 206 { "Partial Content" } else { "OK" };
    let mut head =
        format!("HTTP/1.1 {status} {reason}\r\nContent-Length: {len}\r\nETag: {etag}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n");
    if let Some(range) = range {
        head.push_str(&format!("Content-Range: {range}\r\n"));
    }
    head.push_str("\r\n");
    head
}

fn stage(root: &Path, name: &str) -> PathBuf {
    root.join(WORK_DIR_NAME).join("staging").join(name)
}

struct Seen {
    offsets: Mutex<Vec<u64>>,
    retrying: Mutex<Vec<bool>>,
    over: AtomicU64,
}

fn watch(updater: &Updater, total: u64, parallel: usize) -> (tokio::task::JoinHandle<()>, Arc<Seen>) {
    let handle = updater.handle();
    let seen = Arc::new(Seen { offsets: Mutex::new(Vec::new()), retrying: Mutex::new(Vec::new()), over: AtomicU64::new(0) });
    let shared = Arc::clone(&seen);
    let task = tokio::spawn(async move {
        let mut last_network = 0_u64;
        loop {
            let snap = handle.transfer_progress();
            if snap.files.len() > parallel {
                shared.over.fetch_add(1, Ordering::Relaxed);
            }
            if snap.network_bytes < last_network {
                shared.over.fetch_add(1, Ordering::Relaxed);
            }
            last_network = snap.network_bytes;
            for file in &snap.files {
                if file.bytes_received > file.bytes_total || file.bytes_total != total {
                    shared.over.fetch_add(1, Ordering::Relaxed);
                }
                shared.offsets.lock().unwrap().push(file.bytes_received);
                shared.retrying.lock().unwrap().push(file.retrying);
            }
            if snap.progress.phase == SessionPhase::Idle && snap.files.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    });
    (task, seen)
}

#[tokio::test]
async fn repeated_cuts_past_five_attempts_finish_in_one_run() {
    let root = temp_root();
    let bytes = body();
    let origin = serve(bytes.clone(), Script { steps: std::iter::repeat_n(Fault::Cut { bytes: 512 }, 8).collect(), rest: Fault::Honest });
    let updater = open_parallel(&root);
    updater.prepare(manifest("cut", vec![file("cut.bin", &bytes, Some(format!("http://{}/cut.bin", origin.addr)))]), None).unwrap();
    let (watch, seen) = watch(&updater, bytes.len() as u64, 2);
    let started = Instant::now();
    let phase = updater.run().await.expect("progressing cuts must finish");
    assert_eq!(phase, SessionPhase::Idle);
    assert!(started.elapsed() < Duration::from_secs(8), "a few kilobytes with short cuts took {:?}", started.elapsed());
    assert!(origin.hits.load(Ordering::SeqCst) > 5, "expected more than five connections, got {}", origin.hits.load(Ordering::SeqCst));
    assert_eq!(read(&root.join("cut.bin")), bytes);
    watch.abort();
    assert_eq!(seen.over.load(Ordering::Relaxed), 0);
    let offsets = seen.offsets.lock().unwrap().clone();
    assert!(offsets.windows(2).any(|pair| pair[1] > pair[0]), "per-file offset never advanced: {offsets:?}");
    assert!(seen.retrying.lock().unwrap().iter().any(|flag| *flag), "retrying was never reported during the cuts");
    assert!(updater.handle().transfer_progress().files.is_empty());
}

#[tokio::test]
async fn a_slow_moving_body_is_not_cut_by_a_total_deadline() {
    let root = temp_root();
    let bytes = body();
    // 256-byte slices, 30ms apart: under a second of continuous progress.
    // The scaled read-idle deadline is 100ms, so each gap survives it and a
    // whole-request deadline of that same 100ms would cut the body off.
    let origin =
        serve(bytes.clone(), Script { steps: vec![Fault::Drip { gap: Duration::from_millis(30), chunk: 256 }], rest: Fault::Honest });
    let updater = open_parallel(&root);
    updater.prepare(manifest("drip", vec![file("drip.bin", &bytes, Some(format!("http://{}/drip.bin", origin.addr)))]), None).unwrap();
    let started = Instant::now();
    let phase = updater.run().await.expect("a moving body must complete");
    let elapsed = started.elapsed();
    assert_eq!(phase, SessionPhase::Idle);
    assert!(elapsed > Duration::from_millis(200), "drip was not slower than the 100ms deadline: {elapsed:?}");
    assert!(elapsed < Duration::from_secs(8), "drip stalled: {elapsed:?}");
    assert_eq!(origin.hits.load(Ordering::SeqCst), 1, "a moving body was restarted");
    assert_eq!(read(&root.join("drip.bin")), bytes);
}

#[tokio::test]
async fn a_silent_body_fails_fast_and_stays_resumable() {
    let root = temp_root();
    let bytes = body();
    let origin = serve(bytes.clone(), Script { steps: vec![], rest: Fault::Stall });
    let updater = open_parallel(&root);
    updater.prepare(manifest("stall", vec![file("stall.bin", &bytes, Some(format!("http://{}/stall.bin", origin.addr)))]), None).unwrap();
    let started = Instant::now();
    let err = tokio::time::timeout(Duration::from_secs(60), updater.run())
        .await
        .expect("a silent body was retried forever")
        .expect_err("a silent body must fail");
    let elapsed = started.elapsed();
    assert!(err.is_resumable(), "{err}");
    // Two per-file budgets: the session requeues a file that spent its first
    // one. Each budget is five scaled idle deadlines plus backoff.
    assert!(elapsed < Duration::from_secs(25), "stall was not bounded by the scaled idle deadline: {elapsed:?}");
    assert!(origin.hits.load(Ordering::SeqCst) < 30, "stall hammered the origin: {}", origin.hits.load(Ordering::SeqCst));
    assert!(!root.join("stall.bin").exists());
    let partial = read(&stage(&root, "stall.bin"));
    assert!(partial.is_empty() || partial.len() < bytes.len());
}

#[tokio::test]
async fn a_retry_after_past_the_cap_stops_without_a_second_request() {
    let root = temp_root();
    let bytes = body();
    let origin =
        serve(bytes.clone(), Script { steps: vec![Fault::Status { code: 503, retry_after: Some("3600".into()) }], rest: Fault::Honest });
    let updater = open_parallel(&root);
    updater
        .prepare(
            manifest("later", vec![super::file_without_etag("later.bin", &bytes, Some(format!("http://{}/later.bin", origin.addr)))]),
            None,
        )
        .unwrap();
    let started = Instant::now();
    let err = updater.run().await.expect_err("a one hour wait must stop");
    assert!(err.is_resumable(), "{err}");
    assert!(started.elapsed() < Duration::from_secs(4), "Retry-After: 3600 was waited out: {:?}", started.elapsed());
    assert_eq!(origin.hits.load(Ordering::SeqCst), 1, "the client retried before the server allowed it");
    assert!(!root.join("later.bin").exists());
    let journal = std::fs::read_to_string(root.join(WORK_DIR_NAME).join("journal.json")).unwrap_or_default();
    assert!(!journal.contains("error-page"), "an error-page etag was persisted: {journal}");
}

#[tokio::test]
async fn a_short_retry_after_is_waited_then_the_file_finishes() {
    let root = temp_root();
    let bytes = body();
    let origin =
        serve(bytes.clone(), Script { steps: vec![Fault::Status { code: 429, retry_after: Some("1".into()) }], rest: Fault::Honest });
    let updater = open_parallel(&root);
    updater
        .prepare(
            manifest("soon", vec![super::file_without_etag("soon.bin", &bytes, Some(format!("http://{}/soon.bin", origin.addr)))]),
            None,
        )
        .unwrap();
    let (watch, seen) = watch(&updater, bytes.len() as u64, 2);
    let started = Instant::now();
    let phase = updater.run().await.expect("a one second wait must finish");
    let elapsed = started.elapsed();
    assert_eq!(phase, SessionPhase::Idle);
    // The requested second is scaled to 20ms. The loopback server then holds
    // the finished connection, so the run is longer than the wait; it must
    // still be far under the unscaled second and must have retried exactly once.
    assert_eq!(origin.hits.load(Ordering::SeqCst), 2);
    assert!(elapsed < Duration::from_secs(2), "the short wait was not scaled: {elapsed:?}");
    assert_eq!(read(&root.join("soon.bin")), bytes);
    watch.abort();
    assert!(seen.retrying.lock().unwrap().iter().any(|flag| *flag));
    assert!(updater.handle().transfer_progress().files.is_empty());
}

#[tokio::test]
async fn an_http_date_retry_after_stops_instead_of_retrying_at_once() {
    let root = temp_root();
    let bytes = body();
    let origin = serve(
        bytes.clone(),
        Script { steps: vec![Fault::Status { code: 503, retry_after: Some("Wed, 21 Oct 2026 07:28:00 GMT".into()) }], rest: Fault::Honest },
    );
    let updater = open_parallel(&root);
    updater.prepare(manifest("dated", vec![file("dated.bin", &bytes, Some(format!("http://{}/dated.bin", origin.addr)))]), None).unwrap();
    let err = updater.run().await.expect_err("an HTTP-date wait must stop");
    assert!(err.is_resumable(), "{err}");
    assert_eq!(origin.hits.load(Ordering::SeqCst), 1);
    assert!(!root.join("dated.bin").exists());
}

#[tokio::test]
async fn pause_during_a_silent_header_returns_promptly() {
    let root = temp_root();
    let bytes = body();
    let origin = serve(bytes.clone(), Script { steps: vec![], rest: Fault::Silent });
    let updater = open_parallel(&root);
    updater
        .prepare(manifest("headers", vec![file("headers.bin", &bytes, Some(format!("http://{}/headers.bin", origin.addr)))]), None)
        .unwrap();
    let handle = updater.handle();
    let pauser = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        handle.pause();
    });
    let started = Instant::now();
    // The scaled header budget is 800ms. Pause is latched at 150ms, so a wait
    // that only ends at the header deadline misses this bound.
    let phase = tokio::time::timeout(Duration::from_millis(400), updater.run())
        .await
        .expect("pause lost to the header deadline")
        .expect("pause is not a download error");
    let _ = pauser.await;
    assert_eq!(phase, SessionPhase::Paused);
    assert!(started.elapsed() < Duration::from_millis(400));
    assert!(!root.join("headers.bin").exists());
}

#[tokio::test]
async fn an_error_status_etag_is_not_learned_before_the_real_object() {
    let root = temp_root();
    let bytes = body();
    let origin = serve(bytes.clone(), Script { steps: vec![Fault::Status { code: 502, retry_after: None }], rest: Fault::Honest });
    let updater = open_parallel(&root);
    updater
        .prepare(
            manifest("badgw", vec![super::file_without_etag("badgw.bin", &bytes, Some(format!("http://{}/badgw.bin", origin.addr)))]),
            None,
        )
        .unwrap();
    let phase = updater.run().await.expect("a 502 must not poison the object");
    assert_eq!(phase, SessionPhase::Idle);
    assert_eq!(read(&root.join("badgw.bin")), bytes);
    let journal = std::fs::read_to_string(root.join(WORK_DIR_NAME).join("journal.json")).unwrap_or_default();
    assert!(!journal.contains("error-page"), "the 502 etag was kept: {journal}");
    assert!(updater.handle().transfer_progress().files.is_empty());
}

#[tokio::test]
async fn pause_during_a_stall_returns_without_waiting_out_the_deadline() {
    let root = temp_root();
    let bytes = body();
    let origin = serve(bytes.clone(), Script { steps: vec![], rest: Fault::Stall });
    let updater = open_parallel(&root);
    updater.prepare(manifest("pause", vec![file("pause.bin", &bytes, Some(format!("http://{}/pause.bin", origin.addr)))]), None).unwrap();
    let handle = updater.handle();
    let pauser = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        handle.pause();
    });
    let started = Instant::now();
    let phase = updater.run().await.expect("pause must win over a stall");
    let elapsed = started.elapsed();
    let _ = pauser.await;
    assert_eq!(phase, SessionPhase::Paused);
    assert!(elapsed < Duration::from_secs(3), "pause waited out the stall deadline: {elapsed:?}");
    assert!(!root.join("pause.bin").exists());
}

#[tokio::test]
async fn a_persistent_outage_stops_and_keeps_the_partial() {
    let root = temp_root();
    let bytes = body();
    let origin =
        serve(bytes.clone(), Script { steps: vec![Fault::Cut { bytes: 1024 }], rest: Fault::Status { code: 503, retry_after: None } });
    let updater = open_parallel(&root);
    updater.prepare(manifest("out", vec![file("out.bin", &bytes, Some(format!("http://{}/out.bin", origin.addr)))]), None).unwrap();
    let started = Instant::now();
    let err = updater.run().await.expect_err("an endless 503 must stop");
    assert!(err.is_resumable(), "{err}");
    // Two per-file budgets and the requeue between them, under 12s: a 503 is
    // a server answer, never an outage, so no session-wide wait applies.
    assert!(started.elapsed() < Duration::from_secs(25), "outage did not give up: {:?}", started.elapsed());
    let partial = read(&stage(&root, "out.bin"));
    assert!(partial.len() >= 1024 && partial.len() < bytes.len(), "partial {}", partial.len());
    assert_eq!(&partial, &bytes[..partial.len()]);
    assert!(!root.join("out.bin").exists());
    let hits = origin.hits.load(Ordering::SeqCst);
    assert!(hits < 40, "outage kept hammering: {hits}");
    let ranges = origin.ranges.lock().unwrap().clone();
    assert!(ranges.iter().any(|start| *start >= 1024), "resume never continued from the partial: {ranges:?}");
}

/// Holds the first `silence` connections open without a status line, then
/// answers honestly. The listener is bound once, so one plan URL survives the
/// silence.
fn serve_outage(bytes: Vec<u8>, silence: usize) -> Origin {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicU64::new(0));
    let ranges = Arc::new(Mutex::new(Vec::new()));
    let hits_t = Arc::clone(&hits);
    let ranges_t = Arc::clone(&ranges);
    thread::spawn(move || {
        let mut quiet = 0_usize;
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_nodelay(true);
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = [0_u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            if n == 0 {
                continue;
            }
            let req = String::from_utf8_lossy(&buf[..n]);
            let start = req
                .lines()
                .find_map(|line| {
                    let lower = line.to_ascii_lowercase();
                    let rest = lower.strip_prefix("range:")?.trim().strip_prefix("bytes=")?;
                    rest.split('-').next()?.parse::<u64>().ok()
                })
                .unwrap_or(0);
            ranges_t.lock().unwrap().push(start);
            hits_t.fetch_add(1, Ordering::SeqCst);
            if quiet < silence {
                quiet += 1;
                // No bytes at all, held past the scaled header budget.
                thread::sleep(Duration::from_secs(2));
                continue;
            }
            let _ = answer(&mut stream, &bytes, start, &Fault::Honest);
        }
    });
    Origin { addr: format!("127.0.0.1:{}", addr.port()), hits, ranges }
}

#[tokio::test]
async fn a_silent_origin_shorter_than_the_session_budget_finishes_in_one_run() {
    let root = temp_root();
    let bytes = body();
    let origin = serve_outage(bytes.clone(), 3);
    let updater = open_parallel(&root);
    updater.prepare(manifest("blip", vec![file("blip.bin", &bytes, Some(format!("http://{}/blip.bin", origin.addr)))]), None).unwrap();
    let started = Instant::now();
    let phase = tokio::time::timeout(Duration::from_secs(60), updater.run())
        .await
        .expect("a header blip must not outlive the session")
        .expect("a header blip must not fail the session");
    assert_eq!(phase, SessionPhase::Idle);
    assert!(started.elapsed() < Duration::from_secs(60), "{:?}", started.elapsed());
    assert!(origin.hits.load(Ordering::SeqCst) >= 2);
    assert_eq!(read(&root.join("blip.bin")), bytes);
}

#[tokio::test]
async fn a_file_that_exhausts_its_budget_is_requeued_once_and_finishes() {
    let root = temp_root();
    let bytes = body();
    // Five 503s exhaust the per-file stall budget. The session must put the
    // file back and finish rather than failing the whole run.
    let origin = serve(
        bytes.clone(),
        Script { steps: std::iter::repeat_n(Fault::Status { code: 503, retry_after: None }, 5).collect(), rest: Fault::Honest },
    );
    let updater = open_parallel(&root);
    updater
        .prepare(manifest("requeue", vec![file("requeue.bin", &bytes, Some(format!("http://{}/requeue.bin", origin.addr)))]), None)
        .unwrap();
    let started = Instant::now();
    let phase = updater.run().await.expect("one budget of 503s must be requeued");
    assert_eq!(phase, SessionPhase::Idle);
    assert!(started.elapsed() < Duration::from_secs(20), "{:?}", started.elapsed());
    assert!(origin.hits.load(Ordering::SeqCst) > 5);
    assert_eq!(read(&root.join("requeue.bin")), bytes);
}

#[tokio::test]
async fn a_file_that_fails_twice_stops_the_session_as_a_resumable_error() {
    let root = temp_root();
    let bytes = body();
    // Enough 503s for two whole budgets. Requeueing is a second chance, not
    // an infinite one.
    let origin = serve(
        bytes.clone(),
        Script { steps: std::iter::repeat_n(Fault::Status { code: 503, retry_after: None }, 12).collect(), rest: Fault::Honest },
    );
    let updater = open_parallel(&root);
    updater.prepare(manifest("twice", vec![file("twice.bin", &bytes, Some(format!("http://{}/twice.bin", origin.addr)))]), None).unwrap();
    let err = updater.run().await.expect_err("a repeated budget must stop");
    assert!(err.is_resumable(), "{err}");
    assert!(!root.join("twice.bin").exists());
}

#[tokio::test]
async fn a_permanent_404_still_stops_on_the_first_verified_url() {
    let root = temp_root();
    let bytes = body();
    let origin = serve(bytes.clone(), Script { steps: vec![], rest: Fault::Status { code: 404, retry_after: None } });
    let updater = open_parallel(&root);
    updater.prepare(manifest("gone", vec![file("gone.bin", &bytes, Some(format!("http://{}/gone.bin", origin.addr)))]), None).unwrap();
    let err = updater.run().await.expect_err("a 404 is permanent");
    assert!(err.is_resumable(), "{err}");
    assert_eq!(origin.hits.load(Ordering::SeqCst), 1, "a 404 was retried");
}

#[tokio::test]
async fn a_dripping_connection_below_the_floor_is_cut_and_reconnected() {
    let root = temp_root();
    let bytes = body();
    // 64-byte slices every 60ms: ~1 KiB/s, under the 4 KiB/s floor. The
    // scaled low-rate window is 100ms, so the first window trips it. Each cut
    // keeps the bytes it staged, so the next connection continues from there.
    let origin =
        serve(bytes.clone(), Script { steps: vec![Fault::Drip { gap: Duration::from_millis(60), chunk: 64 }], rest: Fault::Honest });
    let updater = open_parallel(&root);
    updater.prepare(manifest("slow", vec![file("slow.bin", &bytes, Some(format!("http://{}/slow.bin", origin.addr)))]), None).unwrap();
    let started = Instant::now();
    let phase = updater.run().await.expect("a wedged connection must be replaced");
    assert_eq!(phase, SessionPhase::Idle);
    assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
    assert_eq!(read(&root.join("slow.bin")), bytes);
    let ranges = origin.ranges.lock().unwrap().clone();
    assert!(ranges.iter().any(|start| *start > 0), "the cut connection was not resumed: {ranges:?}");
}

#[tokio::test]
async fn a_body_above_the_floor_is_not_cut() {
    let root = temp_root();
    let bytes = body();
    // 2 KiB every 40ms is ~50 KiB/s, above the 4 KiB/s floor. No reconnect:
    // the watchdog must sit far below a real server and must not treat
    // steady progress as a wedge.
    let origin =
        serve(bytes.clone(), Script { steps: vec![Fault::Drip { gap: Duration::from_millis(40), chunk: 2048 }], rest: Fault::Honest });
    let updater = open_parallel(&root);
    updater
        .prepare(manifest("steady", vec![file("steady.bin", &bytes, Some(format!("http://{}/steady.bin", origin.addr)))]), None)
        .unwrap();
    let phase = updater.run().await.expect("a body above the floor must not be cut");
    assert_eq!(phase, SessionPhase::Idle);
    assert_eq!(origin.hits.load(Ordering::SeqCst), 1, "a steady body was reconnected");
    assert_eq!(read(&root.join("steady.bin")), bytes);
}

#[tokio::test]
async fn a_refused_connection_is_waited_as_an_outage_and_stays_resumable() {
    let root = temp_root();
    let bytes = body();
    // A bound-then-closed port refuses at once, which is the outage class:
    // nothing answered at all. The session must hold its one bounded wait
    // sequence, labelled as network trouble, and leave the file resumable
    // rather than burning every file's budget at connect speed.
    let port = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("127.0.0.1:{}", port.local_addr().unwrap().port());
    drop(port);
    let updater = open_parallel(&root);
    updater.prepare(manifest("refused", vec![file("refused.bin", &bytes, Some(format!("http://{addr}/refused.bin")))]), None).unwrap();
    let started = Instant::now();
    let err = tokio::time::timeout(Duration::from_secs(30), updater.run())
        .await
        .expect("a dead port must end on the session budget")
        .expect_err("a port that stays closed is a real failure");
    let elapsed = started.elapsed();
    assert!(err.is_resumable(), "{err}");
    assert!(err.to_string().contains("unreachable"), "{err}");
    // A 1/50 pace unit scales the 211 s outage sequence to about 4.2 s.
    assert!(elapsed >= Duration::from_secs(2), "the outage was not waited: {elapsed:?}");
    assert!(elapsed < Duration::from_secs(15), "the session wait was unbounded: {elapsed:?}");
    assert!(!root.join("refused.bin").exists());
}
