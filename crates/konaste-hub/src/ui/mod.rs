//! Native interface (gpui).
mod art;
mod assets;
mod motion;
mod theme;
mod tray;
mod view;

use crate::{account::AccountStatus, hub::Hub, platform, state::HubState};
use gpui_kit::*;

pub fn run() {
    let directory = platform::data_directory();
    let _instance = match crate::instance::acquire(&directory) {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            crate::instance::request_activation(&directory);
            return;
        }
        Err(_) => startup_error("Could not open the Hub data folder. Check that it is writable.", &directory),
    };
    crate::log::init(&directory);
    let state = match HubState::load(directory.clone()) {
        Ok(state) => state,
        Err(error) => {
            crate::log::line(format!("startup settings: {}", error.code));
            startup_error(
                "Could not read settings.json. The Hub has left the file and game folders unchanged. Restore a known-good copy or include hub.log when asking for help.",
                &directory,
            );
        }
    };
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("tokio runtime");
    // Keeps tokio types (timers, reqwest) usable from UI-thread callbacks.
    let _enter = runtime.enter();
    let remember = state.stored.lock().unwrap().preferences.remember;
    state.session.load(remember);
    #[cfg(windows)]
    crate::browser::retire_stale(&directory);
    let (hub, receivers) = Hub::new(state, runtime.handle().clone());
    start_background(&hub);

    gpui_kit::application().with_assets(assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        gpui_kit::component::Theme::change(gpui_kit::component::ThemeMode::Dark, None, cx);
        let size = size(px(1280.), px(760.));
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(None, size, cx)),
            window_min_size: Some(gpui_kit::size(px(960.), px(600.))),
            titlebar: Some(TitlebarOptions { title: Some("Konaste Hub".into()), appears_transparent: true, traffic_light_position: None }),
            app_owns_titlebar_drag: true,
            window_background: WindowBackgroundAppearance::Opaque,
            ..Default::default()
        };
        let hub = hub.clone();
        let runtime_guard = std::sync::Arc::new(runtime);
        let mut receivers = Some(receivers);
        gpui_kit::open_window(options, cx, move |window, cx| {
            let keep = runtime_guard.clone();
            let receivers = receivers.take().expect("main window opens once");
            cx.new(|cx| view::App::new(hub.clone(), receivers, keep, window, cx))
        })
        .expect("open the main window");
    });
}

fn startup_error(message: &str, directory: &std::path::Path) -> ! {
    let message = format!("{message}\n\n{}", directory.display());
    eprintln!("Konaste Hub: {message}");
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        let title: Vec<_> = "Konaste Hub".encode_utf16().chain(Some(0)).collect();
        let text: Vec<_> = message.encode_utf16().chain(Some(0)).collect();
        unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR) };
    }
    std::process::exit(1);
}

fn start_background(hub: &Hub) {
    crate::maintenance::start_monitor(hub.clone());
    // A queue restored from settings starts its head once the app is running.
    crate::update::resume_queue(hub.clone());
    let account = hub.clone();
    hub.spawn(async move {
        if account.state().session.status() != AccountStatus::SignedOut {
            let _ = account.state().session.refresh(&account).await;
        }
    });
    let checks = hub.clone();
    hub.spawn(async move {
        let installed: Vec<_> = checks
            .state()
            .snapshot()
            .games
            .into_iter()
            .filter(|g| g.supported && g.phase == crate::state::GamePhase::Checking)
            .map(|g| g.id)
            .collect();
        // Titles are checked one after another; each result shows as soon as it is ready.
        for id in installed {
            if let Err(error) = crate::launch::check(checks.clone(), id).await
                && !matches!(error.code, "installationOutdated" | "checkChanged")
            {
                let _ = error;
            }
            if checks.state().snapshot().games.iter().any(|g| g.id == id && g.phase == crate::state::GamePhase::Update) {
                checks.notify(crate::hub::Notice::UpdateFound(id));
            }
        }
    });
}
