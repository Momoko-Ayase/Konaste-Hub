//! Recoverable staged updater for one Konaste installation.
//!
//! Callers (the Hub) supply an already-authenticated [`Manifest`] and,
//! when the parent has one, an [`OfficialManifest`] whose install-relative
//! path is title-specific. This crate does not discover, parse, or vouch for
//! either document. It downloads the changed files, verifies the staged delta,
//! then replaces installed files with a durable journal.
//!
//! # Preconditions the parent must hold
//!
//! * No game process is running for this installation. This crate does not
//!   inspect or stop processes, and it will write files the parent said were idle.
//! * `root` is the canonical installation directory the user approved.
//! * Official metadata authenticity was established before `prepare`.
//!
//! # Ordering
//!
//! `open`, then `prepare`, then `run`. After a pause, a process restart, or a
//! resumable [`UpdateError::Download`], call `run` again. Call `resume` first
//! if `pause` was requested: `run` does not clear the pause latch, so a pause
//! that arrives before the worker enters `run` is still honored.
//!
//! `recover` never downloads. It remeasures partials and finishes or rolls
//! back an interrupted replacement. The official manifest path is written only
//! after every content file verifies, and the session does not become [`SessionPhase::Idle`]
//! while that path still has its previous bytes.
//!
//! Unknown files in the installation are never deleted.

mod download;
mod error;
mod fsutil;
mod hash;
mod install;
mod journal;
mod manifest;
mod paths;
mod pin;
mod policy;
mod volume;

#[cfg(test)]
mod tests;

pub use error::UpdateError;
pub use journal::COMMITTED_MANIFEST_FILE;
pub use manifest::{Manifest, ManifestFile, OfficialManifest, Sha256};
pub use paths::WORK_DIR_NAME;
pub use policy::DownloadPolicy;

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use journal::{FilePhase, Journal, JournalPhase, WorkPaths};

const DEFAULT_PARALLEL: usize = 4;

/// Bounds the parent can ask for. Anything else is rejected.
const PARALLEL_CHOICES: [usize; 4] = [2, 4, 6, 8];

/// One unit of work. `Clone` so a completed future can hand its job back;
/// the URL and digest are small and the future owns its own copies.
#[derive(Clone)]
struct DownloadJob {
    index: usize,
    relative: String,
    url: String,
    size: u64,
    sha: String,
    etag: Option<String>,
}

/// What the session is doing. `Idle` means the journal is gone and every
/// planned file, including the official manifest path, matches the manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPhase {
    Idle,
    /// `prepare` is hashing installed or staged files. Not a durable journal phase.
    Preparing,
    /// The plan is durable and `prepare` has returned.
    Planned,
    Downloading,
    Paused,
    Verifying,
    Installing,
    Publishing,
    Recovering,
    Failed,
}

/// Phase-owned work a UI can read without taking the journal or the lease.
/// `progress()` is the fuller view and may take the journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkSnapshot {
    pub phase: SessionPhase,
    pub files_completed: u64,
    pub files_total: u64,
    pub bytes_completed: u64,
    pub bytes_total: u64,
    /// Validated manifest-relative path of the file currently being read.
    pub current: Option<String>,
}

impl WorkSnapshot {
    fn empty(phase: SessionPhase) -> Self {
        Self { phase, files_completed: 0, files_total: 0, bytes_completed: 0, bytes_total: 0, current: None }
    }
}

/// Byte progress. `verified_bytes` counts files whose staged hash matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub phase: SessionPhase,
    pub files_total: u64,
    pub files_completed: u64,
    pub bytes_total: u64,
    pub bytes_received: u64,
    pub paused: bool,
}

/// A bounded active transfer. Paths are manifest-relative, never URLs or roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileProgress {
    pub relative: String,
    pub bytes_received: u64,
    pub bytes_total: u64,
    pub retrying: bool,
}

/// Useful local bytes and actual network writes are separate: retries can
/// transfer bytes without increasing the amount of reusable staged content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferSnapshot {
    pub progress: Progress,
    pub network_bytes: u64,
    pub files: Vec<FileProgress>,
    /// True while the session is holding all new files behind one bounded
    /// wait because the last attempt reached no server at all. Additive: a UI
    /// that ignores it behaves as before, but it can say "waiting for network"
    /// instead of looking frozen.
    pub waiting_for_network: bool,
}

/// Point-in-time view a UI can render without re-entering the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub phase: SessionPhase,
    pub manifest_version: Option<String>,
    pub progress: Progress,
    pub resumable: bool,
    pub official_path: Option<String>,
    pub official_published: bool,
}

/// Result of planning. Files already matching the install are not downloaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub manifest_version: String,
    pub download_files: usize,
    pub download_bytes: u64,
    pub unchanged_files: usize,
    pub official_path: Option<String>,
}

/// What `recover` found. `needs_run` means bytes are still missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovery {
    pub phase: SessionPhase,
    pub repaired_files: u32,
    pub needs_run: bool,
}

struct Shared {
    paths: WorkPaths,
    policy: DownloadPolicy,
    client: reqwest::Client,
    journal: Mutex<Option<Journal>>,
    phase: Mutex<SessionPhase>,
    pause: AtomicBool,
    network_bytes: AtomicU64,
    transfers: Mutex<BTreeMap<String, FileProgress>>,
    /// Copied out under this lock only. Never held across disk or network IO.
    work: Mutex<WorkSnapshot>,
    /// Set while a file failed without ever receiving response headers. The
    /// download supervisor treats this as "the network is down" and holds all
    /// new work behind one capped wait instead of retrying every file.
    net_failed: AtomicBool,
    /// Set while the supervisor is inside that bounded wait, so the UI can say
    /// "waiting for network" instead of showing a frozen download.
    net_waiting: AtomicBool,
    parallel: usize,
    pace: std::time::Duration,
    /// Set for the whole of `prepare`, `run`, or `recover`. Not held across
    /// `.await` by a `MutexGuard`; the flag is an owned guard.
    busy: AtomicBool,
    /// Exclusive lock held for the life of this session. Dropping the session
    /// releases it. A second process opening the same root fails.
    _lock: std::os::windows::io::OwnedHandle,
    /// Set only by a successful `prepare` in this process. A reopened journal
    /// stays unbound until that call matches it.
    bound: AtomicBool,
    /// One immediate recovery may reuse prepare's stage reconciliation. Never persisted.
    stages_checked: AtomicBool,
    /// Root, work, and every directory created under them. Held until drop.
    lease: Mutex<crate::pin::Lease>,
}

/// Options for [`Updater::open_with`]. `parallel` is fixed before `prepare`.
#[derive(Debug, Clone)]
pub struct UpdaterOptions {
    pub policy: DownloadPolicy,
    parallel: usize,
    /// One millisecond in production. A test passes a shorter unit so the
    /// same connect, header, and read-idle deadlines finish in seconds.
    pub(crate) pace: std::time::Duration,
}

impl UpdaterOptions {
    pub fn new(policy: DownloadPolicy) -> Self {
        Self { policy, parallel: DEFAULT_PARALLEL, pace: std::time::Duration::from_millis(1) }
    }

    /// Scale every transport deadline by `unit` instead of one millisecond.
    /// Not part of the public options a caller can set.
    #[cfg(test)]
    pub(crate) fn pace(mut self, unit: std::time::Duration) -> Self {
        self.pace = unit;
        self
    }

    /// `parallel` must be 2, 4, 6, or 8.
    pub fn parallel(mut self, parallel: usize) -> Result<Self, UpdateError> {
        if !PARALLEL_CHOICES.contains(&parallel) {
            return Err(UpdateError::state("parallel downloads must be 2, 4, 6, or 8"));
        }
        self.parallel = parallel;
        Ok(self)
    }
}

/// One update session bound to a single installation root.
///
/// Cloning shares the session. `pause`, `progress`, and `snapshot` are safe
/// on a clone (or on [`UpdateHandle`]) while another task is inside `run`.
/// Locks are not held across `.await`.
#[derive(Clone)]
pub struct Updater {
    shared: Arc<Shared>,
}

/// UI-facing handle. It cannot prepare, download, or replace files.
#[derive(Clone)]
pub struct UpdateHandle {
    shared: Arc<Shared>,
}

#[derive(Clone, Copy)]
enum RootMode {
    Existing,
    Empty,
    Unstarted,
}

impl Updater {
    /// Open `root`, which must already exist and be a directory.
    ///
    /// An existing journal is loaded but not applied. Call `recover` or `run`.
    pub fn open(root: impl AsRef<Path>, policy: DownloadPolicy) -> Result<Self, UpdateError> {
        Self::open_with(root, UpdaterOptions::new(policy))
    }

    pub fn open_with(root: impl AsRef<Path>, options: UpdaterOptions) -> Result<Self, UpdateError> {
        Self::open_root(root.as_ref(), options, RootMode::Existing)
    }

    /// Open a directory that must be empty, for a brand-new installation.
    ///
    /// Ancestors and `root` are pinned before the emptiness check. A nonempty
    /// directory is refused and `.konaste-hub` is not created. The path is used
    /// as given after that pin; it is not canonicalized a second time.
    pub fn open_fresh(root: impl AsRef<Path>, policy: DownloadPolicy) -> Result<Self, UpdateError> {
        Self::open_fresh_with(root, UpdaterOptions::new(policy))
    }

    pub fn open_fresh_with(root: impl AsRef<Path>, options: UpdaterOptions) -> Result<Self, UpdateError> {
        Self::open_root(root.as_ref(), options, RootMode::Empty)
    }

    /// Open an already-reserved fresh destination after an interruption before
    /// preparation began. Its only allowed child is the pinned work directory,
    /// and it must not already contain a transaction journal.
    pub fn open_unstarted_with(root: impl AsRef<Path>, options: UpdaterOptions) -> Result<Self, UpdateError> {
        Self::open_root(root.as_ref(), options, RootMode::Unstarted)
    }

    fn open_root(root: &Path, options: UpdaterOptions, mode: RootMode) -> Result<Self, UpdateError> {
        let policy = options.policy;
        let parallel = options.parallel;
        let pace = options.pace;
        let root = absolute_directory(root)?;
        let paths = WorkPaths::under(root);
        let lease = match mode {
            RootMode::Existing => crate::pin::Lease::open(&paths.root, WORK_DIR_NAME)?,
            RootMode::Empty => crate::pin::Lease::open_empty(&paths.root, WORK_DIR_NAME, false)?,
            RootMode::Unstarted => crate::pin::Lease::open_empty(&paths.root, WORK_DIR_NAME, true)?,
        };
        volume::same_volume(&paths.root, &paths.work)?;
        let lock = acquire_session_lock(&lease)?;
        let work = lease.work()?;
        let journal = journal::load_from(&work, &policy)?;
        if matches!(mode, RootMode::Unstarted) && journal.is_some() {
            return Err(UpdateError::state("fresh destination already contains an update journal"));
        }
        let phase = match &journal {
            None => SessionPhase::Idle,
            Some(doc) => phase_of(doc),
        };
        let client = reqwest::Client::builder()
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(download::pace_of(options.pace, std::time::Duration::from_secs(10)))
            // A per-read deadline, not a whole-request one. The body watchdog
            // sits beside this; neither cuts a moving body.
            .read_timeout(download::pace_of(options.pace, std::time::Duration::from_secs(15)))
            .tcp_keepalive(std::time::Duration::from_secs(20))
            .tcp_keepalive_retries(Some(4))
            // An idle keep-alive socket is not worth reusing across a sleep.
            // Five minutes is far longer than the gap between files of one
            // run and far shorter than most resume-in-the-morning sessions,
            // so a socket wedged by suspend is retired before it is reused.
            .pool_idle_timeout(std::time::Duration::from_secs(300))
            // Pings only ride a live stream. An idle pooled socket is governed
            // by pool_idle_timeout; an in-flight body has the read deadline.
            .http2_keep_alive_interval(Some(std::time::Duration::from_secs(20)))
            .http2_keep_alive_timeout(std::time::Duration::from_secs(10))
            .http2_keep_alive_while_idle(false)
            .build()
            .map_err(|err| UpdateError::Other(format!("http client: {err}")))?;
        Ok(Self {
            shared: Arc::new(Shared {
                paths,
                policy,
                client,
                journal: Mutex::new(journal),
                phase: Mutex::new(phase),
                pause: AtomicBool::new(false),
                network_bytes: AtomicU64::new(0),
                transfers: Mutex::new(BTreeMap::new()),
                work: Mutex::new(WorkSnapshot::empty(phase)),
                net_failed: AtomicBool::new(false),
                net_waiting: AtomicBool::new(false),
                parallel,
                pace,
                busy: AtomicBool::new(false),
                _lock: lock,
                bound: AtomicBool::new(false),
                stages_checked: AtomicBool::new(false),
                lease: Mutex::new(lease),
            }),
        })
    }

    /// Validate, diff the installation, and persist the plan.
    ///
    /// Does not download and does not modify installed files. Replaces a
    /// previous plan only when that plan is not mid-replacement.
    pub fn prepare(&self, manifest: Manifest, official: Option<OfficialManifest>) -> Result<Plan, UpdateError> {
        let _busy = self.busy()?;
        self.shared.bound.store(false, Ordering::SeqCst);
        self.shared.stages_checked.store(false, Ordering::SeqCst);
        let checked = manifest.check(official)?;
        self.publish_work(WorkSnapshot {
            phase: SessionPhase::Preparing,
            files_completed: 0,
            files_total: checked.files.len() as u64 * 2,
            bytes_completed: 0,
            bytes_total: 0,
            current: checked.files.first().map(|file| file.relative.display().to_string()),
        });
        for file in &checked.files {
            if let Some(url) = &file.url {
                self.shared.policy.check_url(url)?;
            }
        }
        let mut doc = Journal::from_checked(&checked);
        let previous = journal::load_from(&self.shared.lease.lock().unwrap_or_else(|err| err.into_inner()).work()?, &self.shared.policy)?;
        if let Some(existing) = previous.as_ref()
            && matches!(existing.phase, JournalPhase::Installing | JournalPhase::Publishing)
        {
            if !same_identity(existing, &doc) {
                self.set_phase(SessionPhase::Failed);
                return Err(UpdateError::RecoveryRequired);
            }
            let plan = Plan {
                manifest_version: checked.version,
                download_files: existing.files.iter().filter(|file| file.phase != FilePhase::Installed).count(),
                download_bytes: existing.files.iter().filter(|file| file.phase != FilePhase::Installed).map(|file| file.size).sum(),
                unchanged_files: existing.files.iter().filter(|file| file.phase == FilePhase::Installed).count(),
                official_path: doc.official.as_ref().map(|item| item.relative.clone()),
            };
            let mut guard = self.lock_journal()?;
            if guard.is_some() && !journal_matches(guard.as_ref(), previous.as_ref()) {
                return Err(UpdateError::PlanChanged);
            }
            let phase = phase_of(existing);
            *guard = previous;
            self.shared.bound.store(true, Ordering::SeqCst);
            self.set_phase(phase);
            return Ok(plan);
        }
        let mut download_files = 0_usize;
        let mut download_bytes = 0_u64;
        let mut unchanged = 0_usize;
        let prepare_total = doc.files.len() as u64;
        let mut prepare_done_bytes = 0_u64;
        for (index, (file, checked_file)) in doc.files.iter_mut().zip(&checked.files).enumerate() {
            self.publish_work(WorkSnapshot {
                phase: SessionPhase::Preparing,
                files_completed: index as u64,
                files_total: prepare_total * 2,
                bytes_completed: prepare_done_bytes,
                bytes_total: 0,
                current: Some(file.relative.clone()),
            });
            let dest = install::install_path(&self.shared.paths, &file.relative)?;
            let opened = {
                let mut lease = self.shared.lease.lock().unwrap_or_else(|err| err.into_inner());
                lease.open_path(&dest)?
            };
            let kind = match opened {
                None => InstalledKind::Missing,
                Some(opened) if opened.size() != file.size => InstalledKind::Different,
                Some(mut opened) => {
                    let digest = opened.hash_with(|read| {
                        self.publish_work(WorkSnapshot {
                            phase: SessionPhase::Preparing,
                            files_completed: index as u64,
                            files_total: prepare_total * 2,
                            bytes_completed: prepare_done_bytes + read,
                            bytes_total: 0,
                            current: Some(file.relative.clone()),
                        });
                    })?;
                    prepare_done_bytes += file.size;
                    if Sha256::parse(&file.sha256)?.matches(&digest) { InstalledKind::Match } else { InstalledKind::Different }
                }
            };
            match kind {
                InstalledKind::Match => {
                    file.phase = FilePhase::Installed;
                    file.received = file.size;
                    unchanged += 1;
                }
                InstalledKind::Missing | InstalledKind::Different => {
                    if checked_file.url.is_none() {
                        return Err(UpdateError::manifest(format!("`{}` differs from the install and has no download url", file.relative)));
                    }
                    download_files += 1;
                    download_bytes += file.size;
                }
            }
        }
        if let Some(official) = &mut doc.official {
            let dest = install::install_path(&self.shared.paths, &official.relative)?;
            official.had_previous = self.shared.lease.lock().unwrap_or_else(|err| err.into_inner()).open_path(&dest)?.is_some();
            official.phase = FilePhase::Staged;
        }
        let need_space = download_bytes.saturating_mul(2);
        let available = volume::free_bytes(&self.shared.paths.root)?;
        if available < need_space {
            return Err(UpdateError::DiskSpace { needed: need_space, available });
        }
        doc.phase = if download_files == 0 { JournalPhase::Staged } else { JournalPhase::Planned };
        if let Some(existing) = previous.as_ref()
            && !same_identity(existing, &doc)
        {
            discard_foreign_stages(&mut self.shared.lease.lock().unwrap_or_else(|err| err.into_inner()), existing, &doc)?;
        }
        reconcile_stages(
            &mut doc,
            previous.as_ref(),
            &mut || Ok(self.shared.lease.lock().unwrap_or_else(|err| err.into_inner())),
            |relative, done, total, read| {
                self.publish_work(WorkSnapshot {
                    phase: SessionPhase::Preparing,
                    files_completed: prepare_total + done,
                    files_total: prepare_total + total,
                    bytes_completed: prepare_done_bytes + read,
                    bytes_total: 0,
                    current: Some(relative.to_string()),
                });
            },
        )?;
        doc.phase = if doc.files.iter().all(|file| matches!(file.phase, FilePhase::Installed | FilePhase::Staged)) {
            JournalPhase::Staged
        } else {
            JournalPhase::Planned
        };
        if let Some(official) = &checked.official {
            journal::publish_bytes(
                &mut self.shared.lease.lock().unwrap_or_else(|err| err.into_inner()),
                "official.stage.tmp",
                "official.stage",
                &official.bytes,
            )?;
        }
        journal::store(&self.shared.paths, &doc, &mut self.shared.lease.lock().unwrap_or_else(|err| err.into_inner()))?;
        let mut guard = self.lock_journal()?;
        if guard.is_some() && !journal_matches(guard.as_ref(), previous.as_ref()) {
            return Err(UpdateError::PlanChanged);
        }
        *guard = Some(doc);
        self.shared.bound.store(true, Ordering::SeqCst);
        self.shared.stages_checked.store(true, Ordering::SeqCst);
        drop(guard);
        self.set_phase(if download_files == 0 { SessionPhase::Verifying } else { SessionPhase::Planned });
        Ok(Plan {
            manifest_version: checked.version,
            download_files,
            download_bytes,
            unchanged_files: unchanged,
            official_path: checked.official.map(|item| item.relative.display().to_string()),
        })
    }

    /// Download missing staged files, verify the delta, then replace.
    ///
    /// A set pause latch returns [`SessionPhase::Paused`] without discarding
    /// partials, including a latch set before this call starts.
    pub async fn run(&self) -> Result<SessionPhase, UpdateError> {
        self.run_guarded(|| Ok(())).await
    }

    /// Like [`Updater::run`], calling `guard` immediately before every
    /// destination replacement and official publication. A guard error leaves
    /// the journal and stage untouched.
    pub async fn run_guarded<G: FnMut() -> Result<(), UpdateError>>(&self, mut guard_fn: G) -> Result<SessionPhase, UpdateError> {
        let _busy = self.busy()?;
        self.require_fresh_bind()?;
        self.shared.stages_checked.store(false, Ordering::SeqCst);
        self.check_disk_plan()?;
        if self.shared.pause.load(Ordering::SeqCst) {
            self.set_phase(SessionPhase::Paused);
            return Ok(SessionPhase::Paused);
        }
        let phase = {
            let guard = self.lock_journal()?;
            let doc = guard.as_ref().ok_or_else(|| UpdateError::state("prepare an update before run"))?;
            doc.phase
        };
        if matches!(phase, JournalPhase::Installing | JournalPhase::Publishing | JournalPhase::Committed) {
            let recovery = self.recover_locked(&mut guard_fn)?;
            if !recovery.needs_run {
                return Ok(SessionPhase::Idle);
            }
        }
        self.download_all().await?;
        if self.shared.pause.load(Ordering::SeqCst) {
            self.set_phase(SessionPhase::Paused);
            return Ok(SessionPhase::Paused);
        }
        self.set_phase(SessionPhase::Verifying);
        {
            let guard = self.lock_journal()?;
            let doc = guard.as_ref().ok_or_else(|| UpdateError::state("journal disappeared"))?;
            install::verify_staged_delta(
                &self.shared.paths,
                doc,
                &mut || Ok(self.shared.lease.lock().unwrap_or_else(|err| err.into_inner())),
                |relative, files_completed, files_total, bytes_completed, bytes_total| {
                    self.publish_work(WorkSnapshot {
                        phase: SessionPhase::Verifying,
                        files_completed,
                        files_total,
                        bytes_completed,
                        bytes_total,
                        current: Some(relative.to_string()),
                    });
                },
            )?;
        }
        guard_fn()?;
        self.check_disk_plan()?;
        self.set_phase(SessionPhase::Installing);
        {
            let mut guard = self.lock_journal()?;
            let doc = guard.as_mut().ok_or_else(|| UpdateError::state("journal disappeared"))?;
            install::apply(
                &self.shared.paths,
                doc,
                &mut || Ok(self.shared.lease.lock().unwrap_or_else(|err| err.into_inner())),
                &mut guard_fn,
                |relative, files_completed, files_total, bytes_completed, bytes_total| {
                    self.publish_work(WorkSnapshot {
                        phase: SessionPhase::Installing,
                        files_completed,
                        files_total,
                        bytes_completed,
                        bytes_total,
                        current: Some(relative.to_string()),
                    });
                },
            )?;
        }
        self.finish_success()?;
        Ok(SessionPhase::Idle)
    }

    /// Latch a cooperative pause. Does not clear itself when `run` starts.
    pub fn pause(&self) {
        self.shared.pause.store(true, Ordering::SeqCst);
    }

    /// Clear the pause latch. Required before a later `run` will do work.
    pub fn resume(&self) {
        self.shared.pause.store(false, Ordering::SeqCst);
    }

    pub fn progress(&self) -> Progress {
        let guard = self.shared.journal.lock().unwrap_or_else(|err| err.into_inner());
        progress_of(
            guard.as_ref(),
            *self.shared.phase.lock().unwrap_or_else(|e| e.into_inner()),
            self.shared.pause.load(Ordering::SeqCst),
            &self.shared.transfers.lock().unwrap_or_else(|e| e.into_inner()),
        )
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        let guard = self.shared.journal.lock().unwrap_or_else(|err| err.into_inner());
        let phase = *self.shared.phase.lock().unwrap_or_else(|e| e.into_inner());
        let progress = progress_of(
            guard.as_ref(),
            phase,
            self.shared.pause.load(Ordering::SeqCst),
            &self.shared.transfers.lock().unwrap_or_else(|e| e.into_inner()),
        );
        let (version, official_path, official_published, resumable) = match guard.as_ref() {
            Some(doc) => (
                Some(doc.manifest_version.clone()),
                doc.official.as_ref().map(|item| item.relative.clone()),
                doc.official.as_ref().map(|item| item.phase == FilePhase::Installed).unwrap_or(true),
                !matches!(doc.phase, JournalPhase::Committed),
            ),
            None => (None, None, true, false),
        };
        SessionSnapshot { phase, manifest_version: version, progress, resumable, official_path, official_published }
    }

    /// Reconcile disk with the journal. Never contacts the network.
    pub fn recover(&self) -> Result<Recovery, UpdateError> {
        self.recover_guarded(|| Ok(()))
    }

    /// Like [`Updater::recover`], calling `guard` immediately before every
    /// recovery mutation. A guard error leaves the journal and stage untouched.
    pub fn recover_guarded<G: FnMut() -> Result<(), UpdateError>>(&self, mut guard_fn: G) -> Result<Recovery, UpdateError> {
        let _busy = self.busy()?;
        self.require_fresh_bind()?;
        if self.shared.stages_checked.swap(false, Ordering::SeqCst) {
            self.check_disk_plan()?;
            let needs_run = self.lock_journal()?.as_ref().is_some_and(|doc| {
                matches!(doc.phase, JournalPhase::Planned | JournalPhase::Downloading | JournalPhase::Staged)
                    && (doc.files.iter().any(|file| file.phase != FilePhase::Installed)
                        || doc.official.as_ref().is_some_and(|file| file.phase != FilePhase::Installed))
            });
            if needs_run {
                // Prepare has just reconciled these stages against the fresh source.
                // Run still verifies the full delta before any destination mutation.
                guard_fn()?;
                self.set_phase(SessionPhase::Planned);
                return Ok(Recovery { phase: SessionPhase::Planned, repaired_files: 0, needs_run: true });
            }
        }
        self.recover_locked(&mut guard_fn)
    }

    fn check_disk_plan(&self) -> Result<(), UpdateError> {
        let bound = self.lock_journal()?;
        let work = self.shared.lease.lock().unwrap_or_else(|err| err.into_inner()).work()?;
        let disk = journal::load_from(&work, &self.shared.policy)?;
        if !journal_matches(bound.as_ref(), disk.as_ref()) {
            self.shared.bound.store(false, Ordering::SeqCst);
            return Err(UpdateError::PlanChanged);
        }
        Ok(())
    }

    fn require_fresh_bind(&self) -> Result<(), UpdateError> {
        if self.shared.bound.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(UpdateError::state("bind this session with prepare before run or recover"))
        }
    }

    fn recover_locked(&self, guard_fn: &mut dyn FnMut() -> Result<(), UpdateError>) -> Result<Recovery, UpdateError> {
        guard_fn()?;
        self.set_phase(SessionPhase::Recovering);
        let mut repaired = 0_u32;
        let needs_run = {
            let mut guard = self.lock_journal()?;
            let work = self.shared.lease.lock().unwrap_or_else(|err| err.into_inner()).work()?;
            let disk = journal::load_from(&work, &self.shared.policy)?;
            if !journal_matches(guard.as_ref(), disk.as_ref()) {
                self.shared.bound.store(false, Ordering::SeqCst);
                return Err(UpdateError::PlanChanged);
            }
            *guard = disk;
            let Some(doc) = guard.as_mut() else {
                self.set_phase(SessionPhase::Idle);
                return Ok(Recovery { phase: SessionPhase::Idle, repaired_files: 0, needs_run: false });
            };
            let stage_total = doc.files.iter().filter(|file| file.phase != FilePhase::Installed).count() as u64;
            let mut stage_done = 0_u64;
            let mut read_bytes = 0_u64;
            for file in &mut doc.files {
                if file.phase == FilePhase::Installed {
                    continue;
                }
                self.publish_work(WorkSnapshot {
                    phase: SessionPhase::Recovering,
                    files_completed: stage_done,
                    files_total: stage_total,
                    bytes_completed: read_bytes,
                    bytes_total: 0,
                    current: Some(file.relative.clone()),
                });
                let (parent, name) = stage_parent(&mut self.shared.lease.lock().unwrap_or_else(|err| err.into_inner()), &file.relative)?;
                let staged = crate::pin::open_file_read(&parent, &name)?;
                match staged {
                    Some(mut staged) if staged.size() == file.size => {
                        let expected = Sha256::parse(&file.sha256)?;
                        let digest = staged.hash_with(|read| {
                            self.publish_work(WorkSnapshot {
                                phase: SessionPhase::Recovering,
                                files_completed: stage_done,
                                files_total: stage_total,
                                bytes_completed: read_bytes + read,
                                bytes_total: 0,
                                current: Some(file.relative.clone()),
                            })
                        })?;
                        read_bytes += file.size;
                        if expected.matches(&digest) {
                            if file.phase != FilePhase::Staged {
                                repaired += 1;
                            }
                            file.phase = FilePhase::Staged;
                            file.received = file.size;
                        } else {
                            drop(staged);
                            crate::pin::delete_file(&parent, &name)?;
                            file.received = 0;
                            if file.phase != FilePhase::Replacing {
                                file.phase = FilePhase::Pending;
                            }
                        }
                    }
                    Some(staged) if staged.size() < file.size => {
                        file.phase = FilePhase::Partial;
                        file.received = staged.size();
                    }
                    Some(staged) => {
                        drop(staged);
                        crate::pin::delete_file(&parent, &name)?;
                        file.received = 0;
                        if file.phase != FilePhase::Replacing {
                            file.phase = FilePhase::Pending;
                        }
                    }
                    None => {
                        file.received = 0;
                        if file.phase != FilePhase::Replacing {
                            file.phase = FilePhase::Pending;
                        }
                    }
                }
                stage_done += 1;
            }
            self.set_phase(SessionPhase::Recovering);
            journal::store(&self.shared.paths, doc, &mut self.shared.lease.lock().unwrap_or_else(|err| err.into_inner()))?;
            install::recover_install(
                &self.shared.paths,
                doc,
                &mut self.shared.lease.lock().unwrap_or_else(|err| err.into_inner()),
                guard_fn,
            )?;
            doc.files.iter().any(|file| file.phase != FilePhase::Installed)
                || doc.official.as_ref().map(|item| item.phase != FilePhase::Installed).unwrap_or(false)
        };
        if !needs_run {
            self.finish_success()?;
        } else {
            self.set_phase(SessionPhase::Planned);
        }
        Ok(Recovery { phase: self.progress().phase, repaired_files: repaired, needs_run })
    }

    pub fn handle(&self) -> UpdateHandle {
        UpdateHandle { shared: Arc::clone(&self.shared) }
    }

    async fn download_all(&self) -> Result<(), UpdateError> {
        self.set_phase(SessionPhase::Downloading);
        let (jobs, log) = {
            let mut guard = self.lock_journal()?;
            let doc = guard.as_mut().ok_or_else(|| UpdateError::state("nothing to download"))?;
            materialize_empty(doc, &mut self.shared.lease.lock().unwrap_or_else(|err| err.into_inner()))?;
            doc.phase = JournalPhase::Downloading;
            journal::store(&self.shared.paths, doc, &mut self.shared.lease.lock().unwrap_or_else(|err| err.into_inner()))?;
            // One log for every per-file record in this download. The checkpoint
            // above already retired any previous generation. The handle lives
            // only in this call: it is dropped before the next checkpoint.
            let log =
                Arc::new(Mutex::new(journal::DownloadLog::open(&mut self.shared.lease.lock().unwrap_or_else(|err| err.into_inner()))?));
            let jobs: Vec<_> = doc
                .files
                .iter()
                .enumerate()
                .filter(|(_, file)| file.phase != FilePhase::Installed && file.phase != FilePhase::Staged)
                .filter_map(|(index, file)| {
                    Some(DownloadJob {
                        index,
                        relative: file.relative.clone(),
                        url: file.url.clone()?,
                        size: file.size,
                        sha: file.sha256.clone(),
                        etag: file.etag.clone(),
                    })
                })
                .collect();
            (jobs, log)
        };
        let parallel = self.shared.parallel.max(1);
        let mut queue: std::collections::VecDeque<usize> = (0..jobs.len()).collect();
        let mut inflight = Vec::new();
        // Jobs currently in flight, in completion order. `select_all` gives the
        // index of the future it resolved, which is where this job came from.
        let mut flying: Vec<usize> = Vec::new();
        let mut failure = None;
        // Files that already used their whole per-file budget. The first such
        // failure requeues the file; the second stops the session. A file only
        // refused because the session was offline does not count.
        let mut requeued: std::collections::BTreeSet<usize> = Default::default();
        let mut outage = Outage::default();
        loop {
            let paused = self.shared.pause.load(Ordering::SeqCst);
            if paused || failure.is_some() {
                self.shared.net_failed.store(false, Ordering::SeqCst);
                self.shared.net_waiting.store(false, Ordering::SeqCst);
                // Drain what is already running. Nothing new is started.
                if inflight.is_empty() {
                    break;
                }
            } else if self.shared.net_failed.load(Ordering::SeqCst) {
                // One wait covers every queued file. No per-file budget is
                // touched while the group sits here, and every partial stays.
                match outage.next_delay(self.shared.pace) {
                    Some(delay) => {
                        self.shared.net_waiting.store(true, Ordering::SeqCst);
                        let finished = sleep_unless_paused(&self.shared.pause, delay).await;
                        self.shared.net_waiting.store(false, Ordering::SeqCst);
                        if !finished || self.shared.pause.load(Ordering::SeqCst) {
                            continue;
                        }
                        // Probe with one file. If that also reaches no server,
                        // the marker is re-armed and the wait grows. The step
                        // only drops when some attempt actually completes.
                        self.shared.net_failed.store(false, Ordering::SeqCst);
                        if let Some(index) = queue.pop_front() {
                            flying.push(index);
                            inflight.push(self.spawn_download(jobs[index].clone(), Arc::clone(&log)));
                        }
                        continue;
                    }
                    None => {
                        failure = Some(UpdateError::download("download", "network stayed unreachable past the session wait"));
                        continue;
                    }
                }
            } else {
                while inflight.len() < parallel {
                    let Some(index) = queue.pop_front() else { break };
                    flying.push(index);
                    inflight.push(self.spawn_download(jobs[index].clone(), Arc::clone(&log)));
                }
            }
            if inflight.is_empty() {
                if queue.is_empty() {
                    break;
                }
                continue;
            }
            let (result, at, rest) = futures_util::future::select_all(inflight).await;
            inflight = rest;
            let index = flying.swap_remove(at);
            let download::FileDone { failed } = result;
            if !self.shared.net_failed.load(Ordering::SeqCst) {
                // A real success drops the wait back to its floor. A completed
                // attempt beats continuously while the group is already
                // waiting, so a long file mid-outage does not keep raising it.
                outage.reset();
            }
            let Some(failed) = failed else { continue };
            if self.shared.pause.load(Ordering::SeqCst) {
                // Every in-flight file saw the latch. Its error is not the
                // file's and must not be requeued as one.
                continue;
            }
            if failure.is_some() {
                continue;
            }
            match failed {
                download::Failed::Permanent(error) => failure = Some(error),
                download::Failed::Transient(error) => {
                    // The file spent a whole per-file budget. Give it one more
                    // visit after everything else; a body class that cannot
                    // hash is already terminal inside the fetch.
                    if requeued.insert(index) {
                        queue.push_back(index);
                    } else {
                        failure = Some(error);
                    }
                }
                download::Failed::Offline => {
                    // The session was already waiting; this file spent none of
                    // its own budget. Retry it when the group is probed next.
                    queue.push_front(index);
                }
            }
        }
        // Every completion record has been flushed. Drop the write handle before
        // a checkpoint or a later resume tries to retire or reopen the log.
        drop(log);
        self.shared.net_failed.store(false, Ordering::SeqCst);
        self.shared.net_waiting.store(false, Ordering::SeqCst);
        if let Some(error) = failure {
            return Err(error);
        }
        if self.shared.pause.load(Ordering::SeqCst) {
            self.set_phase(SessionPhase::Paused);
            return Ok(());
        }
        let mut guard = self.lock_journal()?;
        if let Some(doc) = guard.as_mut()
            && doc.files.iter().all(|file| file.phase == FilePhase::Staged || file.phase == FilePhase::Installed)
        {
            doc.phase = JournalPhase::Staged;
            journal::store(&self.shared.paths, doc, &mut self.shared.lease.lock().unwrap_or_else(|err| err.into_inner()))?;
        }
        Ok(())
    }

    fn spawn_download(
        &self,
        job: DownloadJob,
        log: Arc<Mutex<journal::DownloadLog>>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = download::FileDone> + Send>> {
        let DownloadJob { index, relative, url, size, sha, etag } = job;
        let shared = Arc::clone(&self.shared);
        Box::pin(async move {
            // Every failure is reported as a class, including a local one that
            // fails before a request is built. Such a failure is permanent: no
            // number of retries fixes a malformed digest or a missing stage.
            let run = async {
                let expected = Sha256::parse(&sha)?;
                let (parent, name, replacement_parent) = {
                    let mut lease = shared.lease.lock().unwrap_or_else(|err| err.into_inner());
                    let (parent, name) = stage_parent(&mut lease, &relative)?;
                    let replacement_parent = lease.descend_from_work(&["replacements".into()])?;
                    (parent, name, replacement_parent)
                };
                let replacement_name = std::ffi::OsString::from(format!("{}.part", hex::encode(hash::sha256(relative.as_bytes()))));
                let initial = {
                    let guard = shared.journal.lock().unwrap_or_else(|err| err.into_inner());
                    guard
                        .as_ref()
                        .and_then(|doc| doc.files.get(index))
                        .filter(|file| file.relative == relative)
                        .map_or(0, |file| file.received.min(size))
                };
                shared.transfers.lock().unwrap_or_else(|err| err.into_inner()).insert(
                    relative.clone(),
                    FileProgress { relative: relative.clone(), bytes_received: initial, bytes_total: size, retrying: false },
                );
                // Drop removes the entry if this future is cancelled before the
                // journal update below. It does not touch the stage or the log.
                let active = ActiveTransfer { transfers: &shared.transfers, relative: &relative };
                let fetch = download::Fetch {
                    client: &shared.client,
                    policy: &shared.policy,
                    url: &url,
                    expected: &expected,
                    expected_size: size,
                    expected_etag: etag.as_deref(),
                    dest: &relative,
                    parent: &parent,
                    name: &name,
                    replacement_parent: &replacement_parent,
                    replacement_name: &replacement_name,
                    pause: &shared.pause,
                    bytes_progress: &|n| {
                        shared.network_bytes.fetch_add(n, Ordering::Relaxed);
                    },
                    file_progress: &|received| {
                        if let Some(file) = shared.transfers.lock().unwrap_or_else(|err| err.into_inner()).get_mut(&relative) {
                            file.bytes_received = received.min(size);
                        }
                    },
                    retrying: &|retrying| {
                        if let Some(file) = shared.transfers.lock().unwrap_or_else(|err| err.into_inner()).get_mut(&relative) {
                            file.retrying = retrying;
                        }
                    },
                    pace: shared.pace,
                    signal: download::NetSignal { failed: &shared.net_failed },
                    learned_etag: &|etag| {
                        let mut guard = shared.journal.lock().unwrap_or_else(|err| err.into_inner());
                        let doc = guard.as_mut().ok_or_else(|| UpdateError::state("download journal disappeared"))?;
                        let file = doc
                            .files
                            .get_mut(index)
                            .filter(|file| file.relative == relative)
                            .ok_or_else(|| UpdateError::state("download is not in the bound plan"))?;
                        if file.etag.as_deref() != Some(etag) {
                            file.etag = Some(etag.to_string());
                            if file.phase == FilePhase::Pending {
                                file.phase = FilePhase::Partial;
                            }
                            // Flushed before this callback returns, and the fetcher
                            // writes no original-stage byte until it returns.
                            log.lock().unwrap_or_else(|err| err.into_inner()).record_etag(index, etag, file)?;
                        }
                        Ok(())
                    },
                };
                // `fetch_verified` already labels every failure with this file's
                // manifest-relative name. Rewrapping would hide the class.
                let transfer = download::fetch_verified(fetch).await;
                let mut guard = shared.journal.lock().unwrap_or_else(|err| err.into_inner());
                // Same removal Drop performs. Explicit while the journal lock is
                // held, so a snapshot cannot observe a finished file as active.
                active.release();
                if let Some(doc) = guard.as_mut()
                    && let Some(file) = doc.files.get_mut(index).filter(|file| file.relative == relative)
                {
                    match transfer.as_ref() {
                        Ok(transfer) => {
                            file.phase = FilePhase::Staged;
                            file.received = transfer.received;
                            file.etag.clone_from(&transfer.etag);
                        }
                        Err(_) => {
                            file.received = crate::pin::open_file_read(&parent, &name)?.map(|file| file.size().min(size)).unwrap_or(0);
                            file.phase = if file.received > 0 { FilePhase::Partial } else { FilePhase::Pending };
                        }
                    }
                    log.lock().unwrap_or_else(|err| err.into_inner()).record_received(index, file)?;
                }
                Ok::<_, UpdateError>(transfer.err())
            };
            let failed = match run.await {
                Ok(failed) => failed,
                Err(err) => Some(download::Failed::Permanent(err)),
            };
            download::FileDone { failed }
        })
    }

    fn finish_success(&self) -> Result<(), UpdateError> {
        let mut guard = self.lock_journal()?;
        if let Some(doc) = guard.as_ref() {
            let official_ok = doc.official.as_ref().map(|item| item.phase == FilePhase::Installed).unwrap_or(true);
            let files_ok = doc.files.iter().all(|file| file.phase == FilePhase::Installed);
            if files_ok && official_ok && doc.phase == JournalPhase::Committed {
                journal::remove(&self.shared.paths, &self.shared.lease.lock().unwrap_or_else(|err| err.into_inner()))?;
                *guard = None;
                drop(guard);
                self.set_phase(SessionPhase::Idle);
                return Ok(());
            }
        }
        Err(UpdateError::state("refusing to clear the journal before the official manifest is published"))
    }

    fn lock_journal(&self) -> Result<MutexGuard<'_, Option<Journal>>, UpdateError> {
        Ok(self.shared.journal.lock().unwrap_or_else(|err| err.into_inner()))
    }

    fn busy(&self) -> Result<BusyGuard<'_>, UpdateError> {
        if self.shared.busy.swap(true, Ordering::SeqCst) {
            return Err(UpdateError::state("another prepare, run, or recover is already active on this installation"));
        }
        Ok(BusyGuard { flag: &self.shared.busy })
    }

    fn set_phase(&self, phase: SessionPhase) {
        *self.shared.phase.lock().unwrap_or_else(|err| err.into_inner()) = phase;
        let mut work = self.shared.work.lock().unwrap_or_else(|err| err.into_inner());
        *work = WorkSnapshot::empty(phase);
    }

    fn publish_work(&self, snapshot: WorkSnapshot) {
        *self.shared.phase.lock().unwrap_or_else(|err| err.into_inner()) = snapshot.phase;
        *self.shared.work.lock().unwrap_or_else(|err| err.into_inner()) = snapshot;
    }
}

impl UpdateHandle {
    /// At most the configured parallel count of active files. Network bytes are
    /// monotonic within this session and exclude retained bytes from a restart.
    pub fn transfer_progress(&self) -> TransferSnapshot {
        let journal = self.shared.journal.lock().unwrap_or_else(|err| err.into_inner());
        let phase = *self.shared.phase.lock().unwrap_or_else(|err| err.into_inner());
        let files = self.shared.transfers.lock().unwrap_or_else(|err| err.into_inner());
        TransferSnapshot {
            progress: progress_of(journal.as_ref(), phase, self.shared.pause.load(Ordering::SeqCst), &files),
            network_bytes: self.shared.network_bytes.load(Ordering::Relaxed),
            files: files.values().cloned().collect(),
            waiting_for_network: self.shared.net_waiting.load(Ordering::SeqCst),
        }
    }

    pub fn pause(&self) {
        self.shared.pause.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.shared.pause.store(false, Ordering::SeqCst);
    }

    pub fn progress(&self) -> Progress {
        Updater { shared: Arc::clone(&self.shared) }.progress()
    }

    /// Phase-owned progress. Copies one snapshot and releases it. Does not take
    /// the journal or the lease, so it returns while those are held for hashing.
    pub fn work(&self) -> WorkSnapshot {
        self.shared.work.lock().unwrap_or_else(|err| err.into_inner()).clone()
    }

    /// Network snapshot, or `None` if the journal, phase, or transfer map is busy.
    /// Any busy lock is a retry. Guards are taken journal, then phase, then
    /// transfers, and all three are held until the snapshot is copied.
    pub fn try_transfer_progress(&self) -> Option<TransferSnapshot> {
        let journal = self.shared.journal.try_lock().ok()?;
        let phase = *self.shared.phase.try_lock().ok()?;
        let files = self.shared.transfers.try_lock().ok()?;
        Some(TransferSnapshot {
            progress: progress_of(journal.as_ref(), phase, self.shared.pause.load(Ordering::SeqCst), &files),
            network_bytes: self.shared.network_bytes.load(Ordering::Relaxed),
            files: files.values().cloned().collect(),
            waiting_for_network: self.shared.net_waiting.load(Ordering::SeqCst),
        })
    }

    pub fn snapshot(&self) -> SessionSnapshot {
        Updater { shared: Arc::clone(&self.shared) }.snapshot()
    }
}

/// Removes one active-transfer entry. Drop covers a cancelled download future.
/// [`ActiveTransfer::release`] is the same removal, taken under the journal lock.
struct ActiveTransfer<'a> {
    transfers: &'a Mutex<BTreeMap<String, FileProgress>>,
    relative: &'a str,
}

impl ActiveTransfer<'_> {
    fn release(self) {
        drop(self);
    }
}

impl Drop for ActiveTransfer<'_> {
    fn drop(&mut self) {
        self.transfers.lock().unwrap_or_else(|err| err.into_inner()).remove(self.relative);
    }
}

/// Capped wait between session-wide connection probes.
///
/// One complete outage should survive without user action; an outage longer
/// than a few minutes is no longer "flaky" and the user is better served by a
/// visible resumable error than by a process that waits forever. The sequence
/// is 1, 2, 4, 8, 16, then 30 s seven times — 211 s, about three and a half
/// minutes total.
#[derive(Default)]
struct Outage {
    step: u32,
}

impl Outage {
    const STEPS: u32 = 12;
    const FLOOR: Duration = Duration::from_secs(1);
    const CAP: Duration = Duration::from_secs(30);

    /// `unit` is the same pace every transport deadline uses, so a test can
    /// drive a whole outage in a fraction of a second.
    fn next_delay(&mut self, unit: Duration) -> Option<Duration> {
        if self.step >= Self::STEPS {
            return None;
        }
        let delay = Self::FLOOR.saturating_mul(1_u32 << self.step.min(5)).min(Self::CAP);
        self.step += 1;
        Some(download::pace_of(unit, delay))
    }

    /// Drop back to the floor after any completed attempt. A long file beats
    /// continuously through `observe`, so a blip mid-file does not inherit the
    /// wait a previous outage produced.
    fn reset(&mut self) {
        self.step = 0;
    }
}

/// Sleeps `total`, returning early with `false` when pause is latched.
async fn sleep_unless_paused(pause: &AtomicBool, total: Duration) -> bool {
    let step = Duration::from_millis(50);
    let mut left = total;
    while !left.is_zero() {
        if pause.load(Ordering::SeqCst) {
            return false;
        }
        let slice = step.min(left);
        tokio::time::sleep(slice).await;
        left = left.saturating_sub(slice);
    }
    !pause.load(Ordering::SeqCst)
}

struct BusyGuard<'a> {
    flag: &'a AtomicBool,
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

fn acquire_session_lock(lease: &crate::pin::Lease) -> Result<std::os::windows::io::OwnedHandle, UpdateError> {
    let work = lease.work()?;
    let handle = crate::pin::open_lock(&work, std::ffi::OsStr::new(journal::LOCK_FILE))?;
    crate::pin::try_lock_exclusive(&handle)?;
    Ok(handle)
}

fn absolute_directory(root: &Path) -> Result<PathBuf, UpdateError> {
    if !root.is_absolute() || root.components().any(|part| matches!(part, Component::ParentDir)) {
        return Err(UpdateError::Root(format!("cannot open {}: an absolute directory is required", root.display())));
    }
    if !root.is_dir() {
        return Err(UpdateError::Root(format!("{} is not a directory", root.display())));
    }
    Ok(root.to_path_buf())
}

fn same_identity(existing: &Journal, fresh: &Journal) -> bool {
    if existing.manifest_version != fresh.manifest_version || existing.files.len() != fresh.files.len() {
        return false;
    }
    let files = existing
        .files
        .iter()
        .zip(&fresh.files)
        .all(|(old, new)| old.relative == new.relative && old.size == new.size && old.sha256 == new.sha256 && old.url == new.url);
    let official = match (&existing.official, &fresh.official) {
        (None, None) => true,
        (Some(old), Some(new)) => old.relative == new.relative && old.size == new.size && old.sha256 == new.sha256,
        _ => false,
    };
    files && official
}

fn journal_matches(bound: Option<&Journal>, disk: Option<&Journal>) -> bool {
    match (bound, disk) {
        (Some(bound), Some(disk)) => same_identity(bound, disk),
        (None, None) => true,
        _ => false,
    }
}

/// A known-empty object is created locally. Its digest is the empty SHA-256,
/// so no request is sent for a file the manifest already fully describes.
fn stage_parent(
    lease: &mut crate::pin::Lease,
    relative: &str,
) -> Result<(std::os::windows::io::OwnedHandle, std::ffi::OsString), UpdateError> {
    let safe = paths::parse_relative(relative)?;
    let mut parts: Vec<String> = safe.display().split('/').map(str::to_owned).collect();
    let name = parts.pop().ok_or_else(|| UpdateError::path(relative, "stage has no name"))?;
    parts.insert(0, "staging".into());
    Ok((lease.descend_from_work(&parts)?, name.into()))
}

fn materialize_empty(doc: &mut Journal, lease: &mut crate::pin::Lease) -> Result<(), UpdateError> {
    const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    for file in &mut doc.files {
        if file.size != 0 || matches!(file.phase, FilePhase::Installed | FilePhase::Staged) {
            continue;
        }
        if file.sha256 != EMPTY_SHA {
            continue;
        }
        let (parent, name) = stage_parent(lease, &file.relative)?;
        let existing = crate::pin::open_file_read(&parent, &name)?;
        if existing.as_ref().is_some_and(|file| file.size() != 0) {
            drop(existing);
            crate::pin::delete_file(&parent, &name)?;
        } else if existing.is_some() {
            file.phase = FilePhase::Staged;
            file.received = 0;
            continue;
        }
        let created = crate::pin::create_file(&parent, &name)?;
        crate::pin::flush(&created)?;
        file.phase = FilePhase::Staged;
        file.received = 0;
    }
    Ok(())
}

fn discard_foreign_stages(lease: &mut crate::pin::Lease, old: &Journal, fresh: &Journal) -> Result<(), UpdateError> {
    for previous in &old.files {
        let same = fresh.files.iter().find(|file| file.relative == previous.relative);
        let reusable = same.is_some_and(|file| file.size == previous.size && file.sha256 == previous.sha256);
        if reusable {
            continue;
        }
        let (parent, name) = stage_parent(lease, &previous.relative)?;
        crate::pin::delete_file(&parent, &name)?;
    }
    Ok(())
}

fn reconcile_stages<'a>(
    fresh: &mut Journal,
    existing: Option<&Journal>,
    lease_of: &mut dyn FnMut() -> Result<std::sync::MutexGuard<'a, crate::pin::Lease>, UpdateError>,
    mut on_file: impl FnMut(&str, u64, u64, u64),
) -> Result<(), UpdateError> {
    let old: std::collections::BTreeMap<_, _> =
        existing.into_iter().flat_map(|doc| &doc.files).map(|file| (file.relative.as_str(), file)).collect();
    let stage_total = fresh.files.iter().filter(|file| file.phase != FilePhase::Installed).count() as u64;
    let mut read_bytes = 0_u64;
    for (stage_done, file) in fresh.files.iter_mut().filter(|file| file.phase != FilePhase::Installed).enumerate() {
        let stage_done = stage_done as u64;
        on_file(&file.relative, stage_done, stage_total, read_bytes);
        let matching = old.get(file.relative.as_str()).copied().filter(|old| {
            old.size == file.size && old.sha256 == file.sha256 && old.url == file.url && (file.etag.is_none() || file.etag == old.etag)
        });
        let (parent, name) = {
            let mut lease = lease_of()?;
            stage_parent(&mut lease, &file.relative)?
        };
        let Some(mut staged) = crate::pin::open_file_read(&parent, &name)? else {
            on_file(&file.relative, stage_done + 1, stage_total, read_bytes);
            continue;
        };
        let matches = if staged.size() == file.size {
            let digest = staged.hash_with(|read| on_file(&file.relative, stage_done, stage_total, read_bytes + read))?;
            read_bytes += staged.size();
            Sha256::parse(&file.sha256)?.matches(&digest)
        } else {
            false
        };
        if matches {
            file.phase = FilePhase::Staged;
            file.received = file.size;
        } else if staged.size() < file.size && matching.is_some() {
            file.phase = FilePhase::Partial;
            file.received = staged.size();
        } else {
            drop(staged);
            crate::pin::delete_file(&parent, &name)?;
            on_file(&file.relative, stage_done + 1, stage_total, read_bytes);
            continue;
        }
        if file.etag.is_none()
            && let Some(old) = matching
        {
            file.etag.clone_from(&old.etag);
        }
        on_file(&file.relative, stage_done + 1, stage_total, read_bytes);
    }
    Ok(())
}

enum InstalledKind {
    Match,
    Missing,
    Different,
}

fn phase_of(doc: &Journal) -> SessionPhase {
    match doc.phase {
        JournalPhase::Planned => SessionPhase::Planned,
        JournalPhase::Downloading => SessionPhase::Downloading,
        JournalPhase::Staged => SessionPhase::Verifying,
        JournalPhase::Installing => SessionPhase::Installing,
        JournalPhase::Publishing => SessionPhase::Publishing,
        JournalPhase::Committed => SessionPhase::Idle,
    }
}

fn progress_of(doc: Option<&Journal>, phase: SessionPhase, paused: bool, transfers: &BTreeMap<String, FileProgress>) -> Progress {
    let Some(doc) = doc else {
        return Progress { phase, files_total: 0, files_completed: 0, bytes_total: 0, bytes_received: 0, paused };
    };
    let files_total = doc.files.len() as u64;
    let files_completed =
        doc.files.iter().filter(|file| file.phase == FilePhase::Installed || file.phase == FilePhase::Staged).count() as u64;
    let bytes_total = doc.files.iter().map(|file| file.size).sum();
    let bytes_received = doc
        .files
        .iter()
        .map(|file| transfers.get(&file.relative).map_or(file.received, |active| active.bytes_received).min(file.size))
        .sum();
    Progress { phase, files_total, files_completed, bytes_total, bytes_received, paused }
}
