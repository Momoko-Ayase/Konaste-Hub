#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod account;
mod browser;
mod catalog;
mod game_settings;
mod hub;
mod i18n;
mod instance;
mod jar;
mod launch;
mod log;
mod maintenance;
mod news;
mod official_setup;
mod passkey;
mod platform;
mod playdata;
mod profile;
mod self_update;
mod signin;
mod state;
mod transfer;
mod ui;
mod update;

fn main() {
    platform::hold_app_mutex();
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.first().is_some_and(|value| value == konaste_runtime::WORKER_ARGUMENT) {
        let success = arguments.len() == 1 && konaste_runtime::worker_main().is_ok();
        std::process::exit(if success { 0 } else { 1 });
    }
    // Hidden development entry points; the GUI is still the normal start.
    if arguments.first().is_some_and(|value| value == "--headless-install") {
        std::process::exit(headless_install(&arguments));
    }
    if arguments.first().is_some_and(|value| value == "--headless-index") {
        std::process::exit(headless_index(&arguments));
    }
    ui::run();
}

/// `--headless-install <game-id> <dir> --data <state-dir>` runs the GUI install
/// path (components, update::install) against a separate state directory.
fn headless_install(arguments: &[std::ffi::OsString]) -> i32 {
    // --headless-install <game-id> <dir> --data <state-dir>
    if arguments.len() != 5 || arguments[3] != "--data" {
        eprintln!("usage: konaste-hub --headless-install <game-id> <dir> --data <state-dir>");
        return 2;
    }
    let id = match game_id(&arguments[1]) {
        Some(id) => id,
        None => {
            eprintln!("unknown game id");
            return 2;
        }
    };
    if id.title().is_none() {
        eprintln!("unsupported title");
        return 2;
    }
    let destination = std::path::PathBuf::from(&arguments[2]);
    let state_directory = std::path::PathBuf::from(&arguments[4]);
    let _instance = match crate::instance::acquire(&state_directory) {
        Ok(Some(lock)) => lock,
        _ => {
            eprintln!("Could not get exclusive access to the Hub data directory.");
            return 1;
        }
    };
    let state = match crate::state::HubState::load(state_directory) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("state: {}", error.code);
            return 1;
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("runtime");
            return 1;
        }
    };
    let (hub, _receivers) = crate::hub::Hub::new(state, runtime.handle().clone());
    runtime.block_on(async {
        if let Err(error) = crate::update::install(hub.clone(), id, destination).await {
            eprintln!("install: {}", error.code);
            return 1;
        }
        let started = std::time::Instant::now();
        let mut last_line = started - std::time::Duration::from_secs(11);
        let mut download_start = None::<std::time::Instant>;
        let mut download_seconds = 0_u64;
        let mut bytes = 0_u64;
        loop {
            let snapshot = hub.state().snapshot();
            let Some(game) = snapshot.games.iter().find(|game| game.id == id) else {
                eprintln!("state: snapshot");
                return 1;
            };
            if let Some(transfer) = &game.transfer {
                bytes = bytes.max(transfer.bytes_received);
                if download_start.is_none() && transfer.bytes_received > 0 {
                    download_start = Some(std::time::Instant::now());
                }
            }
            if download_start.is_some() && game.phase != crate::state::GamePhase::Downloading {
                download_seconds = download_seconds.max(download_start.take().map(|at| at.elapsed().as_secs()).unwrap_or(0));
            }
            if last_line.elapsed() >= std::time::Duration::from_secs(10) {
                last_line = std::time::Instant::now();
                let work = game.work.as_ref();
                println!(
                    "phase={:?} progress={} bytes={}/{}{} files={}/{}{}",
                    game.phase,
                    game.progress,
                    game.transfer.as_ref().map(|t| t.bytes_received).unwrap_or_else(|| work.map(|w| w.bytes_completed).unwrap_or(0)),
                    game.transfer.as_ref().map(|t| t.bytes_total).unwrap_or_else(|| work.map(|w| w.bytes_total).unwrap_or(0)),
                    game.transfer.as_ref().and_then(|t| t.bytes_per_second).map(|rate| format!(" speed={rate}B/s")).unwrap_or_default(),
                    game.transfer.as_ref().map(|t| t.files_completed).unwrap_or_else(|| work.map(|w| w.files_completed).unwrap_or(0)),
                    game.transfer.as_ref().map(|t| t.files_total).unwrap_or_else(|| work.map(|w| w.files_total).unwrap_or(0)),
                    game.error.map(|code| format!(" error={code}")).unwrap_or_default(),
                );
            }
            if !hub.state().updates.busy(id)
                && matches!(game.phase, crate::state::GamePhase::Ready | crate::state::GamePhase::Blocked | crate::state::GamePhase::Paused)
            {
                if let Some(transfer) = &game.transfer {
                    bytes = bytes.max(transfer.bytes_received);
                }
                let elapsed = started.elapsed();
                let rate = (download_seconds > 0).then(|| bytes / download_seconds);
                println!(
                    "done phase={:?} version={} bytes={} seconds={} avg_bytes_per_second={} error={}",
                    game.phase,
                    game.installed_version.clone().unwrap_or_default(),
                    bytes,
                    elapsed.as_secs(),
                    rate.map(|value| value.to_string()).unwrap_or_else(|| "-".into()),
                    game.error.unwrap_or("-"),
                );
                return if game.error.is_none() && game.installed_version.is_some() { 0 } else { 1 };
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    })
}

/// `--headless-index <game-id> --data <state-dir>` prints settings-related
/// entries from the fresh authenticated index; it installs nothing.
fn headless_index(arguments: &[std::ffi::OsString]) -> i32 {
    if arguments.len() != 4 || arguments[2] != "--data" {
        eprintln!("usage: konaste-hub --headless-index <game-id> --data <state-dir>");
        return 2;
    }
    let id = match game_id(&arguments[1]) {
        Some(id) => id,
        None => {
            eprintln!("unknown game id");
            return 2;
        }
    };
    let family = match id.runtime_family() {
        Some(family) => family,
        None => {
            eprintln!("unsupported title");
            return 2;
        }
    };
    let state_directory = std::path::PathBuf::from(&arguments[3]);
    let _instance = match crate::instance::acquire(&state_directory) {
        Ok(Some(lock)) => lock,
        _ => {
            eprintln!("Could not get exclusive access to the Hub data directory.");
            return 1;
        }
    };
    let state = match crate::state::HubState::load(state_directory) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("state: {}", error.code);
            return 1;
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(_) => return 1,
    };
    let (hub, _receivers) = crate::hub::Hub::new(state, runtime.handle().clone());
    if let Err(error) = runtime.block_on(crate::launch::ensure_components(&hub, family)) {
        eprintln!("components: {}", error.code);
        return 1;
    }
    // Discovery and its blocking HTTP client must run outside a runtime context.
    let cache = hub.state().directory.join("runtimes");
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(_) => return 1,
    };
    let metadata = match konaste_discovery::check(family, &cache, &executable) {
        Ok(metadata) => metadata,
        Err(error) => {
            eprintln!("metadata: {error}");
            return 1;
        }
    };
    println!("revision={}", metadata.index.revision);
    for file in metadata
        .index
        .files
        .iter()
        .filter(|file| file.path.ends_with("/settings.exe") || file.path == "settings.exe" || file.path.ends_with("/libsettings.dll"))
    {
        println!(
            "file={} bytes={} sha256={}",
            file.path,
            file.stored_size,
            file.stored_sha256.iter().map(|byte| format!("{byte:02x}")).collect::<String>()
        );
    }
    0
}

fn game_id(value: &std::ffi::OsStr) -> Option<crate::catalog::GameId> {
    value.to_str().and_then(|value| match value {
        "bomber-girl" | "bombergirl" => Some(crate::catalog::GameId::BomberGirl),
        "sdvx" => Some(crate::catalog::GameId::Sdvx),
        "ddr" => Some(crate::catalog::GameId::Ddr),
        "popn" => Some(crate::catalog::GameId::Popn),
        "gitadora" => Some(crate::catalog::GameId::Gitadora),
        "nostalgia" => Some(crate::catalog::GameId::Nostalgia),
        _ => None,
    })
}
