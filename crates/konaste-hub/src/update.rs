//! One verified-title install or update at a time.
//!
//! Fresh discovery is fetched for every start, resume, and restart. The engine
//! runs on a worker thread with its own runtime because prepare, run, and
//! recover hash and replace files while holding the journal mutex. This module
//! never holds hub state across those calls, and it never starts the game.

use crate::{
    catalog::GameId,
    hub::Hub,
    platform,
    state::{CheckedFiles, GamePhase, GameSnapshot, HubError, HubState, Snapshot},
};
use konaste_bootstrap::GameFamily;
use konaste_install::UpdateSource;
use konaste_runtime::InstallationError;
use konaste_updater::{Progress, SessionPhase, UpdateError, UpdateHandle, Updater, UpdaterOptions};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crate::state::{PendingInstall, PendingIntent};

type Intent = PendingIntent;

/// Transient download failures retried automatically before the item surfaces
/// an error. The engine already absorbs short outages (it waits for the network
/// and backs off inside a run), so this outer bound only covers failures that
/// end the run outright. Keep it small.
const MAX_RETRIES: u8 = 2;
const RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(2);

struct Active {
    root: PathBuf,
    handle: Option<UpdateHandle>,
    pause_requested: bool,
}

struct Record {
    generation: u64,
    active: Option<Active>,
    /// Queue admission. Present while a title waits in the queue; promoted
    /// into `active` by the pump, or dropped by Remove.
    staged: bool,
}

pub struct UpdateSession {
    records: Mutex<BTreeMap<GameId, Record>>,
    // Serializes queue changes and admission. Never held across an await.
    admission: Mutex<()>,
    /// Set while a queue pump is scheduled, so one failure or finish wakes one
    /// pump only. The pump itself is the only place that starts a queued item.
    pumping: std::sync::atomic::AtomicBool,
}

impl Default for UpdateSession {
    fn default() -> Self {
        Self { records: Mutex::new(BTreeMap::new()), admission: Mutex::new(()), pumping: std::sync::atomic::AtomicBool::new(false) }
    }
}

#[derive(Debug)]
pub struct UpdateStamp {
    id: GameId,
    generation: u64,
    root: PathBuf,
}

impl UpdateSession {
    pub fn busy(&self, id: GameId) -> bool {
        self.records.lock().unwrap().get(&id).is_some_and(|record| record.active.is_some())
    }

    /// True while any title holds the engine. The queue runs one item at a time.
    pub fn any_active(&self) -> bool {
        self.records.lock().unwrap().values().any(|record| record.active.is_some())
    }

    #[cfg(test)]
    pub fn queued(&self, id: GameId) -> bool {
        self.records.lock().unwrap().get(&id).is_some_and(|record| record.staged)
    }

    pub fn reserved(&self, id: GameId) -> bool {
        self.records.lock().unwrap().get(&id).is_some_and(|record| record.active.is_some() || record.staged)
    }

    fn reserve(
        &self,
        state: &HubState,
        id: GameId,
        root: PathBuf,
        intent: Intent,
        expected_selected: Option<PathBuf>,
    ) -> Result<UpdateStamp, HubError> {
        // Same order as import: settings, checks, games, then this record map.
        let mut stored = state.stored.lock().unwrap();
        let mut checks = state.checks.lock().unwrap();
        let mut games = state.games.lock().unwrap();
        let mut records = self.records.lock().unwrap();
        check_root_conflict(&stored, &games, id, &root, intent)?;
        let game = games.iter_mut().find(|game| game.id == id).ok_or(HubError { code: "unsupportedTitle" })?;
        if game.phase.settings_busy() {
            return Err(HubError { code: "gameSettingsBusy" });
        }
        if matches!(game.phase, GamePhase::Running | GamePhase::Launching) {
            return Err(HubError { code: "gameRunning" });
        }
        if game.install_dir != expected_selected {
            return Err(HubError { code: "updateChanged" });
        }
        if records.values().any(|record| record.active.is_some()) {
            return Err(HubError { code: "updateBusy" });
        }
        let record = records.entry(id).or_insert(Record { generation: 0, active: None, staged: false });
        let next_check = checks.entry(id).or_default().checked_add(1).ok_or(HubError { code: "updateFailed" })?;
        let next_generation = record.generation.checked_add(1).ok_or(HubError { code: "updateFailed" })?;
        let resumable = stored.pending_installs.get(&id).is_some_and(|p| p.root == root && p.intent == intent && p.resumable);
        let mut next = stored.clone();
        next.pending_installs.insert(id, PendingInstall { root: root.clone(), intent, paused: false, resumable, expected_selected });
        next.download_queue.retain(|queued| *queued != id);
        state.save(&next)?;
        *stored = next;
        record.staged = false;
        record.generation = next_generation;
        record.active = Some(Active { root: root.clone(), handle: None, pause_requested: false });
        *checks.get_mut(&id).expect("check generation was inserted") = next_check;
        game.update_pending = true;
        game.transfer = None;
        game.work = None;
        Ok(UpdateStamp { id, generation: record.generation, root })
    }

    fn current(&self, stamp: &UpdateStamp) -> bool {
        let records = self.records.lock().unwrap();
        records.get(&stamp.id).is_some_and(|record| {
            record.generation == stamp.generation && record.active.as_ref().is_some_and(|active| same_file(&active.root, &stamp.root))
        })
    }

    fn finish(&self, stamp: &UpdateStamp) -> bool {
        let mut records = self.records.lock().unwrap();
        let Some(record) = records.get_mut(&stamp.id) else {
            return false;
        };
        if record.generation != stamp.generation {
            return false;
        }
        let matches = record.active.as_ref().is_some_and(|active| same_file(&active.root, &stamp.root));
        if matches {
            record.active = None;
        }
        matches
    }
}

pub async fn install(app: Hub, id: GameId, chosen: PathBuf) -> Result<Snapshot, HubError> {
    require_installable(id)?;
    let state = app.state();
    let _command = state.updates.admission.lock().unwrap();
    // Check the existing ancestor before creating anything. Recheck the actual
    // canonical destination and reserve it before releasing admission.
    let proposed = prospective_root(&chosen)?;
    validate_destination(state, id, &proposed)?;
    std::fs::create_dir_all(&chosen).map_err(|_| HubError { code: "filesystem" })?;
    let canonical = accept_destination(state, id, &chosen)?;
    let expected = game_of(state, id)?.install_dir;
    submit(&app, id, PendingIntent::Install, canonical, expected)
}

pub async fn start_selected(app: Hub, id: GameId) -> Result<Snapshot, HubError> {
    require_installable(id)?;
    let state = app.state();
    let _command = state.updates.admission.lock().unwrap();
    let game = game_of(state, id)?;
    let (intent, root, expected) = if let Some(pending) = pending_of(state, id) {
        // Older update records didn't store the selected root separately.
        let expected = pending.expected_selected.or_else(|| (pending.intent == Intent::Update).then(|| pending.root.clone()));
        (pending.intent, pending.root, expected)
    } else if game.install_dir.is_some()
        && (matches!(game.phase, GamePhase::Update | GamePhase::Paused)
            || matches!(game.error, Some("installationOutdated" | "installationInvalid")))
    {
        let root = game.install_dir.clone().ok_or(HubError { code: "notAnInstallation" })?;
        (PendingIntent::Update, root.clone(), Some(root))
    } else {
        return Err(HubError { code: "updateUnavailable" });
    };
    submit(&app, id, intent, root, expected)
}

/// Verify every installed file against fresh official metadata and replace
/// anything that differs. Same engine path as an update.
pub async fn repair(app: Hub, id: GameId) -> Result<Snapshot, HubError> {
    require_installable(id)?;
    let state = app.state();
    let _command = state.updates.admission.lock().unwrap();
    if pending_of(state, id).is_some() {
        return Err(HubError { code: "updateBusy" });
    }
    let game = game_of(state, id)?;
    let root = game.install_dir.ok_or(HubError { code: "notAnInstallation" })?;
    submit(&app, id, PendingIntent::Update, root.clone(), Some(root))
}

/// Free space on the volume that would hold `path`, for the install dialog.
pub fn free_space(path: &Path) -> Option<u64> {
    let mut probe = path.to_path_buf();
    while !probe.exists() {
        probe = probe.parent()?.to_path_buf();
    }
    crate::platform::free_space(&probe)
}

pub fn pause(app: &Hub, id: GameId) -> Result<(), HubError> {
    let state = app.state();
    let mut records = state.updates.records.lock().unwrap();
    let active = records.get_mut(&id).and_then(|record| record.active.as_mut()).ok_or(HubError { code: "noUpdate" })?;
    active.pause_requested = true;
    if let Some(handle) = &active.handle {
        handle.pause();
    }
    // The worker wakes the queue only after it has dropped the engine lease.
    Ok(())
}

pub fn resume_pending(state: &HubState) {
    let stored = state.stored.lock().unwrap();
    let pending = stored.pending_installs.clone();
    let queue = stored.download_queue.clone();
    drop(stored);
    let mut games = state.games.lock().unwrap();
    for (id, pending) in pending {
        let Some(game) = games.iter_mut().find(|game| game.id == id) else {
            continue;
        };
        if state.updates.busy(id) {
            continue;
        }
        game.phase = if pending.paused { GamePhase::Paused } else { GamePhase::Blocked };
        game.error = if pending.paused { None } else { Some("updateFailed") };
        game.progress = 0;
        game.transfer = None;
        game.work = None;
        game.update_pending = true;
    }
    // Queued items that never started are inert: no engine state to restore.
    for id in queue {
        let Some(game) = games.iter_mut().find(|game| game.id == id) else {
            continue;
        };
        if state.updates.busy(id) {
            continue;
        }
        game.phase = GamePhase::Queued;
        game.error = None;
        game.progress = 0;
        game.transfer = None;
        game.work = None;
        game.update_pending = true;
    }
}

pub fn startup_skipped(state: &HubState, id: GameId) -> bool {
    state.updates.reserved(id) || pending_of(state, id).is_some()
}

/// Accept an install/update/repair. It starts now when the engine is free,
/// otherwise it is appended to the persistent queue and starts in turn.
fn submit(app: &Hub, id: GameId, intent: Intent, root: PathBuf, expected_selected: Option<PathBuf>) -> Result<Snapshot, HubError> {
    let state = app.state();
    if state.updates.busy(id) || queued_of(state, id) {
        return Err(HubError { code: "updateBusy" });
    }
    ensure_idle(state, id, &root)?;
    if state.updates.any_active() || !state.queue().is_empty() {
        enqueue(app, id, intent, root, expected_selected)?;
        pump(app.clone());
        Ok(state.snapshot())
    } else {
        start(app.clone(), id, intent, root, expected_selected)
    }
}

/// Queue one item behind everything already waiting. The queue and the pending
/// record are written together, so a restart resumes the same order.
pub fn enqueue(app: &Hub, id: GameId, intent: Intent, root: PathBuf, expected_selected: Option<PathBuf>) -> Result<(), HubError> {
    let state = app.state();
    // Same order as reserve: settings, checks, games, records.
    let mut stored = state.stored.lock().unwrap();
    let mut checks = state.checks.lock().unwrap();
    let mut games = state.games.lock().unwrap();
    let mut records = state.updates.records.lock().unwrap();
    check_root_conflict(&stored, &games, id, &root, intent)?;
    let game = games.iter_mut().find(|game| game.id == id).ok_or(HubError { code: "unsupportedTitle" })?;
    if game.phase.settings_busy() {
        return Err(HubError { code: "gameSettingsBusy" });
    }
    if matches!(game.phase, GamePhase::Running | GamePhase::Launching) {
        return Err(HubError { code: "gameRunning" });
    }
    if game.install_dir != expected_selected {
        return Err(HubError { code: "updateChanged" });
    }
    let record = records.entry(id).or_insert(Record { generation: 0, active: None, staged: false });
    if record.active.is_some() || record.staged || stored.download_queue.contains(&id) {
        return Err(HubError { code: "updateBusy" });
    }
    let generation = checks.entry(id).or_default().checked_add(1).ok_or(HubError { code: "updateFailed" })?;
    let next_generation = record.generation.checked_add(1).ok_or(HubError { code: "updateFailed" })?;
    let (paused, resumable) = stored
        .pending_installs
        .get(&id)
        .filter(|p| p.root == root && p.intent == intent)
        .map_or((false, false), |p| (p.paused, p.resumable));
    let mut next = stored.clone();
    next.download_queue.push(id);
    next.pending_installs.insert(id, PendingInstall { root, intent, paused, resumable, expected_selected });
    state.save(&next)?;
    *stored = next;
    record.generation = next_generation;
    record.staged = true;
    *checks.get_mut(&id).expect("check generation was inserted") = generation;
    game.phase = GamePhase::Queued;
    game.error = None;
    game.progress = 0;
    game.transfer = None;
    game.work = None;
    game.update_pending = true;
    game.queue_position = Some(stored.download_queue.len());
    drop(records);
    drop(games);
    app.publish();
    Ok(())
}

/// Move a queued item to the front, or up one place.
pub fn reorder(app: &Hub, id: GameId, to_front: bool) -> Result<(), HubError> {
    let state = app.state();
    let _command = state.updates.admission.lock().unwrap();
    let mut stored = state.stored.lock().unwrap();
    let Some(ix) = stored.download_queue.iter().position(|queued| *queued == id) else {
        return Err(HubError { code: "noUpdate" });
    };
    let target = if to_front { 0 } else { ix.saturating_sub(1) };
    if target == ix {
        return Ok(());
    }
    let mut next = stored.clone();
    let moved = next.download_queue.remove(ix);
    next.download_queue.insert(target, moved);
    state.save(&next)?;
    *stored = next;
    drop(stored);
    app.publish();
    Ok(())
}

/// Remove a queued item that has not started. The active one is paused instead.
pub fn dequeue(app: &Hub, id: GameId) -> Result<(), HubError> {
    let state = app.state();
    let _command = state.updates.admission.lock().unwrap();
    // Only a title that is still waiting may be removed; the active one pauses.
    if state.updates.busy(id) {
        return Err(HubError { code: "updateBusy" });
    }
    let mut stored = state.stored.lock().unwrap();
    let mut games = state.games.lock().unwrap();
    if !stored.download_queue.contains(&id) {
        return Err(HubError { code: "noUpdate" });
    }
    let mut next = stored.clone();
    next.download_queue.retain(|queued| *queued != id);
    // Unqueueing a retry must not discard its previously staged work. An
    // install elsewhere must also keep the old selected root blocked.
    let retained = next.pending_installs.get(&id).is_some_and(|pending| {
        pending.paused || pending.resumable || (pending.intent == Intent::Install && pending.expected_selected.is_some())
    });
    if retained {
        next.pending_installs.get_mut(&id).expect("pending record retained").paused = true;
    } else {
        next.pending_installs.remove(&id);
    }
    state.save(&next)?;
    *stored = next;
    if let Some(record) = state.updates.records.lock().unwrap().get_mut(&id) {
        record.staged = false;
    }
    if let Some(game) = games.iter_mut().find(|game| game.id == id)
        && game.phase == GamePhase::Queued
    {
        game.phase = if retained {
            GamePhase::Paused
        } else if game.install_dir.is_some() {
            GamePhase::Update
        } else {
            GamePhase::Uninstalled
        };
        game.error = None;
        game.update_pending = retained;
        game.progress = 0;
        game.queue_position = None;
    }
    drop(games);
    drop(stored);
    app.publish();
    Ok(())
}

/// Abandon a fresh install without deleting any files. An existing selected
/// installation stays blocked until it is recovered or replaced elsewhere.
pub fn discard_install(app: &Hub, id: GameId) -> Result<(), HubError> {
    let state = app.state();
    let _command = state.updates.admission.lock().unwrap();
    let mut stored = state.stored.lock().unwrap();
    let mut games = state.games.lock().unwrap();
    let game = games.iter_mut().find(|game| game.id == id).ok_or(HubError { code: "unsupportedTitle" })?;
    if state.updates.reserved(id) || stored.download_queue.contains(&id) {
        return Err(HubError { code: "updateBusy" });
    }
    if stored.imports.contains_key(&id)
        || game.install_dir.is_some()
        || !stored.pending_installs.get(&id).is_some_and(|p| p.intent == Intent::Install && p.expected_selected.is_none())
    {
        return Err(HubError { code: "updateUnavailable" });
    }
    let mut next = stored.clone();
    next.pending_installs.remove(&id);
    state.save(&next)?;
    *stored = next;
    game.phase = GamePhase::Uninstalled;
    game.update_pending = false;
    game.error = None;
    game.progress = 0;
    game.transfer = None;
    game.work = None;
    game.queue_position = None;
    drop(games);
    drop(stored);
    app.publish();
    Ok(())
}

/// Startup hint: resume a queue that survived a restart. The pump re-checks
/// everything, so this cannot start a second item.
pub fn resume_queue(app: Hub) {
    if !app.state().queue().is_empty() {
        pump(app);
    }
}

/// Start the next queued item if the engine is free. Every caller only hints;
/// the pump re-checks admission itself, so a hint cannot double-start.
fn pump(app: Hub) {
    if app.state().updates.pumping.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let worker = app.clone();
    app.spawn(async move {
        let state = worker.state();
        let can_continue = pump_once(&worker);
        state.updates.pumping.store(false, std::sync::atomic::Ordering::SeqCst);
        // A completion may have hinted while this pump was still scheduled.
        if can_continue && !state.updates.any_active() && !state.queue().is_empty() {
            pump(worker);
        }
    });
}

fn pump_once(app: &Hub) -> bool {
    let state = app.state();
    let _command = state.updates.admission.lock().unwrap();
    while !state.updates.any_active() {
        let (id, pending) = {
            let stored = state.stored.lock().unwrap();
            let Some(&id) = stored.download_queue.first() else { break };
            (id, stored.pending_installs.get(&id).cloned())
        };
        let Some(pending) = pending else {
            if retire_queue_item(state, id).is_err() {
                return false;
            }
            continue;
        };
        let expected = pending.expected_selected.or_else(|| (pending.intent == Intent::Update).then(|| pending.root.clone()));
        match start(app.clone(), id, pending.intent, pending.root, expected) {
            Ok(_) => break,
            Err(error) if error.code == "updateBusy" => break,
            Err(error) => {
                if let Err(save_error) = retire_queue_item(state, id) {
                    app.notify(crate::hub::Notice::Error(save_error.code));
                    return false;
                }
                fail_queued(app, id, error.code);
            }
        }
    }
    true
}

/// Drop a queue order entry that no longer has admission.
fn retire_queue_item(state: &HubState, id: GameId) -> Result<(), HubError> {
    let mut stored = state.stored.lock().unwrap();
    let mut next = stored.clone();
    next.download_queue.retain(|queued| *queued != id);
    state.save(&next)?;
    *stored = next;
    if let Some(record) = state.updates.records.lock().unwrap().get_mut(&id) {
        record.staged = false;
    }
    Ok(())
}

fn fail_queued(app: &Hub, id: GameId, code: &'static str) {
    let state = app.state();
    let pending = state.stored.lock().unwrap().pending_installs.contains_key(&id);
    let mut games = state.games.lock().unwrap();
    if let Some(game) = games.iter_mut().find(|game| game.id == id) {
        game.phase = GamePhase::Blocked;
        game.error = Some(code);
        game.progress = 0;
        game.queue_position = None;
        game.update_pending = pending;
    }
    drop(games);
    app.notify(crate::hub::Notice::Error(code));
}

// Called with the queue admission lock held.
fn start(app: Hub, id: GameId, intent: Intent, root: PathBuf, expected_selected: Option<PathBuf>) -> Result<Snapshot, HubError> {
    let state = app.state();
    let family = id.runtime_family().ok_or(HubError { code: "unsupportedTitle" })?;
    // The machine-wide `launch_operation` is deliberately not taken here. It is
    // held for a game's whole run and for an open settings window, so taking it
    // would make one running title stall every queued download, and one
    // download block Play on unrelated titles. Files, the journal lease, and
    // the SDK worker are all per root, and the engine guards its own destination
    // immediately before every replacement. The same-title rule stays: an
    // active update keeps `update_pending` set, which `can_launch` rejects, and
    // `ensure_idle`/the mutation guard reject a running or settings process of
    // this title.
    ensure_idle(state, id, &root)?;
    let resumable = pending_of(state, id).is_some_and(|pending| pending.root == root && pending.intent == intent && pending.resumable);
    let stamp = state.updates.reserve(state, id, root, intent, expected_selected)?;
    let parallel = state.stored.lock().unwrap().preferences.parallel;
    let cache = state.directory.join("runtimes");
    publish_phase(&app, &stamp, GamePhase::Checking, 0, None);
    let worker = app.clone();
    app.spawn(async move {
        let mut cleanup = Cleanup { app: worker.clone(), stamp: Some(stamp.clone_stamp()) };
        let prepared = {
            let _metadata = worker.state().metadata_operation.lock().await;
            match crate::launch::ensure_components(&worker, family).await {
                Ok(()) => discover(id, &cache).await,
                Err(error) => Err(error),
            }
        };
        let metadata = match prepared {
            Ok(value) => value,
            Err(error) => {
                cleanup.fail(error.code);
                pump(worker);
                return;
            }
        };
        let app_for_worker = worker.clone();
        let stamp_for_worker = stamp.clone_stamp();
        let joined = tokio::task::spawn_blocking(move || {
            let mut cleanup = Cleanup { app: app_for_worker.clone(), stamp: Some(stamp_for_worker.clone_stamp()) };
            match drive(&app_for_worker, &stamp_for_worker, intent, resumable, metadata, parallel) {
                Ok(()) => {
                    cleanup.stamp.take();
                }
                Err(error) => cleanup.fail(error.code),
            }
        })
        .await;
        if joined.is_err() {
            cleanup.fail("updateFailed");
        } else {
            cleanup.stamp.take();
        }
        // Active slot is free (or paused): give the next queued item its turn.
        pump(worker);
    });
    Ok(app.state().snapshot())
}

struct Cleanup {
    app: Hub,
    stamp: Option<UpdateStamp>,
}

impl Cleanup {
    fn fail(&mut self, code: &'static str) {
        if let Some(stamp) = self.stamp.take() {
            fail(&self.app, &stamp, code);
        }
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        self.fail("updateFailed");
    }
}

fn drive(
    app: &Hub,
    stamp: &UpdateStamp,
    intent: Intent,
    resumable: bool,
    metadata: konaste_discovery::CurrentMetadata,
    parallel: usize,
) -> Result<(), HubError> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_| HubError { code: "updateFailed" })?;
    let state = app.state();
    if !state.updates.current(stamp) {
        return Err(HubError { code: "updateChanged" });
    }
    let mut metadata = metadata;
    let mut resumable = resumable;
    let mut failures = 0_u8;
    let mut progress = 0;
    let (family, revision) = loop {
        if !state.updates.current(stamp) {
            return Err(HubError { code: "updateChanged" });
        }
        if pause_requested(state, stamp) {
            return finish_paused(app, stamp, intent, resumable, progress);
        }
        let source = UpdateSource::from_metadata(metadata).map_err(|_| HubError { code: "metadataInvalid" })?;
        if !stamp.id.admits(source.family()) {
            return Err(HubError { code: "metadataInvalid" });
        }
        let options = UpdaterOptions::new(source.download_policy()).parallel(parallel).map_err(|_| HubError { code: "invalidSettings" })?;
        let updater = open_target(&stamp.root, options, intent, resumable).map_err(map_engine)?;
        if !resumable {
            remember_pending(state, stamp.id, &stamp.root, intent, false, true)?;
            resumable = true;
        }
        let handle = updater.handle();
        attach_handle(state, stamp, handle.clone())?;
        publish_phase(app, stamp, GamePhase::Preparing, 0, None);
        let mut watcher = ProgressWatch::start(app.clone(), stamp.clone_stamp(), handle.clone());
        source.prepare(&updater).map_err(map_engine)?;
        let ran = if pause_requested(state, stamp) {
            Ok(SessionPhase::Paused)
        } else if updater.recover_guarded(mutation_guard(app, stamp)).map_err(map_engine)?.needs_run {
            publish_phase(app, stamp, GamePhase::Downloading, 0, None);
            runtime.block_on(updater.run_guarded(mutation_guard(app, stamp)))
        } else {
            Ok(SessionPhase::Idle)
        };
        watcher.stop();
        let transfer = handle.transfer_progress();
        progress = progress_percent(&transfer.progress);
        publish_transfer(app, stamp, &transfer.progress, crate::transfer::Snapshot::stopped(&transfer));
        // The UI handle owns the same filesystem lease as the engine. Release
        // every reference before opening a replacement session.
        clear_handle(state, stamp);
        drop(handle);
        drop(updater);
        match ran.map_err(map_engine) {
            Ok(SessionPhase::Idle) => break (source.family(), source.revision()),
            Ok(SessionPhase::Paused) => return finish_paused(app, stamp, intent, resumable, progress),
            Err(error) if error.code == "downloadFailed" && failures < MAX_RETRIES => {
                failures += 1;
                let delay = RETRY_DELAY.mul_f32(f32::from(failures));
                if wait_for_retry(state, stamp, delay)? {
                    return finish_paused(app, stamp, intent, resumable, progress);
                }
                let next = discover_blocking(stamp.id, &state.directory.join("runtimes"));
                if pause_requested(state, stamp) {
                    return finish_paused(app, stamp, intent, resumable, progress);
                }
                metadata = next?;
            }
            Err(error) => return Err(error),
            Ok(_) => return Err(HubError { code: "updateFailed" }),
        }
    };
    publish_phase(app, stamp, GamePhase::Finalizing, 0, None);
    let fresh = discover_blocking(stamp.id, &state.directory.join("runtimes"))?;
    if !rediscovery_matches(stamp.id, family, revision, fresh.family(), fresh.index.revision) {
        return Err(HubError { code: "updateRequired" });
    }
    let (pin, settings) = fresh.installation_pins().map_err(|_| HubError { code: "metadataInvalid" })?;
    let installed = konaste_runtime::inspect_installation(fresh.family(), &stamp.root, &pin).map_err(map_installation)?;
    if !stamp.id.admits(installed.family()) || installed.product_version().is_empty() {
        return Err(HubError { code: "installationInvalid" });
    }
    let version = installed.product_version().to_string();
    bind_completed(state, stamp, revision, version, CheckedFiles { installation: pin, settings })?;
    app.notify(crate::hub::Notice::Installed(stamp.id));
    Ok(())
}

struct ProgressWatch {
    stop: Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl ProgressWatch {
    fn start(app: Hub, stamp: UpdateStamp, handle: UpdateHandle) -> Self {
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let worker = std::thread::spawn(move || {
            let mut meter = crate::transfer::Meter::default();
            while !flag.load(std::sync::atomic::Ordering::SeqCst) {
                let work = handle.work();
                if matches!(work.phase, SessionPhase::Downloading | SessionPhase::Paused) {
                    if let Some(transfer) = handle.try_transfer_progress() {
                        let snapshot = meter.sample(&transfer, std::time::Instant::now());
                        publish_transfer(&app, &stamp, &transfer.progress, snapshot);
                    }
                } else {
                    meter = crate::transfer::Meter::default();
                    publish_work(&app, &stamp, &work);
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        });
        Self { stop, worker: Some(worker) }
    }

    fn stop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for ProgressWatch {
    fn drop(&mut self) {
        self.stop();
    }
}

fn bind_completed(state: &HubState, stamp: &UpdateStamp, revision: u32, version: String, files: CheckedFiles) -> Result<(), HubError> {
    let mut stored = state.stored.lock().unwrap();
    let mut checks = state.checks.lock().unwrap();
    let mut games = state.games.lock().unwrap();
    let mut records = state.updates.records.lock().unwrap();
    let record = records.get_mut(&stamp.id).ok_or(HubError { code: "updateChanged" })?;
    let matches =
        record.generation == stamp.generation && record.active.as_ref().is_some_and(|active| same_file(&active.root, &stamp.root));
    if !matches {
        return Err(HubError { code: "updateChanged" });
    }
    let next_check = checks.entry(stamp.id).or_default().checked_add(1).ok_or(HubError { code: "updateFailed" })?;
    let mut next = stored.clone();
    next.imports.insert(stamp.id, stamp.root.clone());
    next.hidden.remove(&stamp.id);
    next.pending_installs.remove(&stamp.id);
    next.download_queue.retain(|queued| *queued != stamp.id);
    state.save(&next)?;
    *stored = next;
    *checks.get_mut(&stamp.id).expect("check generation was inserted") = next_check;
    state.evidence.lock().unwrap().insert(stamp.id, (next_check, files));
    let game = games.iter_mut().find(|game| game.id == stamp.id).ok_or(HubError { code: "unsupportedTitle" })?;
    game.install_dir = Some(stamp.root.clone());
    game.installed_version = Some(version.clone());
    game.available_version = Some(version);
    game.resource_revision = Some(revision);
    game.runtime_ready = true;
    game.launch_supported = game.id.launch_supported();
    if game.launch_supported {
        game.phase = GamePhase::Ready;
        game.error = None;
    } else {
        game.phase = GamePhase::Blocked;
        game.error = Some("launchUnsupported");
    }
    game.progress = 100;
    game.transfer = None;
    game.work = None;
    game.update_pending = false;
    game.detected_dir = None;
    game.queue_position = None;
    record.active = None;
    Ok(())
}

fn publish_phase(app: &Hub, stamp: &UpdateStamp, phase: GamePhase, progress: u8, error: Option<&'static str>) {
    if apply_phase(app.state(), stamp, phase, progress, error) {
        app.publish();
    }
}

fn apply_phase(state: &HubState, stamp: &UpdateStamp, phase: GamePhase, progress: u8, error: Option<&'static str>) -> bool {
    let mut games = state.games.lock().unwrap();
    let records = state.updates.records.lock().unwrap();
    if !records.get(&stamp.id).is_some_and(|record| {
        record.generation == stamp.generation && record.active.as_ref().is_some_and(|active| active.root == stamp.root)
    }) {
        return false;
    }
    let Some(game) = games.iter_mut().find(|game| game.id == stamp.id) else {
        return false;
    };
    if phase != GamePhase::Downloading
        && let Some(transfer) = &mut game.transfer
    {
        transfer.stop();
    }
    game.phase = phase;
    game.progress = progress;
    game.error = error;
    game.update_pending = true;
    game.work = None;
    game.queue_position = None;
    true
}

fn publish_transfer(app: &Hub, stamp: &UpdateStamp, progress: &Progress, transfer: crate::transfer::Snapshot) {
    let state = app.state();
    if apply_transfer(state, stamp, progress, transfer) {
        app.publish();
    }
}

fn publish_work(app: &Hub, stamp: &UpdateStamp, work: &konaste_updater::WorkSnapshot) {
    if apply_work(app.state(), stamp, work) {
        app.publish();
    }
}

fn apply_work(state: &HubState, stamp: &UpdateStamp, work: &konaste_updater::WorkSnapshot) -> bool {
    let phase = match work.phase {
        SessionPhase::Preparing => GamePhase::Preparing,
        SessionPhase::Verifying => GamePhase::Verifying,
        SessionPhase::Installing | SessionPhase::Publishing => GamePhase::Installing,
        SessionPhase::Recovering => GamePhase::Recovering,
        _ => return false,
    };
    let mut games = state.games.lock().unwrap();
    let records = state.updates.records.lock().unwrap();
    if !records.get(&stamp.id).is_some_and(|record| {
        record.generation == stamp.generation && record.active.as_ref().is_some_and(|active| active.root == stamp.root)
    }) {
        return false;
    }
    let Some(game) = games.iter_mut().find(|game| game.id == stamp.id) else {
        return false;
    };
    let use_bytes = matches!(phase, GamePhase::Preparing | GamePhase::Verifying) && work.bytes_total > 0;
    let (done, total) = if use_bytes { (work.bytes_completed, work.bytes_total) } else { (work.files_completed, work.files_total) };
    game.progress = if total == 0 { 0 } else { ((u128::from(done) * 100 / u128::from(total)).min(100)) as u8 };
    game.phase = phase;
    game.error = None;
    game.update_pending = true;
    if let Some(transfer) = &mut game.transfer {
        transfer.stop();
    }
    game.work = Some(crate::transfer::Work::from(work));
    true
}

fn apply_transfer(state: &HubState, stamp: &UpdateStamp, progress: &Progress, transfer: crate::transfer::Snapshot) -> bool {
    let phase = match progress.phase {
        SessionPhase::Downloading => GamePhase::Downloading,
        SessionPhase::Paused => GamePhase::Paused,
        SessionPhase::Verifying => GamePhase::Verifying,
        SessionPhase::Installing | SessionPhase::Publishing => GamePhase::Installing,
        SessionPhase::Recovering => GamePhase::Recovering,
        _ => return false,
    };
    let mut games = state.games.lock().unwrap();
    let records = state.updates.records.lock().unwrap();
    let current = records.get(&stamp.id).is_some_and(|record| {
        record.generation == stamp.generation && record.active.as_ref().is_some_and(|active| active.root == stamp.root)
    });
    if !current {
        return false;
    }
    let Some(game) = games.iter_mut().find(|game| game.id == stamp.id) else {
        return false;
    };
    game.phase = phase;
    game.progress = progress_percent(progress);
    game.error = None;
    game.update_pending = true;
    game.transfer = Some(transfer);
    game.work = None;
    true
}

fn pause_requested(state: &HubState, stamp: &UpdateStamp) -> bool {
    state
        .updates
        .records
        .lock()
        .unwrap()
        .get(&stamp.id)
        .is_some_and(|record| record.generation == stamp.generation && record.active.as_ref().is_some_and(|active| active.pause_requested))
}

fn wait_for_retry(state: &HubState, stamp: &UpdateStamp, delay: std::time::Duration) -> Result<bool, HubError> {
    let until = std::time::Instant::now() + delay;
    loop {
        if !state.updates.current(stamp) {
            return Err(HubError { code: "updateChanged" });
        }
        if pause_requested(state, stamp) {
            return Ok(true);
        }
        let left = until.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Ok(false);
        }
        std::thread::sleep(left.min(std::time::Duration::from_millis(50)));
    }
}

fn finish_paused(app: &Hub, stamp: &UpdateStamp, intent: Intent, resumable: bool, progress: u8) -> Result<(), HubError> {
    let state = app.state();
    if !state.updates.current(stamp) {
        return Err(HubError { code: "updateChanged" });
    }
    remember_pending(state, stamp.id, &stamp.root, intent, true, resumable)?;
    apply_phase(state, stamp, GamePhase::Paused, progress, None);
    state.updates.finish(stamp);
    app.publish();
    Ok(())
}

fn mutation_guard<'a>(app: &'a Hub, stamp: &'a UpdateStamp) -> impl FnMut() -> Result<(), UpdateError> + 'a {
    move || {
        let state = app.state();
        if !state.updates.current(stamp) {
            return Err(UpdateError::operation_rejected("updateChanged"));
        }
        let image = stamp.id.module_image(&stamp.root).ok_or_else(|| UpdateError::operation_rejected("unsupportedTitle"))?;
        if platform::game_running(&image).unwrap_or(true) {
            return Err(UpdateError::operation_rejected("gameRunning"));
        }
        crate::game_settings::ensure_closed(stamp.id, &stamp.root).map_err(|error| UpdateError::operation_rejected(error.code))
    }
}

fn attach_handle(state: &HubState, stamp: &UpdateStamp, handle: UpdateHandle) -> Result<(), HubError> {
    let mut records = state.updates.records.lock().unwrap();
    let active = records
        .get_mut(&stamp.id)
        .and_then(|record| if record.generation == stamp.generation { record.active.as_mut() } else { None })
        .ok_or(HubError { code: "updateChanged" })?;
    if active.root != stamp.root {
        return Err(HubError { code: "updateChanged" });
    }
    if active.pause_requested {
        handle.pause();
    }
    active.handle = Some(handle);
    Ok(())
}

fn clear_handle(state: &HubState, stamp: &UpdateStamp) {
    let mut records = state.updates.records.lock().unwrap();
    if let Some(record) = records.get_mut(&stamp.id)
        && record.generation == stamp.generation
        && let Some(active) = record.active.as_mut()
        && active.root == stamp.root
    {
        active.handle = None;
    }
}

fn open_target(root: &Path, options: konaste_updater::UpdaterOptions, intent: Intent, resumable: bool) -> Result<Updater, UpdateError> {
    if intent == PendingIntent::Install && !resumable {
        Updater::open_unstarted_with(root, options).map_err(|error| match error {
            UpdateError::Root(_) => UpdateError::operation_rejected("destinationNotEmpty"),
            error => error,
        })
    } else {
        Updater::open_with(root, options)
    }
}

fn ensure_fresh_root(root: &Path) -> Result<(), HubError> {
    if std::fs::read_dir(root).map_err(|_| HubError { code: "notAnInstallation" })?.next().is_some() {
        return Err(HubError { code: "destinationNotEmpty" });
    }
    Ok(())
}

fn fail(app: &Hub, stamp: &UpdateStamp, code: &'static str) {
    let state = app.state();
    if state.updates.current(stamp) {
        let mut games = state.games.lock().unwrap();
        if let Some(game) = games.iter_mut().find(|game| game.id == stamp.id) {
            game.phase = GamePhase::Blocked;
            game.error = Some(code);
            game.update_pending = true;
            game.queue_position = None;
            if let Some(transfer) = &mut game.transfer {
                transfer.stop();
                game.progress = if transfer.bytes_total == 0 {
                    0
                } else {
                    ((u128::from(transfer.bytes_received) * 100 / u128::from(transfer.bytes_total)).min(100)) as u8
                };
            }
            game.work = None;
        }
        drop(games);
        state.updates.finish(stamp);
        app.publish();
    }
}

fn ensure_idle(state: &HubState, id: GameId, root: &Path) -> Result<(), HubError> {
    let game = game_of(state, id)?;
    if game.phase.settings_busy() {
        return Err(HubError { code: "gameSettingsBusy" });
    }
    if matches!(game.phase, GamePhase::Running | GamePhase::Launching) {
        return Err(HubError { code: "gameRunning" });
    }
    crate::game_settings::ensure_closed(id, root)?;
    let image = id.module_image(root).ok_or(HubError { code: "unsupportedTitle" })?;
    if platform::game_running(&image)? {
        return Err(HubError { code: "gameRunning" });
    }
    Ok(())
}

fn prospective_root(root: &Path) -> Result<PathBuf, HubError> {
    if !root.is_absolute() || root.components().any(|part| matches!(part, std::path::Component::ParentDir)) {
        return Err(HubError { code: "destinationRejected" });
    }
    match std::fs::canonicalize(root) {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = root.parent().ok_or(HubError { code: "filesystem" })?;
            let name = root.file_name().ok_or(HubError { code: "destinationRejected" })?;
            Ok(prospective_root(parent)?.join(name))
        }
        Err(_) => Err(HubError { code: "filesystem" }),
    }
}

fn accept_destination(state: &HubState, id: GameId, root: &Path) -> Result<PathBuf, HubError> {
    let canonical = std::fs::canonicalize(root).map_err(|_| HubError { code: "notAnInstallation" })?;
    ensure_fresh_root(&canonical)?;
    validate_destination(state, id, &canonical)?;
    Ok(canonical)
}

fn validate_destination(state: &HubState, id: GameId, root: &Path) -> Result<(), HubError> {
    if !crate::official_setup::title_paths_ok(id, root) {
        return Err(HubError { code: "destinationPathUnsupported" });
    }
    let stored = state.stored.lock().unwrap();
    let games = state.games.lock().unwrap();
    if stored.pending_installs.get(&id).is_some_and(|pending| related(root, &pending.root)) {
        return Err(HubError { code: "destinationRejected" });
    }
    check_root_conflict(&stored, &games, id, root, Intent::Install)
}

fn check_root_conflict(
    stored: &crate::state::StoredData,
    games: &[GameSnapshot],
    id: GameId,
    root: &Path,
    intent: Intent,
) -> Result<(), HubError> {
    let registered = stored.imports.iter().filter(|(at, _)| **at != id || intent != Intent::Update).map(|(_, path)| path.as_path());
    let pending = stored.pending_installs.iter().filter(|(at, _)| **at != id).map(|(_, pending)| pending.root.as_path());
    let detected = games.iter().filter(|game| game.id != id || intent != Intent::Update).filter_map(|game| game.detected_dir.as_deref());
    if registered.chain(pending).chain(detected).any(|path| related(root, path)) {
        return Err(HubError { code: "destinationRejected" });
    }
    Ok(())
}

pub(crate) fn related(candidate: &Path, existing: &Path) -> bool {
    let existing = std::fs::canonicalize(existing).unwrap_or_else(|_| existing.to_path_buf());
    #[cfg(windows)]
    let (candidate, existing) =
        (PathBuf::from(candidate.to_string_lossy().to_lowercase()), PathBuf::from(existing.to_string_lossy().to_lowercase()));
    candidate.starts_with(&existing) || existing.starts_with(&candidate)
}

fn remember_pending(state: &HubState, id: GameId, root: &Path, intent: Intent, paused: bool, resumable: bool) -> Result<(), HubError> {
    let mut stored = state.stored.lock().unwrap();
    let mut next = stored.clone();
    // Preserve the selected root recorded at admission. An older record has
    // none, and `start` already validated its own expected root.
    let expected_selected = next.pending_installs.get(&id).and_then(|pending| pending.expected_selected.clone());
    next.pending_installs.insert(id, PendingInstall { root: root.to_path_buf(), intent, paused, resumable, expected_selected });
    // Starting, pausing, or failing all end this title's wait: it leaves the
    // queue exactly when it gains a pending record. A paused item is resumed
    // explicitly, not by the queue.
    next.download_queue.retain(|queued| *queued != id);
    state.save(&next)?;
    *stored = next;
    Ok(())
}

fn queued_of(state: &HubState, id: GameId) -> bool {
    state.stored.lock().unwrap().download_queue.contains(&id)
}

fn pending_of(state: &HubState, id: GameId) -> Option<PendingInstall> {
    state.stored.lock().unwrap().pending_installs.get(&id).cloned()
}

async fn discover(id: GameId, cache: &Path) -> Result<konaste_discovery::CurrentMetadata, HubError> {
    let cache = cache.to_path_buf();
    tokio::task::spawn_blocking(move || discover_blocking(id, &cache)).await.map_err(|_| HubError { code: "metadataCheckFailed" })?
}

fn discover_blocking(id: GameId, cache: &Path) -> Result<konaste_discovery::CurrentMetadata, HubError> {
    let family = id.runtime_family().filter(|_| id.title().is_some()).ok_or(HubError { code: "unsupportedTitle" })?;
    let executable = std::env::current_exe().map_err(|_| HubError { code: "metadataCheckFailed" })?;
    let metadata = konaste_discovery::check(family, cache, &executable).map_err(|error| HubError {
        code: match error {
            konaste_discovery::DiscoveryError::ComponentsMissing => "componentsMissing",
            konaste_discovery::DiscoveryError::Runtime => "componentsFailed",
            konaste_discovery::DiscoveryError::InvalidMetadata | konaste_discovery::DiscoveryError::Protocol => "metadataInvalid",
            konaste_discovery::DiscoveryError::Unsupported => "unsupportedTitle",
            _ => "metadataCheckFailed",
        },
    })?;
    if !id.admits(metadata.family()) {
        return Err(HubError { code: "metadataInvalid" });
    }
    Ok(metadata)
}

fn game_of(state: &HubState, id: GameId) -> Result<GameSnapshot, HubError> {
    state.games.lock().unwrap().iter().find(|game| game.id == id).cloned().ok_or(HubError { code: "unsupportedTitle" })
}

fn rediscovery_matches(id: GameId, source_family: GameFamily, source_revision: u32, fresh_family: GameFamily, fresh_revision: u32) -> bool {
    id.admits(source_family) && id.admits(fresh_family) && fresh_revision == source_revision
}

fn require_installable(id: GameId) -> Result<(), HubError> {
    id.title().ok_or(HubError { code: "unsupportedTitle" })?;
    Ok(())
}

fn progress_percent(progress: &Progress) -> u8 {
    if progress.bytes_total == 0 {
        return 0;
    }
    ((progress.bytes_received.saturating_mul(100)) / progress.bytes_total).min(100) as u8
}

fn same_file(left: &Path, right: &Path) -> bool {
    left == right
}

fn map_engine(error: UpdateError) -> HubError {
    HubError {
        code: match error {
            UpdateError::OperationRejected(reason) if reason == "gameRunning" => "gameRunning",
            UpdateError::OperationRejected(reason) if reason == "gameSettingsBusy" => "gameSettingsBusy",
            UpdateError::OperationRejected(reason) if reason == "unsupportedTitle" => "unsupportedTitle",
            UpdateError::OperationRejected(reason) if reason == "destinationNotEmpty" => "destinationNotEmpty",
            UpdateError::OperationRejected(_) => "updateChanged",
            UpdateError::RecoveryRequired => "updateRecoveryRequired",
            UpdateError::PlanChanged => "updateChanged",
            UpdateError::DiskSpace { .. } => "diskSpace",
            UpdateError::Policy { .. } | UpdateError::Manifest(_) => "metadataInvalid",
            UpdateError::Download { .. } => "downloadFailed",
            UpdateError::Staging { .. } | UpdateError::Install { .. } => "installationInvalid",
            _ => "updateFailed",
        },
    }
}

fn map_installation(error: InstallationError) -> HubError {
    HubError {
        code: match error {
            InstallationError::Outdated => "installationOutdated",
            InstallationError::Missing | InstallationError::Invalid => "installationInvalid",
            InstallationError::Unsupported => "unsupportedTitle",
        },
    }
}

trait StampClone {
    fn clone_stamp(&self) -> Self;
}

impl StampClone for UpdateStamp {
    fn clone_stamp(&self) -> Self {
        Self { id: self.id, generation: self.generation, root: self.root.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{catalog::GameId, hub::Hub, state::HubState};

    /// A hub whose state lives in a fresh temp directory. No detection, no
    /// network, no engine start: only order and persistence are exercised.
    fn hub() -> (tempfile::TempDir, Hub) {
        let directory = tempfile::tempdir().unwrap();
        let mut state = HubState::load_without_detection(directory.path().to_path_buf()).unwrap();
        state.maintenance = crate::maintenance::Maintenance::with_clock(|| 0);
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let _enter = runtime.enter();
        let (hub, _receivers) = Hub::new(state, runtime.handle().clone());
        std::mem::forget(runtime);
        (directory, hub)
    }

    #[test]
    fn switching_the_gitadora_instrument_keeps_the_check_and_play_reads_the_new_choice() {
        use konaste_runtime::GitadoraMode;
        let (directory, hub) = hub();
        let state = hub.state();
        let root = directory.path().join("GITADORA");
        std::fs::create_dir_all(root.join("game/modules")).unwrap();
        std::fs::write(root.join("game/modules/gitadora.exe"), b"x").unwrap();
        state.import(GameId::Gitadora, root).unwrap();
        let mut preferences = state.snapshot().preferences;
        preferences.gitadora_mode = Some(GitadoraMode::GuitarFreaks);
        state.commit_preferences(preferences.clone()).unwrap();
        let ticket = state.begin_check(GameId::Gitadora).unwrap();
        let pin = konaste_runtime::InstallationPin { index_sha256: [1; 32], module_sha256: [2; 32], module_bytes: 1 };
        assert!(state.finish_current_check(
            ticket,
            2026062300,
            "U32:J:A:A:2026062300".into(),
            CheckedFiles { installation: pin, settings: None }
        ));
        let ready = |state: &HubState| state.snapshot().games.into_iter().find(|g| g.id == GameId::Gitadora).unwrap();
        assert_eq!(ready(state).phase, GamePhase::Ready);

        preferences.gitadora_mode = Some(GitadoraMode::DrumMania);
        state.commit_preferences(preferences).unwrap();
        let game = ready(state);
        assert_eq!(game.phase, GamePhase::Ready, "no new check after switching");
        assert!(game.error.is_none());
        let (stamp, _) = state.ready_evidence(GameId::Gitadora).unwrap();
        assert_eq!(stamp.mode(), Some(GitadoraMode::DrumMania));
    }

    fn staged(root: &str) -> (PathBuf, Option<PathBuf>) {
        (PathBuf::from(root), None)
    }

    #[test]
    fn queue_keeps_submission_order_and_reorders_to_front_and_up() {
        let (_directory, hub) = hub();
        let state = hub.state();
        for (id, root) in [(GameId::Sdvx, r"C:\a"), (GameId::Ddr, r"C:\b"), (GameId::Popn, r"C:\c")] {
            let (root, expected) = staged(root);
            enqueue(&hub, id, PendingIntent::Install, root, expected).unwrap();
        }
        assert_eq!(state.queue(), vec![GameId::Sdvx, GameId::Ddr, GameId::Popn]);
        reorder(&hub, GameId::Popn, true).unwrap();
        assert_eq!(state.queue(), vec![GameId::Popn, GameId::Sdvx, GameId::Ddr]);
        reorder(&hub, GameId::Ddr, false).unwrap();
        assert_eq!(state.queue(), vec![GameId::Popn, GameId::Ddr, GameId::Sdvx]);
        reorder(&hub, GameId::Popn, false).unwrap();
        assert_eq!(state.queue(), vec![GameId::Popn, GameId::Ddr, GameId::Sdvx]);
        let snapshot = state.snapshot();
        let popn = snapshot.games.iter().find(|game| game.id == GameId::Popn).unwrap();
        assert_eq!(popn.phase, GamePhase::Queued);
        assert_eq!(popn.queue_position, Some(1));
    }

    #[test]
    fn a_queued_title_joins_the_rail_and_cannot_be_queued_twice() {
        let (_directory, hub) = hub();
        let (root, expected) = staged(r"C:\a");
        enqueue(&hub, GameId::Sdvx, PendingIntent::Install, root, expected).unwrap();
        let (root, expected) = staged(r"C:\a");
        assert_eq!(enqueue(&hub, GameId::Sdvx, PendingIntent::Install, root, expected).err().unwrap().code, "updateBusy");
        let snapshot = hub.state().snapshot();
        let sdvx = snapshot.games.iter().find(|game| game.id == GameId::Sdvx).unwrap();
        assert!(sdvx.install_dir.is_none() && sdvx.update_pending);
    }

    #[test]
    fn removing_a_queued_item_drops_its_pending_record_and_resets_the_phase() {
        let (_directory, hub) = hub();
        let state = hub.state();
        let (root, expected) = staged(r"C:\a");
        enqueue(&hub, GameId::Sdvx, PendingIntent::Install, root, expected).unwrap();
        dequeue(&hub, GameId::Sdvx).unwrap();
        assert!(state.queue().is_empty());
        assert!(!state.stored.lock().unwrap().pending_installs.contains_key(&GameId::Sdvx));
        let sdvx = state.snapshot().games.into_iter().find(|game| game.id == GameId::Sdvx).unwrap();
        assert_eq!(sdvx.phase, GamePhase::Uninstalled);
        assert!(!sdvx.update_pending);
    }

    #[test]
    fn the_queue_and_pending_records_survive_a_reload_in_order() {
        let (directory, hub) = hub();
        for (id, root) in [(GameId::Sdvx, r"C:\a"), (GameId::Ddr, r"C:\b")] {
            let (root, expected) = staged(root);
            enqueue(&hub, id, PendingIntent::Install, root, expected).unwrap();
        }
        let queue = hub.state().queue();
        let reloaded = HubState::load_without_detection(directory.path().to_path_buf()).unwrap();
        assert_eq!(reloaded.queue(), queue);
        let pending = reloaded.stored.lock().unwrap().pending_installs.clone();
        assert_eq!(pending.get(&GameId::Sdvx).unwrap().root, PathBuf::from(r"C:\a"));
        assert_eq!(pending.get(&GameId::Ddr).unwrap().root, PathBuf::from(r"C:\b"));
        let snapshot = reloaded.snapshot();
        let sdvx = snapshot.games.iter().find(|game| game.id == GameId::Sdvx).unwrap();
        assert_eq!(sdvx.phase, GamePhase::Queued);
        assert_eq!(sdvx.queue_position, Some(1));
    }

    #[test]
    fn a_paused_pending_install_does_not_join_the_queue_as_a_waiting_item() {
        let (directory, hub) = hub();
        let state = hub.state();
        let mut stored = state.stored.lock().unwrap();
        let mut next = stored.clone();
        next.pending_installs.insert(
            GameId::Sdvx,
            PendingInstall {
                root: PathBuf::from(r"C:\staged"),
                intent: PendingIntent::Update,
                paused: true,
                resumable: true,
                expected_selected: None,
            },
        );
        state.save(&next).unwrap();
        *stored = next;
        drop(stored);
        let reloaded = HubState::load_without_detection(directory.path().to_path_buf()).unwrap();
        assert!(reloaded.queue().is_empty());
        let sdvx = reloaded.snapshot().games.into_iter().find(|game| game.id == GameId::Sdvx).unwrap();
        assert_eq!(sdvx.phase, GamePhase::Paused);
        assert_eq!(sdvx.queue_position, None);
    }

    #[test]
    fn pause_during_retry_persists_the_pause_and_releases_the_queue() {
        let (directory, hub) = hub();
        let state = hub.state();
        let id = GameId::Sdvx;
        let root = directory.path().join("game");
        let stamp = state.updates.reserve(state, id, root.clone(), Intent::Install, None).unwrap();
        publish_phase(&hub, &stamp, GamePhase::Downloading, 13, None);
        // Backoff has no attached handle: the intent has to survive on the slot.
        state.updates.records.lock().unwrap().get_mut(&id).unwrap().active.as_mut().unwrap().pause_requested = true;
        assert!(wait_for_retry(state, &stamp, RETRY_DELAY).unwrap());
        finish_paused(&hub, &stamp, Intent::Install, true, 13).unwrap();
        assert!(!state.updates.any_active());
        assert_eq!(game_of(state, id).unwrap().phase, GamePhase::Paused);
        let reloaded = HubState::load_without_detection(directory.path().to_path_buf()).unwrap();
        let pending = pending_of(&reloaded, id).unwrap();
        assert_eq!(pending.root, root);
        assert!(pending.paused && pending.resumable);
    }

    #[test]
    fn retry_releases_the_saved_progress_handle_before_reopening() {
        let (directory, hub) = hub();
        let state = hub.state();
        let root = directory.path().join("retry");
        std::fs::create_dir(&root).unwrap();
        let stamp = state.updates.reserve(state, GameId::Sdvx, root.clone(), Intent::Install, None).unwrap();
        let options = || UpdaterOptions::new(konaste_updater::DownloadPolicy::official(["example.com"]).unwrap());
        let updater = Updater::open_fresh_with(&root, options()).unwrap();
        attach_handle(state, &stamp, updater.handle()).unwrap();
        drop(updater);
        assert!(Updater::open_with(&root, options()).is_err());
        clear_handle(state, &stamp);
        assert!(Updater::open_with(&root, options()).is_ok());
    }

    #[test]
    fn a_resume_can_queue_behind_another_game_and_later_be_unqueued() {
        let (directory, hub) = hub();
        let state = hub.state();
        let id = GameId::Sdvx;
        let root = directory.path().join("paused");
        let other = directory.path().join("active");
        state.updates.reserve(state, GameId::BomberGirl, other, Intent::Install, None).unwrap();
        remember_pending(state, id, &root, Intent::Install, true, true).unwrap();
        enqueue(&hub, id, Intent::Install, root.clone(), None).unwrap();
        assert_eq!(state.queue(), vec![id]);
        assert!(pending_of(state, id).unwrap().resumable);
        dequeue(&hub, id).unwrap();
        assert!(state.queue().is_empty());
        assert_eq!(game_of(state, id).unwrap().phase, GamePhase::Paused);
        assert!(pending_of(state, id).unwrap().resumable);
    }

    #[test]
    fn conflicting_pending_roots_are_refused_before_creating_a_folder() {
        let (directory, hub) = hub();
        let state = hub.state();
        let root = directory.path().canonicalize().unwrap().join("pending");
        std::fs::create_dir(&root).unwrap();
        enqueue(&hub, GameId::Sdvx, Intent::Install, root.clone(), None).unwrap();
        let sibling = root.join("subfolder");
        let validate = |path: &Path| validate_destination(state, GameId::BomberGirl, &prospective_root(path).unwrap());
        assert_eq!(validate(&root).unwrap_err().code, "destinationRejected");
        assert_eq!(validate(&sibling).unwrap_err().code, "destinationRejected");
        assert!(!sibling.exists());
        let marker = root.join(GameId::BomberGirl.installation_marker().unwrap());
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(marker, b"fixture").unwrap();
        assert_eq!(state.import(GameId::BomberGirl, root.clone()).unwrap_err().code, "destinationRejected");
        assert!(validate(&root.with_file_name("valid sibling")).is_ok());
    }

    #[test]
    fn a_failed_fresh_install_can_be_abandoned_without_deleting_files() {
        let (directory, hub) = hub();
        let state = hub.state();
        let id = GameId::Sdvx;
        let root = directory.path().join("pending");
        std::fs::create_dir(&root).unwrap();
        let partial = root.join("partial");
        std::fs::write(&partial, b"still here").unwrap();
        remember_pending(state, id, &root, Intent::Install, false, true).unwrap();
        resume_pending(state);
        discard_install(&hub, id).unwrap();
        assert!(pending_of(state, id).is_none());
        assert_eq!(game_of(state, id).unwrap().phase, GamePhase::Uninstalled);
        assert_eq!(std::fs::read(partial).unwrap(), b"still here");
    }

    #[test]
    fn older_settings_without_a_queue_key_still_load() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("settings.json"),
            br#"{"preferences":{"locale":"ja"},"imports":{},"pending_installs":{},"hidden":[]}"#,
        )
        .unwrap();
        let state = HubState::load_without_detection(directory.path().to_path_buf()).unwrap();
        assert!(state.queue().is_empty());
        assert_eq!(state.stored.lock().unwrap().preferences.locale, "ja");
        assert_eq!(state.stored.lock().unwrap().preferences.parallel, 4);
    }

    #[test]
    fn a_front_item_without_admission_is_retired_instead_of_blocking_the_queue() {
        let (_directory, hub) = hub();
        let state = hub.state();
        let (root, expected) = staged(r"C:\a");
        enqueue(&hub, GameId::Sdvx, PendingIntent::Install, root, expected).unwrap();
        let (root, expected) = staged(r"C:\b");
        enqueue(&hub, GameId::Ddr, PendingIntent::Install, root, expected).unwrap();
        state.updates.records.lock().unwrap().get_mut(&GameId::Sdvx).unwrap().staged = false;
        retire_queue_item(state, GameId::Sdvx).unwrap();
        assert_eq!(state.queue(), vec![GameId::Ddr]);
        assert!(state.updates.queued(GameId::Ddr));
    }

    #[test]
    fn starting_the_head_leaves_the_queue_and_exposes_the_next_item() {
        let (_directory, hub) = hub();
        let state = hub.state();
        for (id, root) in [(GameId::Sdvx, r"C:\a"), (GameId::Ddr, r"C:\b")] {
            let (root, expected) = staged(root);
            enqueue(&hub, id, PendingIntent::Install, root, expected).unwrap();
        }
        // What `start` does for the head as it promotes the item.
        remember_pending(state, GameId::Sdvx, Path::new(r"C:\a"), PendingIntent::Install, false, false).unwrap();
        assert_eq!(state.queue(), vec![GameId::Ddr]);
        // A queued item owns no engine slot, so the next pump may start it.
        assert!(!state.updates.any_active());
    }

    #[test]
    fn an_active_download_holds_the_engine_and_the_pump_would_stop() {
        let (_directory, hub) = hub();
        let state = hub.state();
        let (root, expected) = staged(r"C:\a");
        enqueue(&hub, GameId::Sdvx, PendingIntent::Install, root.clone(), expected).unwrap();
        // Reserve models `start`'s admission into the engine's active slot.
        state.updates.reserve(state, GameId::Sdvx, root.clone(), PendingIntent::Install, None).unwrap();
        remember_pending(state, GameId::Sdvx, &root, PendingIntent::Install, false, false).unwrap();
        assert!(state.updates.any_active());
        let (root, expected) = staged(r"C:\b");
        enqueue(&hub, GameId::Ddr, PendingIntent::Install, root, expected).unwrap();
        // The second item waits; the pump's `any_active` gate stays closed.
        assert_eq!(state.queue(), vec![GameId::Ddr]);
        assert!(state.updates.queued(GameId::Ddr));
    }

    /// After a restart the in-memory admission is empty. The pump rebuilds it
    /// from the persisted queue plus pending record, so the resumed head keeps
    /// its destination and intent.
    #[test]
    fn a_restarted_queue_rebuilds_admission_from_the_settings_file() {
        let (directory, hub) = hub();
        let (root, expected) = staged(r"C:\a");
        enqueue(&hub, GameId::Sdvx, PendingIntent::Install, root, expected).unwrap();
        drop(hub);
        let reloaded = HubState::load_without_detection(directory.path().to_path_buf()).unwrap();
        let id = reloaded.queue()[0];
        let pending = reloaded.stored.lock().unwrap().pending_installs.get(&id).cloned().unwrap();
        assert_eq!(pending.intent, PendingIntent::Install);
        assert_eq!(pending.root, PathBuf::from(r"C:\a"));
        assert!(!reloaded.updates.any_active());
    }

    #[test]
    fn a_queued_title_owns_no_engine_slot_until_the_pump_starts_it() {
        let (_directory, hub) = hub();
        let state = hub.state();
        assert!(!state.updates.any_active());
        let (root, expected) = staged(r"C:\a");
        enqueue(&hub, GameId::Sdvx, PendingIntent::Update, root, expected).unwrap();
        assert!(!state.updates.any_active());
        assert!(state.updates.queued(GameId::Sdvx));
        assert!(state.updates.reserved(GameId::Sdvx));
    }
}
