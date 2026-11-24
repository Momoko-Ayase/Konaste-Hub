use crate::catalog::{CATALOG, GameId};
use crate::{
    account::{AccountStatus, Session},
    platform,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, Serialize)]
pub struct HubError {
    pub code: &'static str,
}
impl From<std::io::Error> for HubError {
    fn from(_: std::io::Error) -> Self {
        Self { code: "filesystem" }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    pub locale: String,
    pub remember: bool,
    pub parallel: usize,
    pub close_to_tray: bool,
    pub default_folder: Option<PathBuf>,
    pub selected: Option<GameId>,
    /// Last GITADORA instrument. `None` is not a game and is not sent anywhere else.
    #[serde(default)]
    pub gitadora_mode: Option<konaste_runtime::GitadoraMode>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            locale: "en".into(),
            remember: true,
            parallel: 4,
            close_to_tray: true,
            default_folder: None,
            selected: None,
            gitadora_mode: None,
        }
    }
}
impl Preferences {
    pub fn validate(&self) -> Result<(), HubError> {
        if !["en", "ja", "zh-CN", "zh-TW", "ko"].contains(&self.locale.as_str()) || ![2, 4, 6, 8].contains(&self.parallel) {
            return Err(HubError { code: "invalidSettings" });
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PendingIntent {
    Install,
    Update,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingInstall {
    pub root: PathBuf,
    pub intent: PendingIntent,
    pub paused: bool,
    pub resumable: bool,
    /// Selected root expected at start. `None` for a fresh install because no
    /// library root existed.
    pub expected_selected: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StoredData {
    pub preferences: Preferences,
    pub imports: BTreeMap<GameId, PathBuf>,
    pub pending_installs: BTreeMap<GameId, PendingInstall>,
    /// Detected installs the user removed from the library; detection skips them.
    pub hidden: std::collections::BTreeSet<GameId>,
    /// Download queue order, front first. Titles appear once, in this order.
    /// Each queued title has a `pending_installs` record for its destination.
    /// Starting the title removes its queue position, not its pending work.
    pub download_queue: Vec<GameId>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GamePhase {
    Uninstalled,
    Checking,
    Blocked,
    Ready,
    OpeningSettings,
    Configuring,
    Launching,
    Running,
    Update,
    Queued,
    Preparing,
    Downloading,
    Paused,
    Verifying,
    Installing,
    Recovering,
    Finalizing,
}

impl GamePhase {
    pub fn settings_busy(&self) -> bool {
        matches!(self, Self::OpeningSettings | Self::Configuring)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSnapshot {
    pub id: GameId,
    pub title: String,
    pub phase: GamePhase,
    pub install_dir: Option<PathBuf>,
    pub installed_version: Option<String>,
    pub available_version: Option<String>,
    pub resource_revision: Option<u32>,
    pub progress: u8,
    pub transfer: Option<crate::transfer::Snapshot>,
    pub work: Option<crate::transfer::Work>,
    pub error: Option<&'static str>,
    /// Last observed game exit only. No token, account, path, or launch arguments.
    pub last_exit_code: Option<u32>,
    pub runtime_ready: bool,
    pub update_pending: bool,
    /// Verified install contract. Derived on every snapshot, never persisted.
    pub supported: bool,
    /// Pinned bootstrap package exists. Not install, update, or launch permission.
    pub bootstrap: bool,
    /// Verified game creation. Derived on every snapshot, never persisted.
    pub launch_supported: bool,
    /// Selected GITADORA instrument. `None` for every other title.
    pub gitadora_mode: Option<konaste_runtime::GitadoraMode>,
    /// Registry-detected install root that is not in the library yet. The Add
    /// game dialog offers it as "Found at"; it is never adopted silently. This
    /// is detection evidence only, not an install or launch contract.
    pub detected_dir: Option<PathBuf>,
    /// 1-based position while `phase` is `Queued`; `None` otherwise.
    pub queue_position: Option<usize>,
}
impl GameSnapshot {
    pub fn can_launch(&self, maintenance: &crate::maintenance::Snapshot) -> bool {
        self.supported
            && self.launch_supported
            && self.id.launch_supported()
            && (self.id != GameId::Gitadora || self.gitadora_mode.is_some())
            && !self.update_pending
            && maintenance.kind == crate::maintenance::Kind::None
            && self.phase == GamePhase::Ready
            && self.error.is_none()
            && self.runtime_ready
            && self.installed_version.is_some()
            && self.installed_version == self.available_version
            && self.install_dir.is_some()
            && self.resource_revision.is_some()
    }
}

#[derive(Clone)]
pub struct Snapshot {
    pub preferences: Preferences,
    pub games: Vec<GameSnapshot>,
    pub account: AccountStatus,
    pub profile: Option<crate::account::Profile>,
    pub maintenance: crate::maintenance::Snapshot,
}

pub struct CheckTicket {
    id: GameId,
    generation: u64,
    root: Option<PathBuf>,
    gitadora_mode: Option<konaste_runtime::GitadoraMode>,
}

// An opaque native check identity, never frontend state or launch permission.
#[derive(Clone)]
pub struct CheckStamp {
    id: GameId,
    generation: u64,
    root: Option<PathBuf>,
    gitadora_mode: Option<konaste_runtime::GitadoraMode>,
}
impl CheckTicket {
    pub fn stamp(&self) -> CheckStamp {
        CheckStamp { id: self.id, generation: self.generation, root: self.root.clone(), gitadora_mode: self.gitadora_mode }
    }
}
impl CheckStamp {
    pub fn id(&self) -> GameId {
        self.id
    }
    pub fn root(&self) -> Option<&std::path::Path> {
        self.root.as_deref()
    }
    /// Instrument bound to this check. `None` for every title except GITADORA.
    pub fn mode(&self) -> Option<konaste_runtime::GitadoraMode> {
        self.gitadora_mode
    }
}

#[derive(Clone, Copy)]
pub(crate) struct CheckedFiles {
    pub installation: konaste_runtime::InstallationPin,
    pub settings: Option<konaste_runtime::SettingsPin>,
}

/// Exclusive settings operation. Consumed on exit; the installation check and
/// its evidence are left intact because the editor changes user preferences.
pub struct SettingsTicket {
    id: GameId,
    generation: u64,
    pub root: PathBuf,
    pub pin: Option<konaste_runtime::SettingsPin>,
    previous_phase: GamePhase,
}

impl SettingsTicket {
    fn matches(&self, game: &GameSnapshot) -> bool {
        game.id == self.id && game.install_dir.as_ref() == Some(&self.root)
    }
}

pub struct HubState {
    pub directory: PathBuf,
    pub stored: Mutex<StoredData>,
    pub games: Mutex<Vec<GameSnapshot>>,
    pub session: Session,
    pub maintenance: crate::maintenance::Maintenance,
    pub metadata_operation: tokio::sync::Mutex<()>,
    pub launch_operation: Arc<tokio::sync::Mutex<()>>,
    pub updates: crate::update::UpdateSession,
    pub(crate) checks: Mutex<BTreeMap<GameId, u64>>,
    /// Digests from the last successful check, bound to its generation. Not persisted.
    pub(crate) evidence: Mutex<BTreeMap<GameId, (u64, CheckedFiles)>>,
}

impl HubState {
    pub fn load(directory: PathBuf) -> Result<Self, HubError> {
        Self::load_with_detection(directory, true)
    }

    /// Test fixture only. Production load still detects an installation.
    #[cfg(test)]
    pub(crate) fn load_without_detection(directory: PathBuf) -> Result<Self, HubError> {
        Self::load_with_detection(directory, false)
    }

    fn load_with_detection(directory: PathBuf, detect: bool) -> Result<Self, HubError> {
        fs::create_dir_all(&directory)?;
        let file = directory.join("settings.json");
        let stored = match fs::read(&file) {
            Ok(bytes) => serde_json::from_slice::<StoredData>(&bytes).map_err(|_| HubError { code: "invalidSettings" })?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => StoredData::default(),
            Err(error) => return Err(error.into()),
        };
        stored.preferences.validate()?;
        let mut games = Vec::with_capacity(CATALOG.len());
        for game in &CATALOG {
            let marker = game.id.installation_marker();
            // Detection is evidence for the Add game dialog, not library
            // membership. Existing imports stay authoritative.
            let detected = if detect && !stored.imports.contains_key(&game.id) && !stored.hidden.contains(&game.id) {
                detected_install(game)
            } else {
                None
            };
            let root = stored.imports.get(&game.id).cloned().and_then(|root| fs::canonicalize(root).ok());
            let verified = game.id.title().is_some();
            let bootstrap = game.id.runtime_family().is_some();
            let launch_supported = game.id.launch_supported();
            let root = root.filter(|root| marker.is_some_and(|exe| root.join(exe).is_file()));
            let installed = root.is_some();
            let installed_version =
                if detect { root.as_ref().zip(marker).and_then(|(root, exe)| platform::product_version(&root.join(exe))) } else { None };
            let (phase, error) = if !verified {
                (if installed { GamePhase::Blocked } else { GamePhase::Uninstalled }, Some("unsupportedTitle"))
            } else if installed {
                (GamePhase::Checking, None)
            } else {
                (GamePhase::Uninstalled, None)
            };
            games.push(GameSnapshot {
                id: game.id,
                title: game.title.into(),
                phase,
                installed_version,
                install_dir: root,
                available_version: None,
                resource_revision: None,
                progress: 0,
                transfer: None,
                work: None,
                error,
                last_exit_code: None,
                runtime_ready: false,
                update_pending: false,
                supported: verified,
                bootstrap,
                launch_supported,
                gitadora_mode: if game.id == GameId::Gitadora { stored.preferences.gitadora_mode } else { None },
                detected_dir: detected,
                queue_position: None,
            });
        }
        let session = Session::new(directory.join("session.dat"));
        let state = Self {
            directory,
            stored: Mutex::new(stored),
            games: Mutex::new(games),
            session,
            maintenance: crate::maintenance::Maintenance::default(),
            metadata_operation: tokio::sync::Mutex::new(()),
            launch_operation: Arc::new(tokio::sync::Mutex::new(())),
            updates: crate::update::UpdateSession::default(),
            checks: Mutex::new(BTreeMap::new()),
            evidence: Mutex::new(BTreeMap::new()),
        };
        crate::update::resume_pending(&state);
        Ok(state)
    }

    pub fn snapshot(&self) -> Snapshot {
        let (preferences, queued) = {
            let stored = self.stored.lock().unwrap();
            (stored.preferences.clone(), stored.download_queue.clone())
        };
        let mut games = self.games.lock().unwrap().clone();
        for game in &mut games {
            game.supported = game.id.title().is_some();
            game.bootstrap = game.id.runtime_family().is_some();
            game.launch_supported = game.id.launch_supported();
            game.gitadora_mode = if game.id == GameId::Gitadora { preferences.gitadora_mode } else { None };
            game.queue_position =
                if game.phase == GamePhase::Queued { queued.iter().position(|id| *id == game.id).map(|ix| ix + 1) } else { None };
        }
        Snapshot {
            preferences,
            games,
            account: self.session.status(),
            profile: self.session.profile(),
            maintenance: self.maintenance.snapshot(),
        }
    }

    pub fn has_running_game(&self) -> bool {
        self.games
            .lock()
            .unwrap()
            .iter()
            .any(|game| matches!(game.phase, GamePhase::Launching | GamePhase::Running) || game.phase.settings_busy())
    }

    pub fn update_active(&self) -> bool {
        self.games.lock().unwrap().iter().any(|game| {
            matches!(
                game.phase,
                GamePhase::Preparing
                    | GamePhase::Downloading
                    | GamePhase::Verifying
                    | GamePhase::Installing
                    | GamePhase::Recovering
                    | GamePhase::Finalizing
            ) || self.updates.busy(game.id)
        }) || self.updates.any_active()
    }

    /// Queue order. Used by the queue tests and available to the UI.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn queue(&self) -> Vec<GameId> {
        self.stored.lock().unwrap().download_queue.clone()
    }

    pub fn begin_check(&self, id: GameId) -> Result<CheckTicket, HubError> {
        let mode = self.stored.lock().unwrap().preferences.gitadora_mode.filter(|_| id == GameId::Gitadora);
        let mut checks = self.checks.lock().unwrap();
        let mut games = self.games.lock().unwrap();
        let game = games.iter_mut().find(|game| game.id == id).ok_or(HubError { code: "unsupportedTitle" })?;
        if game.phase.settings_busy() {
            return Err(HubError { code: "gameSettingsBusy" });
        }
        if matches!(game.phase, GamePhase::Launching | GamePhase::Running) {
            return Err(HubError { code: "gameRunning" });
        }
        if matches!(game.phase, GamePhase::Queued) || self.updates.reserved(id) {
            return Err(HubError { code: "updateBusy" });
        }
        let generation = checks.entry(id).or_default();
        *generation = generation.checked_add(1).ok_or(HubError { code: "metadataCheckFailed" })?;
        self.evidence.lock().unwrap().remove(&id);
        game.phase = GamePhase::Checking;
        game.error = None;
        game.available_version = None;
        game.resource_revision = None;
        game.runtime_ready = false;
        game.transfer = None;
        game.work = None;
        game.last_exit_code = None;
        game.gitadora_mode = mode;
        Ok(CheckTicket { id, generation: *generation, root: game.install_dir.clone(), gitadora_mode: mode })
    }

    pub fn finish_check(&self, ticket: CheckTicket, runtime_ready: bool, revision: Option<u32>, error: &'static str) -> bool {
        self.apply_check(ticket, |game| {
            game.runtime_ready = runtime_ready;
            game.resource_revision = revision;
            let replaceable = game.id.title().is_some()
                && game.install_dir.is_some()
                && runtime_ready
                && revision.is_some()
                && matches!(error, "installationOutdated" | "installationInvalid");
            game.phase = if replaceable {
                GamePhase::Update
            } else if game.install_dir.is_some() {
                GamePhase::Blocked
            } else {
                GamePhase::Uninstalled
            };
            game.error = Some(error);
        })
    }

    pub fn finish_current_check(&self, ticket: CheckTicket, revision: u32, product_version: String, files: CheckedFiles) -> bool {
        if ticket.root.is_none() {
            return false;
        }
        let (id, generation) = (ticket.id, ticket.generation);
        self.apply_check(ticket, |game| {
            self.evidence.lock().unwrap().insert(id, (generation, files));
            game.runtime_ready = true;
            game.resource_revision = Some(revision);
            game.installed_version = Some(product_version.clone());
            game.available_version = Some(product_version);
            game.launch_supported = game.id.launch_supported();
            // Current files are not launch permission for an install-only title.
            if game.launch_supported {
                game.phase = GamePhase::Ready;
                game.error = None;
            } else {
                game.phase = GamePhase::Blocked;
                game.error = Some("launchUnsupported");
            }
        })
    }

    /// The last successful check for Play. The stamp is the check that produced it.
    pub fn ready_evidence(&self, id: GameId) -> Result<(CheckStamp, konaste_runtime::InstallationPin), HubError> {
        let checks = self.checks.lock().unwrap();
        let games = self.games.lock().unwrap();
        let game = games.iter().find(|game| game.id == id).ok_or(HubError { code: "unsupportedTitle" })?;
        if !game.can_launch(&self.maintenance.snapshot()) {
            return Err(HubError {
                code: if self.maintenance.snapshot().kind != crate::maintenance::Kind::None { "maintenance" } else { "launchUnavailable" },
            });
        }
        let generation = *checks.get(&id).ok_or(HubError { code: "launchUnavailable" })?;
        let (bound, files) = *self.evidence.lock().unwrap().get(&id).ok_or(HubError { code: "launchUnavailable" })?;
        if bound != generation {
            return Err(HubError { code: "checkChanged" });
        }
        Ok((CheckStamp { id, generation, root: game.install_dir.clone(), gitadora_mode: game.gitadora_mode }, files.installation))
    }

    #[cfg(test)]
    pub fn check_current(&self, stamp: &CheckStamp) -> bool {
        let checks = self.checks.lock().unwrap();
        checks.get(&stamp.id) == Some(&stamp.generation)
            && self.games.lock().unwrap().iter().any(|game| game.id == stamp.id && game.install_dir == stamp.root)
    }

    fn apply_check(&self, ticket: CheckTicket, update: impl FnOnce(&mut GameSnapshot)) -> bool {
        let checks = self.checks.lock().unwrap();
        if checks.get(&ticket.id) != Some(&ticket.generation) {
            return false;
        }
        let mut games = self.games.lock().unwrap();
        let Some(game) = games.iter_mut().find(|game| game.id == ticket.id) else {
            return false;
        };
        if game.install_dir != ticket.root {
            return false;
        }
        update(game);
        true
    }

    pub fn begin_settings(&self, id: GameId) -> Result<SettingsTicket, HubError> {
        if !id.has_settings() {
            return Err(HubError { code: "unsupportedTitle" });
        }
        let mut checks = self.checks.lock().unwrap();
        let mut games = self.games.lock().unwrap();
        let game = games.iter_mut().find(|game| game.id == id).ok_or(HubError { code: "unsupportedTitle" })?;
        if game.update_pending || self.updates.reserved(id) {
            return Err(HubError { code: "updateBusy" });
        }
        if matches!(game.phase, GamePhase::Running | GamePhase::Launching) {
            return Err(HubError { code: "gameRunning" });
        }
        if !matches!(game.phase, GamePhase::Ready | GamePhase::Blocked | GamePhase::Update) {
            return Err(HubError { code: "gameSettingsBusy" });
        }
        let root = game.install_dir.clone().ok_or(HubError { code: "notAnInstallation" })?;
        let generation = *checks.entry(id).or_default();
        let pin = self.evidence.lock().unwrap().get(&id).filter(|(bound, _)| *bound == generation).and_then(|(_, files)| files.settings);
        let previous_phase = std::mem::replace(&mut game.phase, GamePhase::OpeningSettings);
        Ok(SettingsTicket { id, generation, root, pin, previous_phase })
    }

    pub fn commit_settings<T>(&self, ticket: &SettingsTicket, create: impl FnOnce() -> Result<T, HubError>) -> Result<T, HubError> {
        let checks = self.checks.lock().unwrap();
        let games = self.games.lock().unwrap();
        if checks.get(&ticket.id) != Some(&ticket.generation)
            || !games.iter().any(|game| ticket.matches(game) && game.phase == GamePhase::OpeningSettings && !game.update_pending)
            || self.updates.reserved(ticket.id)
        {
            return Err(HubError { code: "checkChanged" });
        }
        create()
    }

    pub fn settings_visible(&self, ticket: &SettingsTicket) -> bool {
        let checks = self.checks.lock().unwrap();
        if checks.get(&ticket.id) != Some(&ticket.generation) {
            return false;
        }
        let mut games = self.games.lock().unwrap();
        let Some(game) = games.iter_mut().find(|game| ticket.matches(game) && game.phase == GamePhase::OpeningSettings) else {
            return false;
        };
        game.phase = GamePhase::Configuring;
        true
    }

    pub fn finish_settings(&self, ticket: SettingsTicket) -> bool {
        let checks = self.checks.lock().unwrap();
        if checks.get(&ticket.id) != Some(&ticket.generation) {
            return false;
        }
        let mut games = self.games.lock().unwrap();
        let Some(game) = games.iter_mut().find(|game| ticket.matches(game) && game.phase.settings_busy()) else {
            return false;
        };
        game.phase = ticket.previous_phase;
        true
    }

    pub fn begin_launch(&self, stamp: &CheckStamp) -> Result<(), HubError> {
        let checks = self.checks.lock().unwrap();
        if checks.get(&stamp.id) != Some(&stamp.generation) {
            return Err(HubError { code: "checkChanged" });
        }
        let mut games = self.games.lock().unwrap();
        let game =
            games.iter_mut().find(|game| game.id == stamp.id && game.install_dir == stamp.root).ok_or(HubError { code: "checkChanged" })?;
        self.maintenance.ensure_allowed()?;
        if !game.can_launch(&self.maintenance.snapshot()) {
            return Err(HubError { code: "launchUnavailable" });
        }
        game.supported = true;
        game.phase = GamePhase::Launching;
        Ok(())
    }

    // `create` is synchronous and must not call back into HubState. Holding these
    // locks makes sign-out, reimport and a newer check serialize with creation.
    pub fn commit_launch<T>(&self, stamp: &CheckStamp, create: impl FnOnce() -> Result<T, HubError>) -> Result<T, HubError> {
        let checks = self.checks.lock().unwrap();
        if checks.get(&stamp.id) != Some(&stamp.generation) {
            return Err(HubError { code: "checkChanged" });
        }
        let mut games = self.games.lock().unwrap();
        let game =
            games.iter_mut().find(|game| game.id == stamp.id && game.install_dir == stamp.root).ok_or(HubError { code: "checkChanged" })?;
        if game.phase != GamePhase::Launching {
            return Err(HubError { code: "checkChanged" });
        }
        if self.session.status() != AccountStatus::SignedIn {
            return Err(HubError { code: "signInRequired" });
        }
        self.maintenance.ensure_allowed()?;
        let process = create()?;
        game.phase = GamePhase::Running;
        game.error = None;
        Ok(process)
    }

    /// Starting ended (cancelled, failed) or the game exited. The last check stays
    /// valid, so the game is Ready again. `exit` is the observed exit code, if any.
    pub fn finish_launch(&self, stamp: &CheckStamp, exit: Option<Option<u32>>) -> bool {
        let checks = self.checks.lock().unwrap();
        if checks.get(&stamp.id) != Some(&stamp.generation) {
            return false;
        }
        let mut games = self.games.lock().unwrap();
        let Some(game) = games.iter_mut().find(|game| game.id == stamp.id && game.install_dir == stamp.root) else {
            return false;
        };
        if !matches!(game.phase, GamePhase::Launching | GamePhase::Running) {
            return false;
        }
        game.phase = GamePhase::Ready;
        game.error = None;
        game.last_exit_code = exit.flatten();
        true
    }

    /// The files changed since the last check. Play stays closed until a new check.
    pub fn invalidate_launch(&self, stamp: &CheckStamp, code: &'static str) -> bool {
        let checks = self.checks.lock().unwrap();
        if checks.get(&stamp.id) != Some(&stamp.generation) {
            return false;
        }
        let mut games = self.games.lock().unwrap();
        let Some(game) = games.iter_mut().find(|game| game.id == stamp.id && game.install_dir == stamp.root) else {
            return false;
        };
        game.phase = GamePhase::Blocked;
        game.error = Some(code);
        game.runtime_ready = false;
        game.available_version = None;
        game.resource_revision = None;
        self.evidence.lock().unwrap().remove(&stamp.id);
        true
    }

    /// Persist preferences after sign-out handling. A GITADORA instrument change
    /// cannot land while that title is starting or running.
    pub fn commit_preferences(&self, preferences: Preferences) -> Result<(), HubError> {
        preferences.validate()?;
        let mut stored = self.stored.lock().unwrap();
        let mut games = self.games.lock().unwrap();
        let mode_changed = preferences.gitadora_mode != stored.preferences.gitadora_mode;
        if mode_changed {
            let running =
                games.iter().any(|game| game.id == GameId::Gitadora && matches!(game.phase, GamePhase::Launching | GamePhase::Running));
            if running {
                return Err(HubError { code: "gameModeBusy" });
            }
        }
        let mut next = stored.clone();
        next.preferences = preferences;
        self.save(&next)?;
        *stored = next;
        if mode_changed {
            // The instrument is a Play argument, not part of the update check:
            // the check result stays valid and Play reads the new choice.
            if let Some(game) = games.iter_mut().find(|game| game.id == GameId::Gitadora) {
                game.gitadora_mode = stored.preferences.gitadora_mode;
            }
        }
        Ok(())
    }

    pub fn save(&self, stored: &StoredData) -> Result<(), HubError> {
        stored.preferences.validate()?;
        let bytes = serde_json::to_vec_pretty(stored).map_err(|_| HubError { code: "invalidSettings" })?;
        let temporary = self.directory.join("settings.json.tmp");
        let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        platform::replace_file(&temporary, &self.directory.join("settings.json"))?;
        Ok(())
    }

    /// A detected install whose runtime contract is not verified must not stay Checking.
    pub fn close_unsupported(&self, id: GameId) {
        if id.title().is_some() {
            return;
        }
        let mut games = self.games.lock().unwrap();
        let Some(game) = games.iter_mut().find(|game| game.id == id) else {
            return;
        };
        if matches!(game.phase, GamePhase::Running | GamePhase::Launching) || game.phase.settings_busy() || self.updates.busy(id) {
            return;
        }
        game.supported = false;
        game.phase = if game.install_dir.is_some() { GamePhase::Blocked } else { GamePhase::Uninstalled };
        game.error = Some("unsupportedTitle");
        game.available_version = None;
        game.resource_revision = None;
        game.runtime_ready = false;
    }

    /// Removes a title from the library. Files on disk are left untouched.
    pub fn forget(&self, id: GameId) -> Result<(), HubError> {
        if self.updates.reserved(id) {
            return Err(HubError { code: "updateBusy" });
        }
        let mut stored = self.stored.lock().unwrap();
        let mut checks = self.checks.lock().unwrap();
        let mut games = self.games.lock().unwrap();
        let snapshot = games.iter_mut().find(|game| game.id == id).ok_or(HubError { code: "unsupportedTitle" })?;
        if matches!(snapshot.phase, GamePhase::Running | GamePhase::Launching) {
            return Err(HubError { code: "gameRunning" });
        }
        if snapshot.phase.settings_busy() {
            return Err(HubError { code: "gameSettingsBusy" });
        }
        if stored.download_queue.contains(&id) || stored.pending_installs.contains_key(&id) {
            return Err(HubError { code: "updateBusy" });
        }
        let mut next = stored.clone();
        next.imports.remove(&id);
        next.pending_installs.remove(&id);
        next.hidden.insert(id);
        self.save(&next)?;
        *stored = next;
        let generation = checks.entry(id).or_default();
        *generation = generation.saturating_add(1);
        self.evidence.lock().unwrap().remove(&id);
        if let Some(root) = &snapshot.install_dir {
            crate::official_setup::forget(id, root);
        }
        snapshot.detected_dir = None;
        snapshot.queue_position = None;
        snapshot.install_dir = None;
        snapshot.installed_version = None;
        snapshot.phase = GamePhase::Uninstalled;
        snapshot.error = None;
        snapshot.available_version = None;
        snapshot.resource_revision = None;
        snapshot.runtime_ready = false;
        snapshot.update_pending = false;
        snapshot.transfer = None;
        snapshot.work = None;
        Ok(())
    }

    pub fn import(&self, id: GameId, root: PathBuf) -> Result<(), HubError> {
        let exe = id.installation_marker().ok_or(HubError { code: "unsupportedTitle" })?;
        let root = fs::canonicalize(root)?;
        if !root.is_dir() || !root.join(exe).is_file() {
            return Err(HubError { code: "notAnInstallation" });
        }
        self.import_root(id, exe, root)
    }

    fn import_root(&self, id: GameId, exe: &str, root: PathBuf) -> Result<(), HubError> {
        // stored -> checks -> games -> update records. The next generation is
        // computed before the settings write, so exhaustion changes nothing.
        if self.updates.reserved(id) {
            return Err(HubError { code: "updateBusy" });
        }
        let mut stored = self.stored.lock().unwrap();
        let mut checks = self.checks.lock().unwrap();
        let mut games = self.games.lock().unwrap();
        let snapshot = games.iter_mut().find(|game| game.id == id).ok_or(HubError { code: "unsupportedTitle" })?;
        if snapshot.phase.settings_busy() {
            return Err(HubError { code: "gameSettingsBusy" });
        }
        if matches!(snapshot.phase, GamePhase::Launching | GamePhase::Running) {
            return Err(HubError { code: "gameRunning" });
        }
        if self.updates.reserved(id) || stored.pending_installs.contains_key(&id) {
            return Err(HubError { code: "updateBusy" });
        }
        if stored.imports.iter().any(|(at, other)| *at != id && crate::update::related(&root, other))
            || stored.pending_installs.values().any(|pending| crate::update::related(&root, &pending.root))
        {
            return Err(HubError { code: "destinationRejected" });
        }
        let generation = checks.entry(id).or_default();
        let next_generation = generation.checked_add(1).ok_or(HubError { code: "metadataCheckFailed" })?;
        let mut next = stored.clone();
        next.imports.insert(id, root.clone());
        next.hidden.remove(&id);
        next.pending_installs.remove(&id);
        next.download_queue.retain(|queued| *queued != id);
        self.save(&next)?;
        *stored = next;
        *generation = next_generation;
        snapshot.detected_dir = None;
        snapshot.queue_position = None;
        snapshot.installed_version = platform::product_version(&root.join(exe));
        snapshot.install_dir = Some(root);
        snapshot.phase = if id.title().is_some() { GamePhase::Checking } else { GamePhase::Blocked };
        snapshot.error = None;
        snapshot.available_version = None;
        snapshot.resource_revision = None;
        snapshot.runtime_ready = false;
        snapshot.update_pending = false;
        snapshot.transfer = None;
        snapshot.work = None;
        snapshot.last_exit_code = None;
        snapshot.supported = id.title().is_some();
        snapshot.bootstrap = id.runtime_family().is_some();
        snapshot.launch_supported = id.launch_supported();
        snapshot.gitadora_mode = if id == GameId::Gitadora { stored.preferences.gitadora_mode } else { None };
        Ok(())
    }
}

fn detected_install(game: &crate::catalog::GameDescriptor) -> Option<PathBuf> {
    let marker = game.id.installation_marker()?;
    platform::detect_root(game).and_then(|root| fs::canonicalize(root).ok()).filter(|root| root.join(marker).is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETTINGS_PIN: konaste_runtime::SettingsPin = konaste_runtime::SettingsPin { bytes: 7, sha256: [3; 32] };

    fn settings_fixture() -> (tempfile::TempDir, HubState, GameId) {
        let dir = tempfile::tempdir().unwrap();
        let mut state = HubState::load_without_detection(dir.path().to_path_buf()).unwrap();
        state.maintenance = crate::maintenance::Maintenance::with_clock(|| 0);
        let id = GameId::Sdvx;
        let root = dir.path().join("game");
        let marker = root.join(id.installation_marker().unwrap());
        fs::create_dir_all(marker.parent().unwrap()).unwrap();
        fs::write(marker, b"fixture").unwrap();
        state.import(id, root).unwrap();
        let ticket = state.begin_check(id).unwrap();
        let pin = konaste_runtime::InstallationPin { index_sha256: [1; 32], module_sha256: [2; 32], module_bytes: 7 };
        assert!(state.finish_current_check(
            ticket,
            123,
            "test-version".into(),
            CheckedFiles { installation: pin, settings: Some(SETTINGS_PIN) }
        ));
        (dir, state, id)
    }

    #[test]
    fn settings_is_not_open_while_the_editor_is_starting() {
        let (_dir, state, id) = settings_fixture();
        let settings = state.begin_settings(id).unwrap();
        let game = || state.snapshot().games.into_iter().find(|g| g.id == id).unwrap();
        assert_eq!(settings.pin, Some(SETTINGS_PIN));
        assert_eq!(game().phase, GamePhase::OpeningSettings);
        assert!(!game().can_launch(&state.maintenance.snapshot()));
        assert!(state.has_running_game(), "keep tracking settings if the Hub window closes");
        assert_eq!(state.begin_check(id).err().unwrap().code, "gameSettingsBusy");
        assert_eq!(state.forget(id).unwrap_err().code, "gameSettingsBusy");
        assert_eq!(state.import(id, settings.root.clone()).unwrap_err().code, "gameSettingsBusy");

        state.commit_settings(&settings, || Ok(())).unwrap();
        assert_eq!(game().phase, GamePhase::OpeningSettings, "the process exists but its window is not open yet");
        assert!(state.settings_visible(&settings));
        assert_eq!(game().phase, GamePhase::Configuring);
        assert!(state.finish_settings(settings));
    }

    #[test]
    fn settings_close_restores_play_without_a_new_check() {
        let (_dir, state, id) = settings_fixture();
        let (checked, _) = state.ready_evidence(id).unwrap();
        let settings = state.begin_settings(id).unwrap();
        state.commit_settings(&settings, || Ok(())).unwrap();
        assert!(state.settings_visible(&settings));
        assert!(state.finish_settings(settings));
        let game = state.snapshot().games.into_iter().find(|g| g.id == id).unwrap();
        assert_eq!(game.phase, GamePhase::Ready, "closing settings must not check for updates");
        assert!(state.check_current(&checked));
        assert!(state.ready_evidence(id).is_ok());
    }

    #[test]
    fn settings_preserves_an_existing_update_or_check_error() {
        let (_dir, state, id) = settings_fixture();
        for (phase, error) in [(GamePhase::Update, "installationOutdated"), (GamePhase::Blocked, "metadataCheckFailed")] {
            let ticket = state.begin_check(id).unwrap();
            assert!(state.finish_check(ticket, true, Some(124), error));
            let settings = state.begin_settings(id).unwrap();
            assert!(settings.pin.is_none(), "a new check invalidates the cached pin");
            assert!(state.finish_settings(settings));
            let game = state.snapshot().games.into_iter().find(|g| g.id == id).unwrap();
            assert_eq!(game.phase, phase);
            assert_eq!(game.error, Some(error));
            assert_eq!(game.resource_revision, Some(124));
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn settings_uses_cached_pin_without_waiting_for_discovery() {
        let (_dir, state, id) = settings_fixture();
        let (hub, _) = crate::hub::Hub::new(state, tokio::runtime::Handle::current());
        let state = hub.state();
        let _metadata = state.metadata_operation.lock().await;
        // No real settings executable exists in the fixture. Reaching the local
        // file error with the discovery lock held proves we skipped discovery.
        let error = tokio::time::timeout(std::time::Duration::from_secs(2), crate::game_settings::open(hub.clone(), id))
            .await
            .expect("local settings validation must not wait for discovery")
            .unwrap_err();
        assert_eq!(error.code, "gameSettingsUnavailable");
        assert!(state.ready_evidence(id).is_ok(), "a settings error does not invalidate Play");
        assert!(state.launch_operation.try_lock().is_ok());
    }

    #[test]
    fn infinitas_is_listed_but_never_detected_or_imported() {
        let library = tempfile::tempdir().unwrap();
        let state = HubState::load(library.path().to_path_buf()).unwrap();
        let game = state.snapshot().games.into_iter().find(|g| g.id == GameId::Infinitas).unwrap();
        assert!(game.detected_dir.is_none() && game.install_dir.is_none());
        assert!(!game.supported && !game.bootstrap && !game.launch_supported);
        assert_eq!(game.error, Some("unsupportedTitle"));
        let install = tempfile::tempdir().unwrap();
        assert_eq!(state.import(GameId::Infinitas, install.path().to_path_buf()).unwrap_err().code, "unsupportedTitle");
    }
}
