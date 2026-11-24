//! User-requested, unchanged per-title settings. No account or game launch is involved.
use crate::{
    catalog::GameId,
    hub::{Hub, Notice},
    platform,
    state::{HubError, SettingsTicket},
};
use konaste_bootstrap::GameFamily;
use konaste_runtime::SettingsPin;
use std::{path::Path, sync::Arc};
use tokio::sync::OwnedMutexGuard;

pub async fn open(app: Hub, id: GameId) -> Result<(), HubError> {
    if !id.has_settings() {
        return Err(HubError { code: "unsupportedTitle" });
    }
    let family = id.runtime_family().ok_or(HubError { code: "unsupportedTitle" })?;
    let state = app.state();
    let admission = Arc::clone(&state.launch_operation).try_lock_owned().map_err(|_| HubError { code: "launchBusy" })?;
    let ticket = state.begin_settings(id)?;
    let needs_discovery = ticket.pin.is_none();
    let mut lifecycle = Lifecycle { app: app.clone(), active: Some((ticket, admission)) };
    app.publish();
    let _metadata = if needs_discovery { Some(state.metadata_operation.lock().await) } else { None };
    let cache = state.directory.join("runtimes");
    // The workers own cleanup and admission even if the frontend stops awaiting.
    tokio::task::spawn_blocking(move || {
        let ticket = lifecycle.ticket();
        let root = &ticket.root;
        ensure_game_closed(id, root)?;
        ensure_closed(id, root)?;
        // A check without a settings record leaves no cached pin, so fresh
        // discovery is needed. It runs the pinned runtime worker, which needs
        // the official pair; fetch it once here. The cached-pin path never
        // reaches this branch and stays offline.
        let pin = match ticket.pin {
            Some(pin) => pin,
            None => discover_with_components(family, &cache)?,
        };
        // Title-support path check before any DLL copy or registry write. The
        // same helper Play uses to decide a root is usable.
        if !crate::official_setup::title_paths_ok(id, root) {
            return Err(HubError { code: "gameSettingsFailed" });
        }
        let prepared = konaste_runtime::prepare_settings(family, root, &pin).map_err(|_| HubError { code: "gameSettingsUnavailable" })?;
        // The DLL is a prerequisite of `settings.exe`, fetched only when it is
        // not already beside it; a present copy keeps this path offline.
        crate::official_setup::prepare(id, root, &cache)?;
        let mut session = lifecycle.app.state().commit_settings(ticket, || {
            prepared
                .start(|| {
                    ensure_game_closed(id, root)?;
                    ensure_closed(id, root)
                })
                .map_err(|error: HubError| {
                    if matches!(error.code, "gameRunning" | "gameSettingsBusy" | "processCheck") {
                        error
                    } else {
                        HubError { code: "gameSettingsFailed" }
                    }
                })
        })?;
        tokio::task::spawn_blocking(move || {
            if session.wait_for_window() && lifecycle.app.state().settings_visible(lifecycle.ticket()) {
                lifecycle.app.publish();
            }
            let result = session.wait();
            drop(session);
            lifecycle.finish((!matches!(result, Ok(0))).then_some("gameSettingsFailed"));
        });
        Ok(())
    })
    .await
    .map_err(|_| HubError { code: "gameSettingsFailed" })?
}

fn discover_pin(family: GameFamily, cache: &Path) -> Result<SettingsPin, HubError> {
    let executable = std::env::current_exe().map_err(|_| HubError { code: "metadataCheckFailed" })?;
    konaste_discovery::check(family, cache, &executable)
        .map_err(|error| HubError {
            code: match error {
                konaste_discovery::DiscoveryError::ComponentsMissing => "componentsMissing",
                konaste_discovery::DiscoveryError::Runtime => "componentsFailed",
                konaste_discovery::DiscoveryError::InvalidMetadata | konaste_discovery::DiscoveryError::Protocol => "metadataInvalid",
                konaste_discovery::DiscoveryError::Unsupported => "unsupportedTitle",
                _ => "metadataCheckFailed",
            },
        })?
        .settings_pin()
        .map_err(|_| HubError { code: "gameSettingsUnavailable" })
}

/// Discovery needs the pinned official runtime pair. Fetch it once when the
/// cache has no complete one; a cache hit never opens a socket. This runs only
/// when settings has no cached pin, so a cached check and Play are unaffected.
fn discover_with_components(family: GameFamily, cache: &Path) -> Result<SettingsPin, HubError> {
    let present = matches!(konaste_bootstrap::inspect(family, cache), Ok(konaste_bootstrap::PairStatus::Present(_)));
    if !present {
        konaste_bootstrap::acquire(family, cache, true, None).map_err(|_| HubError { code: "componentsMissing" })?;
    }
    discover_pin(family, cache)
}

pub fn ensure_closed(id: GameId, root: &Path) -> Result<(), HubError> {
    let Some(family) = id.runtime_family() else {
        return Ok(());
    };
    let Ok(relative) = konaste_runtime::settings_path(family) else {
        return Ok(());
    };
    if platform::game_running(&root.join(relative))? {
        return Err(HubError { code: "gameSettingsBusy" });
    }
    Ok(())
}

fn ensure_game_closed(id: GameId, root: &Path) -> Result<(), HubError> {
    let image = id.module_image(root).ok_or(HubError { code: "unsupportedTitle" })?;
    if platform::game_running(&image)? {
        return Err(HubError { code: "gameRunning" });
    }
    Ok(())
}

struct Lifecycle {
    app: Hub,
    active: Option<(SettingsTicket, OwnedMutexGuard<()>)>,
}
impl Lifecycle {
    fn ticket(&self) -> &SettingsTicket {
        &self.active.as_ref().expect("active settings identity").0
    }
    fn finish(&mut self, error: Option<&'static str>) {
        let Some((ticket, admission)) = self.active.take() else {
            return;
        };
        let changed = self.app.state().finish_settings(ticket);
        drop(admission);
        if changed {
            match error {
                Some(code) => self.app.notify(Notice::Error(code)),
                None => self.app.publish(),
            }
        }
    }
}
impl Drop for Lifecycle {
    fn drop(&mut self) {
        self.finish(None);
    }
}
