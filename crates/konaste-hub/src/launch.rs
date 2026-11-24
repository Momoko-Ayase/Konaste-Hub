//! Checks and Play.
//!
//! A background check runs fresh official discovery and pins the installed
//! index and main module. Play reuses that check: it re-pins the same two files
//! locally, makes one maintenance probe, takes a fresh launch token, and runs the
//! official SDK verification before creating the game. No discovery runs on Play.
use crate::{
    catalog::GameId,
    hub::Hub,
    platform,
    state::{CheckStamp, CheckedFiles, HubError, HubState, Snapshot},
};
use konaste_bootstrap::{GameFamily, PairStatus};
use konaste_runtime::{GameSession, InstallationError, InstallationPin, RuntimeError};
use std::path::{Path, PathBuf};

pub async fn check(app: Hub, id: GameId) -> Result<Snapshot, HubError> {
    if id.title().is_none() {
        app.state().close_unsupported(id);
        app.publish();
        return Err(HubError { code: "unsupportedTitle" });
    }
    let state = app.state();
    if crate::update::startup_skipped(state, id) {
        return Ok(state.snapshot());
    }
    let _operation = state.metadata_operation.lock().await;
    if crate::update::startup_skipped(state, id) {
        return Ok(state.snapshot());
    }
    let result = run_check(&app, id).await;
    app.publish();
    result.map(|()| state.snapshot())
}

/// Official components are fetched without a prompt when a check needs them.
pub async fn ensure_components(app: &Hub, family: GameFamily) -> Result<(), HubError> {
    let cache = app.state().directory.join("runtimes");
    let probe = cache.clone();
    let present = tokio::task::spawn_blocking(move || konaste_bootstrap::inspect(family, &probe))
        .await
        .map_err(|_| HubError { code: "componentsFailed" })?;
    if matches!(present, Ok(PairStatus::Present(_))) {
        return Ok(());
    }
    tokio::task::spawn_blocking(move || konaste_bootstrap::acquire(family, &cache, true, None))
        .await
        .map_err(|_| HubError { code: "componentsFailed" })?
        .map(|_| ())
        .map_err(|_| HubError { code: "componentsFailed" })
}

async fn run_check(app: &Hub, id: GameId) -> Result<(), HubError> {
    let family = id.runtime_family().ok_or(HubError { code: "unsupportedTitle" })?;
    let state = app.state();
    let ticket = state.begin_check(id)?;
    let stamp = ticket.stamp();
    app.publish();
    if let Err(error) = ensure_components(app, family).await {
        state.finish_check(ticket, false, None, error.code);
        return Err(error);
    }
    let root = stamp.root().map(ToOwned::to_owned);
    let cache = state.directory.join("runtimes");
    let outcome = tokio::task::spawn_blocking(move || inspect(family, &cache, root)).await.unwrap_or(CheckOutcome::Failed {
        runtime_ready: false,
        revision: None,
        error: "metadataCheckFailed",
    });
    match outcome {
        CheckOutcome::Current { revision, files, version } => {
            if state.finish_current_check(ticket, revision, version, files) {
                Ok(())
            } else {
                Err(HubError { code: "checkChanged" })
            }
        }
        CheckOutcome::Failed { runtime_ready, revision, error } => {
            state.finish_check(ticket, runtime_ready, revision, error);
            Err(HubError { code: error })
        }
    }
}

enum CheckOutcome {
    Current { revision: u32, files: CheckedFiles, version: String },
    Failed { runtime_ready: bool, revision: Option<u32>, error: &'static str },
}

fn inspect(family: GameFamily, cache: &Path, root: Option<PathBuf>) -> CheckOutcome {
    if konaste_runtime::title(family).is_err() {
        return failed(false, None, "metadataUnavailable");
    }
    let metadata = std::env::current_exe().ok().map(|worker| konaste_discovery::check(family, cache, &worker));
    let metadata = match metadata {
        Some(Ok(value)) => value,
        Some(Err(konaste_discovery::DiscoveryError::ComponentsMissing)) => return failed(false, None, "componentsMissing"),
        Some(Err(konaste_discovery::DiscoveryError::Runtime)) => return failed(false, None, "componentsFailed"),
        Some(Err(konaste_discovery::DiscoveryError::InvalidMetadata | konaste_discovery::DiscoveryError::Protocol)) => {
            return failed(true, None, "metadataInvalid");
        }
        _ => return failed(true, None, "metadataCheckFailed"),
    };
    let revision = metadata.index.revision;
    if metadata.family() != family {
        return failed(true, Some(revision), "metadataInvalid");
    }
    let Ok((pin, settings)) = metadata.installation_pins() else {
        return failed(true, Some(revision), "metadataInvalid");
    };
    let Some(root) = root else {
        return failed(true, Some(revision), "notAnInstallation");
    };
    match konaste_runtime::inspect_installation(family, &root, &pin) {
        Ok(installation) if installation.family() == family => CheckOutcome::Current {
            revision,
            files: CheckedFiles { installation: pin, settings },
            version: installation.product_version().to_string(),
        },
        Ok(_) => failed(true, Some(revision), "metadataInvalid"),
        Err(error) => failed(true, Some(revision), map_installation(error)),
    }
}

fn failed(runtime_ready: bool, revision: Option<u32>, error: &'static str) -> CheckOutcome {
    CheckOutcome::Failed { runtime_ready, revision, error }
}

fn map_installation(error: InstallationError) -> &'static str {
    match error {
        InstallationError::Outdated => "installationOutdated",
        InstallationError::Missing | InstallationError::Invalid => "installationInvalid",
        InstallationError::Unsupported => "unsupportedTitle",
    }
}

/// `answer` is the user's reply to what the game login page asked on the last
/// Play (terms, a still-running game). See `account::Gate`.
pub async fn play(app: Hub, id: GameId, answer: Option<crate::account::Answer>) -> Result<(), HubError> {
    require_launch_title(id)?;
    let state = app.state();
    let admission = std::sync::Arc::clone(&state.launch_operation).try_lock_owned().map_err(|_| HubError { code: "launchBusy" })?;
    let family = id.runtime_family().ok_or(HubError { code: "unsupportedTitle" })?;
    let (stamp, pin) = state.ready_evidence(id)?;
    if id == GameId::Gitadora && stamp.mode().is_none() {
        return Err(HubError { code: "gameModeRequired" });
    }
    state.begin_launch(&stamp)?;
    let mut lifecycle = LaunchLifecycle { app: app.clone(), stamp: Some(stamp), running: false };
    app.publish();
    match start(&app, id, family, pin, &lifecycle, answer).await {
        Err(error) => {
            crate::log::line(format!("play {id:?}: {}", error.code));
            play_failed(app, id, error, lifecycle)
        }
        Ok(session) => {
            lifecycle.running = true;
            app.publish();
            observe_game(session, lifecycle, admission);
            Ok(())
        }
    }
}

fn play_failed(app: Hub, id: GameId, error: HubError, mut lifecycle: LaunchLifecycle) -> Result<(), HubError> {
    if matches!(error.code, "installationOutdated" | "installationInvalid") {
        lifecycle.invalidate(error.code);
    } else {
        lifecycle.finish();
    }
    if matches!(error.code, "launchDenied" | "installationInvalid" | "installationOutdated") {
        // The server or the files changed since the last check.
        let hub = app.clone();
        tokio::spawn(async move {
            let _ = check(hub, id).await;
        });
    }
    Err(error)
}

async fn start(
    app: &Hub,
    id: GameId,
    family: GameFamily,
    pin: InstallationPin,
    lifecycle: &LaunchLifecycle,
    answer: Option<crate::account::Answer>,
) -> Result<GameSession, HubError> {
    let state = app.state();
    let root = lifecycle.stamp().root().ok_or(HubError { code: "notAnInstallation" })?.to_path_buf();
    let pinned_root = root.clone();
    let installation = tokio::task::spawn_blocking(move || konaste_runtime::inspect_installation(family, &pinned_root, &pin))
        .await
        .map_err(|_| HubError { code: "installationInvalid" })?
        .map_err(|error| HubError { code: map_installation(error) })?;
    if !id.admits(installation.family()) {
        return Err(HubError { code: "checkChanged" });
    }
    // Refuse a root the title's own libraries cannot address, before any folder
    // or check. A shared helper with Settings keeps the rule in one place.
    if !crate::official_setup::title_paths_ok(id, &root) {
        return Err(HubError { code: "installationInvalid" });
    }
    ensure_root_not_running(id, &root)?;
    // For pop'n, use the same ResourceDir the launch arguments will use: a
    // registration for this install, or a path derived from its verified root.
    // Play doesn't write registry values or modify the settings executable.
    let bind_root = root.clone();
    tokio::task::spawn_blocking(move || {
        #[cfg(all(windows, target_arch = "x86_64"))]
        if id == GameId::Popn {
            let resource = konaste_runtime::popn_resource_dir(&bind_root)?;
            return crate::official_setup::create_work_folders(&resource);
        }
        if let Some(resource) = crate::official_setup::resource_dir(id, &bind_root)? {
            crate::official_setup::create_work_folders(&resource)?;
        }
        Ok(())
    })
    .await
    .map_err(|_| HubError { code: "filesystem" })??;
    // The one network availability check for this attempt.
    state.maintenance.refresh().await?;
    app.publish();
    let session_generation = state.session.generation();
    let secret = state.session.launch_token(app, id, answer).await?;
    let cache = state.directory.join("runtimes");
    let executable = std::env::current_exe().map_err(|_| HubError { code: "launchVerificationFailed" })?;
    let mode = if id == GameId::Gitadora { lifecycle.stamp().mode() } else { None };
    let prepared = tokio::task::spawn_blocking(move || {
        konaste_runtime::prepare_launch(&cache, &executable, installation, secret, mode).map_err(|error| {
            // Why the official check stopped, for hub.log. No token or path.
            let note = konaste_runtime::last_worker_note().unwrap_or_default();
            crate::log::line(format!("prepare {family:?}: {error:?}; worker: {note}"));
            error
        })
    })
    .await
    .map_err(|_| HubError { code: "launchVerificationFailed" })?
    .map_err(HubError::from)?;
    let stamp = lifecycle.stamp().clone();
    let hub = app.clone();
    tokio::task::spawn_blocking(move || {
        let state = hub.state();
        state.commit_launch(&stamp, || {
            prepared.start(|| {
                if state.session.generation() != session_generation {
                    return Err(HubError { code: "signInRequired" });
                }
                ensure_root_not_running(id, &root)?;
                state.maintenance.ensure_allowed()
            })
        })
    })
    .await
    .map_err(|_| HubError { code: "gameStartFailed" })?
}

fn observe_game(mut session: GameSession, mut lifecycle: LaunchLifecycle, admission: tokio::sync::OwnedMutexGuard<()>) {
    tokio::spawn(async move {
        let result = tokio::task::spawn_blocking(move || session.wait()).await;
        drop(admission);
        let outcome = match result {
            Ok(Ok(code)) => Ok(code),
            _ => Err(()),
        };
        lifecycle.exited(outcome);
    });
}

struct LaunchLifecycle {
    app: Hub,
    stamp: Option<CheckStamp>,
    running: bool,
}
impl LaunchLifecycle {
    fn stamp(&self) -> &CheckStamp {
        self.stamp.as_ref().expect("active launch identity")
    }
    fn finish(&mut self) {
        if let Some(stamp) = self.stamp.take()
            && self.app.state().finish_launch(&stamp, None)
        {
            self.app.publish();
        }
    }
    fn invalidate(&mut self, code: &'static str) {
        if let Some(stamp) = self.stamp.take()
            && self.app.state().invalidate_launch(&stamp, code)
        {
            self.app.publish();
        }
    }
    fn exited(&mut self, result: Result<u32, ()>) {
        if let Some(stamp) = self.stamp.take() {
            match result {
                Ok(code) => {
                    if self.app.state().finish_launch(&stamp, Some(Some(code))) {
                        self.app.publish();
                    }
                    if code != 0 {
                        self.app.notify(crate::hub::Notice::GameExited { id: stamp.id(), code });
                    }
                }
                Err(()) => {
                    if self.app.state().invalidate_launch(&stamp, "processCheck") {
                        self.app.notify(crate::hub::Notice::Error("processCheck"));
                    }
                }
            }
        }
    }
}
impl Drop for LaunchLifecycle {
    fn drop(&mut self) {
        if self.running {
            self.exited(Err(()));
        } else {
            self.finish();
        }
    }
}

fn require_launch_title(id: GameId) -> Result<(), HubError> {
    if id.title().is_none() {
        return Err(HubError { code: "unsupportedTitle" });
    }
    if !id.launch_supported() {
        return Err(HubError { code: "launchUnsupported" });
    }
    Ok(())
}

pub fn ensure_root_not_running(id: GameId, root: &Path) -> Result<(), HubError> {
    let image = id.module_image(root).ok_or(HubError { code: "unsupportedTitle" })?;
    if platform::game_running(&image)? {
        return Err(HubError { code: "gameRunning" });
    }
    crate::game_settings::ensure_closed(id, root)
}

impl From<RuntimeError> for HubError {
    fn from(error: RuntimeError) -> Self {
        Self {
            code: match error {
                RuntimeError::VerificationDenied => "launchDenied",
                RuntimeError::ModuleMismatch => "installationInvalid",
                RuntimeError::ComponentsMissing => "componentsMissing",
                RuntimeError::Unsupported => "unsupportedTitle",
                RuntimeError::GameStart => "gameStartFailed",
                RuntimeError::GameSettings => "gameSettingsInvalid",
                RuntimeError::GameModeRequired => "gameModeRequired",
                RuntimeError::GameWait => "processCheck",
                _ => "launchVerificationFailed",
            },
        }
    }
}

#[allow(dead_code)]
fn _assert_state_is_shared(state: &HubState) -> &HubState {
    state
}
