//! One-file resume with strict range checks.
//!
//! A partial is appended only when the response is `206` and its `Content-Range`
//! starts at the byte already held and ends at the expected size. `200` means
//! the server ignored `Range`: the partial stays, and the whole object is written
//! on a separate replacement handle, then swapped only after it hashes.
//!
//! Every create, open, truncate, delete, and rename uses a checked handle
//! relative to a parent the caller already pinned. `dest` is a non-secret label.

use std::ffi::OsStr;
use std::io::SeekFrom;
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use reqwest::header::{
    ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_RANGE, ETAG, HeaderMap, HeaderValue, IF_RANGE, RANGE, RETRY_AFTER,
};
use reqwest::{Client, StatusCode};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use url::Url;

use crate::error::UpdateError;
use crate::hash::StreamingHash;
use crate::manifest::Sha256;
use crate::pin::{self, OpenedFile};
use crate::policy::DownloadPolicy;

/// Consecutive attempts that commit no new staged byte. Attempts that do
/// commit one start this count again. A replacement of a retained partial is
/// not new progress: the partial was already durable.
const STALL_ATTEMPTS: u32 = 5;
/// Hard ceiling for one `fetch_verified` call, so a server that drips one
/// fresh byte forever still stops. The session requeues a file that reaches
/// it, so this bounds one visit, not the whole install.
const ABSOLUTE_ATTEMPTS: u32 = 64;
/// Complete bodies that failed the digest before the class is treated as
/// real. Two fresh downloads of the same bytes must both fail to hash; one
/// corrupted transfer is retried in place.
const HASH_RETRIES: u32 = 1;
const CHUNK_LIMIT: usize = 64 * 1024;
/// A connection delivering less than `MIN_RATE` over `LOW_RATE_WINDOW` is
/// aborted and reconnected. A single connection to the SDVX CDN measured about
/// 4.6 MB/s, so 4 KiB/s sits more than a thousand times below it: under any
/// real server speed limit, and above only a socket that has stopped making
/// progress in any useful sense. 4 KiB/s over 10 s is a 40 KiB window.
const MIN_RATE: u64 = 4 * 1024;
const LOW_RATE_WINDOW: Duration = Duration::from_secs(10);

/// Production deadlines. A test scales all three by the same unit.
const CONNECT_BUDGET: Duration = Duration::from_secs(10);
const HEADER_BUDGET: Duration = Duration::from_secs(30);
const READ_IDLE: Duration = Duration::from_secs(15);
/// A numeric `Retry-After` above this is not waited and not shortened.
const RETRY_AFTER_CAP: Duration = Duration::from_secs(30);
const BACKOFF_FLOOR: Duration = Duration::from_millis(500);
const BACKOFF_CAP: Duration = Duration::from_secs(8);

pub(crate) struct Transfer {
    pub received: u64,
    pub etag: Option<String>,
}

pub(crate) struct Fetch<'a> {
    pub client: &'a Client,
    pub policy: &'a DownloadPolicy,
    pub url: &'a str,
    pub expected: &'a Sha256,
    pub expected_size: u64,
    /// Index validator. Often `None`. Immutable for the life of this fetch;
    /// a strong response ETag learned before the first stage byte is kept
    /// locally and sent as `If-Range` on later attempts.
    pub expected_etag: Option<&'a str>,
    /// Non-secret label used only in errors. Not a path this module opens.
    pub dest: &'a str,
    pub pause: &'a AtomicBool,
    /// Accepted body bytes just written. Deltas, not a running total.
    pub bytes_progress: &'a (dyn Fn(u64) + Sync),
    /// Absolute useful bytes for this file. The staged prefix, or
    /// `max(prefix, replacement)` while a range-ignored body is written.
    /// Not the sum of the two. Zero after a hash miss deletes the stage.
    pub file_progress: &'a (dyn Fn(u64) + Sync),
    /// True only while waiting out a retry. False as each attempt starts.
    pub retrying: &'a (dyn Fn(bool) + Sync),
    /// Strong response ETag, learned before the first original-stage byte.
    /// Never invented. A failure is returned before any body byte is written.
    pub learned_etag: &'a (dyn Fn(&str) -> Result<(), UpdateError> + Sync),
    /// Pinned parent of the original stage file.
    pub parent: &'a OwnedHandle,
    /// Single path segment of the original stage file.
    pub name: &'a OsStr,
    /// Pinned parent of the range-ignored replacement (`work/replacements`).
    pub replacement_parent: &'a OwnedHandle,
    /// Single path segment of the replacement (`SHA256(relative).part`).
    pub replacement_name: &'a OsStr,
    /// One unit of connect, header, read-idle, retry-after, and low-rate time.
    /// Production is one millisecond. Tests pass a shorter unit.
    pub(crate) pace: Duration,
    /// Session-level outage state.
    pub(crate) signal: NetSignal<'a>,
}

/// Session-level outage state. A file sets it only when no server answered at
/// all (connect, DNS, TLS, reset before the status line); a dropped body or a
/// page of 5xx is that file's own problem and must not freeze every peer.
#[derive(Clone, Copy)]
pub(crate) struct NetSignal<'a> {
    pub failed: &'a AtomicBool,
}

pub(crate) fn pace_of(unit: Duration, budget: Duration) -> Duration {
    budget.mul_f64(unit.as_secs_f64() * 1_000.0)
}

/// How a completed `fetch_verified` call failed, for the session supervisor.
/// The `UpdateError` is carried unchanged so no caller sees a reworded error.
pub(crate) enum Failed {
    /// The per-file budget is spent. The file may be tried once more later.
    Transient(UpdateError),
    /// The session was already waiting for the network; this file did not
    /// spend any of its own budget.
    /// No error body: the session requeues the file and an error here would
    /// only repeat the one it is already waiting on.
    Offline,
    /// Retrying the same bytes cannot fix this. Stop the session.
    Permanent(UpdateError),
}

/// Result of one download future. Success carries nothing; failure keeps its
/// class and the error so the session can requeue without re-reading a label.
pub(crate) struct FileDone {
    pub failed: Option<Failed>,
}

pub(crate) async fn fetch_verified(fetch: Fetch<'_>) -> Result<Transfer, Failed> {
    let mut last = None;
    // `expected_etag` is immutable and often None. A strong response ETag
    // persisted before the first stage byte is reused as If-Range on retry.
    let mut learned: Option<String> = None;
    let mut durable = stage_len(&fetch).unwrap_or(0).min(fetch.expected_size);
    let mut stalls = 0_u32;
    // Body attempts that ran out of a transient condition this call. The
    // attempt loop itself keeps going; the streak is only the final verdict.
    let mut hash_retries = HASH_RETRIES;
    for _attempt in 0..ABSOLUTE_ATTEMPTS {
        // A session that was told the network is down must not spend this
        // file's attempts while it waits. Return a resumable failure and let
        // the session requeue the file once its own bounded wait is over.
        if fetch.signal.failed.load(Ordering::SeqCst) {
            return Err(Failed::Offline);
        }
        (fetch.retrying)(false);
        if fetch.pause.load(Ordering::SeqCst) {
            return Err(Failed::Permanent(UpdateError::download(fetch.dest, "paused")));
        }
        match fetch_once(&fetch, learned.as_deref()).await {
            Ok((transfer, _fresh)) => return Ok(transfer),
            Err(Step::Retry { etag, err, wait, progress }) => {
                if learned.is_none()
                    && let Some(etag) = etag.filter(|etag| is_strong(etag))
                {
                    learned = Some(etag);
                }
                // Only a larger original stage resets the budget. Bytes written
                // into a not-yet-swapped replacement do not: the partial those
                // bytes might replace was already durable.
                let held = stage_len(&fetch).unwrap_or(durable).min(fetch.expected_size);
                if held > durable {
                    durable = held;
                    stalls = 0;
                } else if !progress {
                    stalls = stalls.saturating_add(1);
                }
                last = Some(err);
                if stalls >= STALL_ATTEMPTS {
                    break;
                }
                (fetch.retrying)(true);
                // The delay follows the no-progress streak. New staged bytes
                // start it again, so a long file is not stuck at the cap.
                // A Retry-After is the only wait this file owes the server.
                // The ordinary backoff is cut short while the session is
                // offline, and an offline file must not sit here: the session
                // requeues it before the next probe starts.
                let pause = match wait {
                    Some(wait) => wait,
                    None if fetch.signal.failed.load(Ordering::SeqCst) => Duration::ZERO,
                    None => backoff(stalls),
                };
                if !pause.is_zero() && !sleep_pausable(&fetch, pace(&fetch, pause)).await {
                    return Err(Failed::Permanent(UpdateError::download(fetch.dest, "paused")));
                }
                if fetch.signal.failed.load(Ordering::SeqCst) {
                    return Err(Failed::Offline);
                }
            }
            Err(Step::Stop(err)) if matches!(err, UpdateError::Staging { .. }) => {
                // A complete body that does not hash. One repeat can be a
                // corrupted transfer; two is the source itself.
                if hash_retries == 0 {
                    return Err(Failed::Permanent(err));
                }
                hash_retries -= 1;
                last = Some(err);
                (fetch.retrying)(true);
                if !sleep_pausable(&fetch, pace(&fetch, backoff(stalls))).await {
                    return Err(Failed::Permanent(UpdateError::download(fetch.dest, "paused")));
                }
            }
            Err(Step::Stop(err)) => return Err(Failed::Permanent(err)),
        }
    }
    Err(Failed::Transient(last.unwrap_or_else(|| UpdateError::download(fetch.url, "download failed"))))
}

fn backoff(attempt: u32) -> Duration {
    BACKOFF_FLOOR.saturating_mul(1_u32 << attempt.min(3)).min(BACKOFF_CAP)
}

/// Aborts a body that moves slower than `MIN_RATE` for `LOW_RATE_WINDOW`.
/// The idle deadline already catches silence; this catches a connection that
/// keeps trickling a few bytes and would otherwise never trip it. Windows
/// restart when any window delivers the floor, so steady progress at the floor
/// or above is never cut. This is not a throughput goal: the abort exists to
/// drop a wedged socket, not to chase a faster one.
struct RateWatch {
    window: Duration,
    floor: u64,
    since: Instant,
    from: u64,
    label: String,
}

impl RateWatch {
    fn start(fetch: &Fetch<'_>, from: u64) -> Self {
        Self { window: pace(fetch, LOW_RATE_WINDOW), floor: MIN_RATE, since: Instant::now(), from, label: fetch.dest.to_string() }
    }

    /// `total` is bytes of this object held after the chunk was accepted.
    fn observe(&mut self, total: u64) -> Option<UpdateError> {
        let moved = total.saturating_sub(self.from);
        let elapsed = self.since.elapsed();
        if elapsed < self.window {
            return None;
        }
        // The floor is a rate, not a per-window count. A window that was
        // scaled down by `pace` still has to deliver its share of the rate.
        let needed = (self.floor as u128 * elapsed.as_millis() / 1000) as u64;
        if moved >= needed {
            self.from = total;
            self.since = Instant::now();
            return None;
        }
        Some(UpdateError::download(&self.label, format!("connection delivered {moved} bytes in {}ms; reconnecting", elapsed.as_millis())))
    }
}

/// Resolves when another file in this session failed without a response.
async fn session_offline(fetch: &Fetch<'_>) {
    loop {
        if fetch.signal.failed.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(pace(fetch, Duration::from_millis(250))).await;
    }
}

/// `pace` is one millisecond in production. Tests pass a fraction of that.
fn pace(fetch: &Fetch<'_>, budget: Duration) -> Duration {
    budget.mul_f64(fetch.pace.as_secs_f64() * 1_000.0)
}

async fn sleep_pausable(fetch: &Fetch<'_>, total: Duration) -> bool {
    let step = Duration::from_millis(20).min(fetch.pace.saturating_mul(20));
    let step = if step.is_zero() { total } else { step };
    let mut left = total;
    while !left.is_zero() {
        if fetch.pause.load(Ordering::SeqCst) {
            return false;
        }
        let slice = step.min(left);
        tokio::time::sleep(slice).await;
        left = left.saturating_sub(slice);
    }
    !fetch.pause.load(Ordering::SeqCst)
}

enum Step {
    /// Resumable failure. `etag` was a content validator, never an error page.
    /// `wait` is a bounded `Retry-After`. `progress` means this attempt wrote
    /// a new body byte somewhere; only the original stage resets the budget.
    Retry {
        etag: Option<String>,
        err: UpdateError,
        wait: Option<Duration>,
        progress: bool,
    },
    Stop(UpdateError),
}

async fn fetch_once(fetch: &Fetch<'_>, learned: Option<&str>) -> Result<(Transfer, Option<String>), Step> {
    let label = fetch.dest;
    let mut have = stage_len(fetch).map_err(Step::Stop)?;
    if have > fetch.expected_size {
        delete_stage(fetch).map_err(Step::Stop)?;
        have = 0;
    }
    (fetch.file_progress)(have);
    if have == fetch.expected_size && stage_exists(fetch).map_err(Step::Stop)? {
        return verify_existing(fetch).await.map(|transfer| (transfer, None)).map_err(Step::Stop);
    }

    let url = fetch.policy.check_url(fetch.url).map_err(Step::Stop)?;
    let response = send(fetch, &url, have, learned).await.map_err(|(err, transport)| {
        if fetch.pause.load(Ordering::SeqCst) {
            Step::Stop(UpdateError::download(label, "paused"))
        } else if transport {
            // Connect, DNS, TLS, or a reset before the status line: no
            // server answered. One such failure may mean the whole link is
            // down, so the session is allowed to wait instead of spending
            // every file's budget at once.
            fetch.signal.failed.store(true, Ordering::SeqCst);
            Step::Retry { etag: None, err, wait: None, progress: false }
        } else if matches!(err, UpdateError::Download { .. }) {
            Step::Retry { etag: None, err, wait: None, progress: false }
        } else {
            // Policy refusal or a local staging problem. Never classified
            // as network: the session must stop for these.
            Step::Stop(err)
        }
    })?;
    if response.status().is_redirection() {
        return Err(Step::Stop(UpdateError::download(label, "redirects are not followed")));
    }
    refuse_content_encoding(response.headers(), label).map_err(Step::Stop)?;
    consume(fetch, response, have, learned).await
}

fn refuse_content_encoding(headers: &HeaderMap, label: &str) -> Result<(), UpdateError> {
    if let Some(encoding) = headers.get(CONTENT_ENCODING) {
        let text = encoding.to_str().unwrap_or("");
        if !text.eq_ignore_ascii_case("identity") {
            return Err(UpdateError::download(label, format!("refusing Content-Encoding {text}")));
        }
    }
    Ok(())
}

fn is_strong(etag: &str) -> bool {
    let trimmed = etag.trim();
    !trimmed.is_empty() && !trimmed.starts_with("W/")
}

/// Index validator, else one learned on this session before any stage byte.
fn validator<'a>(fetch: &'a Fetch<'_>, learned: Option<&'a str>) -> Option<&'a str> {
    fetch.expected_etag.filter(|etag| is_strong(etag)).or_else(|| learned.filter(|etag| is_strong(etag)))
}

/// The response, or the error and whether it was a transport failure with no
/// server reached at all. Only that class feeds the session outage state: our
/// own header budget expiring on a silent server is this file's problem, not
/// proof the network is down.
async fn send(fetch: &Fetch<'_>, url: &Url, have: u64, learned: Option<&str>) -> Result<reqwest::Response, (UpdateError, bool)> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT_ENCODING, HeaderValue::from_static("identity"));
    if have > 0 {
        let value =
            HeaderValue::from_str(&format!("bytes={have}-")).map_err(|err| (UpdateError::download(fetch.dest, err.to_string()), false))?;
        headers.insert(RANGE, value);
        if let Some(etag) = validator(fetch, learned)
            && let Ok(value) = HeaderValue::from_str(etag)
        {
            headers.insert(IF_RANGE, value);
        }
    }
    // No whole-request deadline. Connect and headers are bounded; body silence
    // is bounded per read, inside the stream, so a slow moving body is not cut.
    // Pause is selected beside the request: a header hang must not wait out
    // the header budget before the latch is noticed.
    let connect = pace(fetch, CONNECT_BUDGET);
    let headers_budget = pace(fetch, HEADER_BUDGET);
    let request = fetch.client.get(url.clone()).headers(headers).send();
    tokio::pin!(request);
    let response = tokio::select! {
        response = &mut request => response,
        () = tokio::time::sleep(connect.saturating_add(headers_budget)) => {
            return Err((
                UpdateError::download(fetch.dest, "timed out waiting for response headers"),
                false,
            ));
        }
        () = pause_tick(fetch) => {
            return Err((UpdateError::download(fetch.dest, "paused"), false));
        }
        () = session_offline(fetch) => {
            // Some other file proved the network is down. Leave this partial
            // alone and let the session requeue it after a bounded wait.
            return Err((UpdateError::download(fetch.dest, "waiting for the network"), false));
        }
    };
    response.map_err(|err| (UpdateError::download(fetch.dest, err.to_string()), true))
}

async fn consume(
    fetch: &Fetch<'_>,
    response: reqwest::Response,
    have: u64,
    learned: Option<&str>,
) -> Result<(Transfer, Option<String>), Step> {
    let label = fetch.dest;
    let status = response.status();
    let headers = response.headers().clone();

    // Status first. Only 200 and 206 can carry a content validator. An error
    // page's ETag is never learned and its body is never written.
    if status != StatusCode::OK && status != StatusCode::PARTIAL_CONTENT {
        return Err(status_step(label, status, &headers));
    }

    let etag = strong_etag(&headers);
    // Persist a strong validator before the first original-stage byte. A failed
    // callback writes nothing. The same response is then consumed. A full
    // replacement is a different object until the swap, so its ETag is not
    // learned against the retained partial.
    let mut fresh = None;
    if have == 0
        && let Some(value) = etag.as_deref()
        && validator(fetch, learned).is_none()
    {
        (fetch.learned_etag)(value).map_err(Step::Stop)?;
        fresh = Some(value.to_string());
    }
    let remembered = fresh.as_deref().or(learned);
    let resume = |err: UpdateError| Step::Retry {
        etag: remembered.filter(|etag| is_strong(etag)).map(str::to_string),
        err,
        wait: None,
        progress: false,
    };

    if let Some(expected) = validator(fetch, remembered) {
        match &etag {
            Some(actual) if actual == expected => {}
            Some(_) if have > 0 => {
                return Err(resume(UpdateError::download(label, "etag changed; partial left in place")));
            }
            Some(_) => return Err(Step::Stop(UpdateError::download(label, "etag changed"))),
            None if have > 0 => {
                return Err(resume(UpdateError::download(label, "etag omitted on resume; partial left in place")));
            }
            None => {}
        }
    }

    if have > 0 && status == StatusCode::OK {
        return stream_full_replacement(fetch, response, have, etag).await.map(|transfer| (transfer, None)).map_err(resume);
    }
    if have > 0 && status != StatusCode::PARTIAL_CONTENT {
        return Err(resume(UpdateError::download(label, format!("resume rejected with status {status}; partial left in place"))));
    }
    if have == 0 && status != StatusCode::OK && status != StatusCode::PARTIAL_CONTENT {
        return Err(resume(UpdateError::download(label, format!("unexpected status {status}"))));
    }
    if have > 0 || status == StatusCode::PARTIAL_CONTENT {
        let range = headers
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| resume(UpdateError::download(label, "206 response missing Content-Range; partial left in place")))?;
        let (start, end_inclusive, complete) = parse_content_range(range)
            .ok_or_else(|| resume(UpdateError::download(label, format!("unusable Content-Range {range}; partial left in place"))))?;
        if start != have || complete != fetch.expected_size || end_inclusive.saturating_add(1) != complete {
            return Err(resume(UpdateError::download(
                label,
                format!("Content-Range {range} does not continue byte {have}; partial left in place"),
            )));
        }
    } else if let Some(len) = content_length(&headers)
        && len > fetch.expected_size
    {
        return Err(Step::Stop(UpdateError::download(label, format!("Content-Length {len} exceeds expected {}", fetch.expected_size))));
    }

    // A response was accepted, so the network was up at least once. This
    // file is not evidence of an outage; the session only reads the flag
    // after a completed per-file failure, so a later stall cannot be hidden.
    fetch.signal.failed.store(false, Ordering::SeqCst);
    match append_body(fetch, response, have, etag).await {
        Ok(transfer) => Ok((transfer, fresh)),
        Err(err) if err.is_resumable() => Err(Step::Retry {
            etag: remembered.filter(|etag| is_strong(etag)).map(str::to_string),
            err,
            wait: None,
            // A short body still committed the bytes it carried.
            progress: stage_len(fetch).unwrap_or(have) > have,
        }),
        Err(err) => Err(Step::Stop(err)),
    }
}

/// A missing header uses the ordinary backoff. A numeric value is waited as
/// asked, up to the cap. Past the cap, or an HTTP-date this crate does not
/// parse, the attempt stops instead of retrying early.
fn retry_after(headers: &HeaderMap) -> Result<Option<Duration>, ()> {
    let Some(value) = headers.get(RETRY_AFTER) else {
        return Ok(None);
    };
    let text = value.to_str().map_err(|_| ())?.trim();
    if text.is_empty() {
        return Err(());
    }
    let Ok(seconds) = text.parse::<u64>() else {
        return Err(());
    };
    let asked = Duration::from_secs(seconds);
    if asked > RETRY_AFTER_CAP {
        return Err(());
    }
    Ok(Some(asked))
}

fn status_step(label: &str, status: StatusCode, headers: &HeaderMap) -> Step {
    let transient = matches!(
        status,
        StatusCode::REQUEST_TIMEOUT
            | StatusCode::TOO_MANY_REQUESTS
            | StatusCode::INTERNAL_SERVER_ERROR
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    );
    if transient {
        return match retry_after(headers) {
            Ok(wait) => Step::Retry { etag: None, err: UpdateError::download(label, format!("status {status}")), wait, progress: false },
            Err(()) => Step::Stop(UpdateError::download(label, format!("status {status} asked for a wait this updater will not shorten"))),
        };
    }
    // Not transient. Stop this run without deleting the partial: the error is a
    // download error, so a later run can continue.
    Step::Stop(UpdateError::download(label, format!("unexpected status {status}; partial left in place")))
}

fn strong_etag(headers: &HeaderMap) -> Option<String> {
    headers.get(ETAG).and_then(|value| value.to_str().ok()).map(str::trim).filter(|value| is_strong(value)).map(str::to_string)
}

async fn append_body(fetch: &Fetch<'_>, response: reqwest::Response, have: u64, etag: Option<String>) -> Result<Transfer, UpdateError> {
    let label = fetch.dest;
    let owned = open_stage_write(fetch)?;
    let mut file = tokio_file(owned)?;
    file.seek(SeekFrom::Start(have)).await.map_err(|err| UpdateError::download(label, err.to_string()))?;
    // Same mutable handle. A second open would fail the read-only share mode.
    let mut hasher = hash_prefix(&mut file, have, label).await?;
    file.seek(SeekFrom::Start(have)).await.map_err(|err| UpdateError::download(label, err.to_string()))?;

    let mut stream = response.bytes_stream();
    let mut rate = RateWatch::start(fetch, have);
    loop {
        let next = tokio::select! {
            chunk = stream.next() => chunk,
            () = tokio::time::sleep(pace(fetch, READ_IDLE)) => {
                sync_file(&mut file).await;
                return Err(UpdateError::download(label, "stalled waiting for the next bytes"));
            }
            () = pause_tick(fetch) => {
                sync_file(&mut file).await;
                return Err(UpdateError::download(label, "paused"));
            }
        };
        let Some(chunk) = next else { break };
        if fetch.pause.load(Ordering::SeqCst) {
            sync_file(&mut file).await;
            return Err(UpdateError::download(label, "paused"));
        }
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(err) => {
                sync_file(&mut file).await;
                return Err(UpdateError::download(label, err.to_string()));
            }
        };
        if chunk.is_empty() {
            continue;
        }
        if hasher.bytes() + chunk.len() as u64 > fetch.expected_size {
            sync_file(&mut file).await;
            return Err(UpdateError::download(label, "server sent more bytes than expected; extra bytes were not written"));
        }
        if let Err(err) = file.write_all(&chunk).await {
            sync_file(&mut file).await;
            return Err(UpdateError::download(label, err.to_string()));
        }
        hasher.update(&chunk);
        (fetch.bytes_progress)(chunk.len() as u64);
        (fetch.file_progress)(hasher.bytes());
        if let Some(slow) = rate.observe(hasher.bytes()) {
            sync_file(&mut file).await;
            return Err(slow);
        }
        // Another file proved the network is down. This body keeps its bytes
        // and lets the session wait instead of spending its budget first.
        if fetch.signal.failed.load(Ordering::SeqCst) {
            sync_file(&mut file).await;
            return Err(UpdateError::download(label, "waiting for the network"));
        }
    }
    sync_file(&mut file).await;
    drop(file);
    let received = hasher.bytes();
    if received != fetch.expected_size {
        return Err(UpdateError::download(label, format!("received {received} of {} bytes", fetch.expected_size)));
    }
    let digest = hasher.finish();
    if !fetch.expected.matches(&digest) {
        delete_stage(fetch)?;
        (fetch.file_progress)(0);
        return Err(UpdateError::Staging {
            relative: label.to_string(),
            message: "downloaded bytes do not match the expected sha256".into(),
        });
    }
    Ok(Transfer { received, etag })
}

async fn stream_full_replacement(
    fetch: &Fetch<'_>,
    response: reqwest::Response,
    retained: u64,
    etag: Option<String>,
) -> Result<Transfer, UpdateError> {
    let label = fetch.dest;
    let replacement = open_replacement(fetch)?;
    set_len(&replacement, 0).map_err(|err| UpdateError::download(label, err.to_string()))?;
    let mut writer = tokio_file(replacement)?;
    let streamed = write_replacement(fetch, &mut writer, response, retained, label).await;
    // The tokio wrapper is closed before rename or delete. The original partial
    // was never opened, so a failed replacement cannot change it.
    let std_file = writer.into_std().await;
    if let Err(err) = streamed {
        drop(std_file);
        let _ = pin::delete_file(fetch.replacement_parent, fetch.replacement_name);
        (fetch.file_progress)(retained);
        return Err(err);
    }
    let replacement = OwnedHandle::from(std_file);
    pin::rename_into(&replacement, fetch.parent, fetch.name, true)?;
    drop(replacement);
    Ok(Transfer { received: fetch.expected_size, etag })
}

async fn write_replacement(
    fetch: &Fetch<'_>,
    writer: &mut tokio::fs::File,
    response: reqwest::Response,
    retained: u64,
    label: &str,
) -> Result<(), UpdateError> {
    let mut hasher = StreamingHash::new();
    let mut stream = response.bytes_stream();
    // A full replacement starts from byte 0 on the wire, whatever is retained.
    let mut rate = RateWatch::start(fetch, 0);
    loop {
        let next = tokio::select! {
            chunk = stream.next() => chunk,
            () = tokio::time::sleep(pace(fetch, READ_IDLE)) => {
                sync_file(writer).await;
                return Err(UpdateError::download(label, "stalled waiting for the next bytes"));
            }
            () = pause_tick(fetch) => {
                sync_file(writer).await;
                return Err(UpdateError::download(label, "paused"));
            }
        };
        let Some(chunk) = next else { break };
        if fetch.pause.load(Ordering::SeqCst) {
            sync_file(writer).await;
            return Err(UpdateError::download(label, "paused"));
        }
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(err) => {
                sync_file(writer).await;
                return Err(UpdateError::download(label, err.to_string()));
            }
        };
        if chunk.is_empty() {
            continue;
        }
        if hasher.bytes() + chunk.len() as u64 > fetch.expected_size {
            sync_file(writer).await;
            return Err(UpdateError::download(label, "full response exceeded expected size"));
        }
        if let Err(err) = writer.write_all(&chunk).await {
            sync_file(writer).await;
            return Err(UpdateError::download(label, err.to_string()));
        }
        hasher.update(&chunk);
        (fetch.bytes_progress)(chunk.len() as u64);
        (fetch.file_progress)(retained.max(hasher.bytes()));
        if let Some(slow) = rate.observe(hasher.bytes()) {
            sync_file(writer).await;
            return Err(slow);
        }
        if fetch.signal.failed.load(Ordering::SeqCst) {
            sync_file(writer).await;
            return Err(UpdateError::download(label, "waiting for the network"));
        }
    }
    sync_file(writer).await;
    if hasher.bytes() != fetch.expected_size || !fetch.expected.matches(&hasher.finish()) {
        return Err(UpdateError::Staging {
            relative: label.to_string(),
            message: "range-ignored response did not match; partial was left untouched".into(),
        });
    }
    Ok(())
}

async fn verify_existing(fetch: &Fetch<'_>) -> Result<Transfer, UpdateError> {
    let label = fetch.dest;
    let Some(file) = pin::open_file_read(fetch.parent, fetch.name)? else {
        return Err(UpdateError::download(label, "staged file disappeared before verification"));
    };
    let mut file = tokio_file(file.into_handle())?;
    let digest = crate::hash::hash_file(&mut file).await.map_err(|err| UpdateError::download(label, err.to_string()))?;
    drop(file);
    if !fetch.expected.matches(&digest) {
        delete_stage(fetch)?;
        (fetch.file_progress)(0);
        return Err(UpdateError::Staging {
            relative: label.to_string(),
            message: "staged file has the expected size but the wrong sha256".into(),
        });
    }
    Ok(Transfer { received: fetch.expected_size, etag: fetch.expected_etag.filter(|etag| is_strong(etag)).map(str::to_string) })
}

async fn hash_prefix(file: &mut tokio::fs::File, len: u64, label: &str) -> Result<StreamingHash, UpdateError> {
    file.seek(SeekFrom::Start(0)).await.map_err(|err| UpdateError::download(label, err.to_string()))?;
    let mut hasher = StreamingHash::new();
    let mut left = len;
    let mut buf = vec![0_u8; CHUNK_LIMIT];
    while left > 0 {
        let want = std::cmp::min(left as usize, buf.len());
        let n = file.read(&mut buf[..want]).await.map_err(|err| UpdateError::download(label, err.to_string()))?;
        if n == 0 {
            return Err(UpdateError::download(label, "partial file shrank while being hashed"));
        }
        hasher.update(&buf[..n]);
        left -= n as u64;
    }
    Ok(hasher)
}

fn content_length(headers: &HeaderMap) -> Option<u64> {
    headers.get(CONTENT_LENGTH).and_then(|value| value.to_str().ok()).and_then(|value| value.parse().ok())
}

/// `bytes <start>-<end>/<complete>`. `*` is not a usable complete length.
fn parse_content_range(value: &str) -> Option<(u64, u64, u64)> {
    let rest = value.trim().strip_prefix("bytes ")?;
    let (span, complete) = rest.split_once('/')?;
    if complete == "*" {
        return None;
    }
    let (start, end) = span.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()?, complete.parse().ok()?))
}

fn stage_len(fetch: &Fetch<'_>) -> Result<u64, UpdateError> {
    Ok(open_stage(fetch)?.map(|file| file.size()).unwrap_or(0))
}

fn stage_exists(fetch: &Fetch<'_>) -> Result<bool, UpdateError> {
    Ok(open_stage(fetch)?.is_some())
}

fn open_stage(fetch: &Fetch<'_>) -> Result<Option<OpenedFile>, UpdateError> {
    pin::open_file_read(fetch.parent, fetch.name)
}

fn open_stage_write(fetch: &Fetch<'_>) -> Result<OwnedHandle, UpdateError> {
    match pin::open_file_write(fetch.parent, fetch.name)? {
        Some(file) => Ok(file.into_handle()),
        None => pin::create_file(fetch.parent, fetch.name),
    }
}

fn open_replacement(fetch: &Fetch<'_>) -> Result<OwnedHandle, UpdateError> {
    match pin::open_file_write(fetch.replacement_parent, fetch.replacement_name)? {
        Some(file) => Ok(file.into_handle()),
        None => pin::create_file(fetch.replacement_parent, fetch.replacement_name),
    }
}

fn delete_stage(fetch: &Fetch<'_>) -> Result<(), UpdateError> {
    pin::delete_file(fetch.parent, fetch.name)
}

/// Truncate a handle that was already opened checked. Not `CREATE_ALWAYS`.
fn set_len(handle: &OwnedHandle, len: u64) -> std::io::Result<()> {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Storage::FileSystem::{FILE_END_OF_FILE_INFO, FileEndOfFileInfo, SetFileInformationByHandle};
    let info = FILE_END_OF_FILE_INFO { EndOfFile: len as i64 };
    let ok = unsafe {
        SetFileInformationByHandle(
            handle.as_raw_handle() as HANDLE,
            FileEndOfFileInfo,
            &info as *const FILE_END_OF_FILE_INFO as *const _,
            std::mem::size_of::<FILE_END_OF_FILE_INFO>() as u32,
        )
    };
    if ok == 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
}

fn tokio_file(handle: OwnedHandle) -> Result<tokio::fs::File, UpdateError> {
    Ok(tokio::fs::File::from_std(std::fs::File::from(handle)))
}

async fn sync_file(file: &mut tokio::fs::File) {
    let _ = file.sync_all().await;
}

async fn pause_tick(fetch: &Fetch<'_>) {
    loop {
        if fetch.pause.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
