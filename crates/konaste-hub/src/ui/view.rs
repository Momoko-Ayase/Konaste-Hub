//! Main window: rail, game stage, overlays.

use super::art::{self, Service};
use super::motion::{self, MOVE, SETTLE};
use super::theme::{self, icons};
use crate::{
    account::{AccountStatus, COOP, COURSE, MY_KONAMI, MYPAGE, PASELI_CHARGE, PASS_MENU},
    catalog::{CATALOG, GameId},
    hub::{Hub, Notice, Receivers},
    i18n::{self, K, Locale, fill, t},
    news::{self, News},
    playdata::{self, Kind as StatKind, PlayData},
    profile::{format_card, mask_card},
    state::{GamePhase, GameSnapshot, Snapshot},
};
use gpui_kit::{prelude::FluentBuilder, *};
use std::{
    cell::RefCell,
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, PartialEq)]
enum Overlay {
    Drawer,
    Downloads,
    Settings(Section),
    Add,
    Install(GameId),
    Remove(GameId),
    Instrument(GameId),
    /// What the game login page asked before Play could start.
    Gate(GameId, Rc<crate::account::Gate>),
    SignIn,
}

#[derive(Clone, Copy, PartialEq)]
enum Section {
    General,
    Downloads,
    Account,
    About,
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    News,
    Details,
}

struct Toast {
    id: u64,
    text: String,
    action: Option<GameId>,
    leaving: bool,
    /// When it leaves on its own. Errors stay longer so they can be read.
    until: Instant,
}

pub struct App {
    hub: Hub,
    snapshot: Snapshot,
    selected: Option<GameId>,
    /// The game shown before the current one; its art stays underneath while the new art fades in.
    previous: Option<GameId>,
    overlay: Option<Overlay>,
    /// An overlay playing its exit animation.
    closing: Option<Overlay>,
    /// Open dropdown in Settings.
    select: Option<&'static str>,
    /// A popup closed by an outside press; the same press must not reopen it.
    dismissed: Option<(&'static str, Instant)>,
    hovered: Option<SharedString>,
    tab: Tab,
    menu: bool,
    reveal_card: bool,
    toasts: Vec<Toast>,
    toast_seq: u64,
    news: HashMap<GameId, Option<News>>,
    images: HashMap<String, Option<Arc<Image>>>,
    /// Play data fetches started by this window; the data itself is cached in the session.
    stat_reads: std::collections::HashSet<GameId>,
    banner: usize,
    banner_previous: Option<usize>,
    /// Decoded before first use, so switching games never shows a half-loaded backdrop.
    fallback: HashMap<GameId, Arc<RenderImage>>,
    backgrounds: HashMap<GameId, Arc<RenderImage>>,
    icons: HashMap<GameId, Arc<RenderImage>>,
    /// Icon reads already started, by install root.
    icon_reads: HashMap<GameId, PathBuf>,
    services: HashMap<Service, Arc<RenderImage>>,
    pass_card: Option<Arc<RenderImage>>,
    install_path: PathBuf,
    install_free: Option<u64>,
    install_submitting: bool,
    settings_tab: Section,
    news_client: reqwest::Client,
    _runtime: Arc<tokio::runtime::Runtime>,
    focus: FocusHandle,
    /// The native sign-in in progress.
    sign_in: Option<SignIn>,
    /// A newer Hub release, and whether its setup is being fetched.
    hub_update: Option<crate::self_update::Release>,
    hub_updating: bool,
    konami: u8,
    #[cfg(windows)]
    tray: Rc<RefCell<super::tray::Tray>>,
}

/// The sign-in dialog: the flow, its fields, and the decoded pictures.
struct SignIn {
    flow: crate::signin::Flow,
    id: Entity<gpui_kit::component::input::InputState>,
    password: Entity<gpui_kit::component::input::InputState>,
    code: Entity<gpui_kit::component::input::OtpState>,
    /// The captcha generation these images belong to; the sample first.
    pictures: (u32, Vec<Option<Arc<Image>>>),
    /// Play this game once signed in.
    then: Option<GameId>,
    /// The Windows passkey dialog's answer, from its own thread.
    passkey: Option<std::sync::mpsc::Receiver<Result<String, crate::signin::PasskeyFailure>>>,
    /// The hidden official page has been asked to load.
    opened: bool,
    /// The session generation this attempt belongs to, captured when it
    /// started. Cookies it collects are adopted only while it still holds.
    session: u64,
    _subscriptions: Vec<Subscription>,
}

/// Browser work the UI loop does after a sign-in update.
#[derive(Default)]
struct Outgoing {
    /// Profile directory, and whether the page may persist anything there.
    open: Option<(PathBuf, bool)>,
    close: bool,
    calls: Vec<crate::signin::Call>,
    navigate: Option<String>,
}

impl App {
    pub fn new(hub: Hub, receivers: Receivers, runtime: Arc<tokio::runtime::Runtime>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let snapshot = hub.state().snapshot();
        let selected = snapshot.preferences.selected.filter(|id| installed(&snapshot, *id)).or_else(|| first_installed(&snapshot));
        let news_client = reqwest::Client::builder()
            .https_only(true)
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) KonasteHub/0.3")
            .timeout(Duration::from_secs(10))
            .build()
            .expect("news client");
        let mut app = Self {
            hub,
            snapshot,
            selected,
            previous: None,
            overlay: None,
            closing: None,
            select: None,
            dismissed: None,
            hovered: None,
            tab: Tab::News,
            menu: false,
            reveal_card: false,
            toasts: Vec::new(),
            toast_seq: 0,
            news: HashMap::new(),
            images: HashMap::new(),
            stat_reads: std::collections::HashSet::new(),
            banner: 0,
            banner_previous: None,
            fallback: HashMap::new(),
            backgrounds: HashMap::new(),
            icons: HashMap::new(),
            icon_reads: HashMap::new(),
            services: HashMap::new(),
            pass_card: None,
            install_path: PathBuf::new(),
            install_free: None,
            install_submitting: false,
            settings_tab: Section::General,
            news_client,
            _runtime: runtime,
            focus: cx.focus_handle(),
            sign_in: None,
            hub_update: None,
            hub_updating: false,
            konami: 0,
            #[cfg(windows)]
            tray: Rc::new(RefCell::new(super::tray::Tray::new())),
        };
        app.listen(receivers, window, cx);
        app.close_behavior(window, cx);
        app.load_art(cx);
        app.load_icons(cx);
        app.watch_hub_releases(cx);
        if let Some(id) = app.selected {
            app.load_news(id, cx);
            app.load_playdata(id, cx);
        }
        window.focus(&app.focus, cx);
        app
    }

    fn locale(&self) -> Locale {
        Locale::from_code(&self.snapshot.preferences.locale)
    }

    fn tr(&self, key: K) -> &'static str {
        t(self.locale(), key)
    }

    /// Pull state changes and notices from background work onto the UI thread.
    fn listen(&mut self, receivers: Receivers, window: &mut Window, cx: &mut Context<Self>) {
        let Receivers { mut changed, mut notices } = receivers;
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(120)).await;
                let dirty = changed.has_changed().unwrap_or(false);
                if dirty {
                    changed.borrow_and_update();
                }
                let mut batch = Vec::new();
                while let Ok(notice) = notices.try_recv() {
                    batch.push(notice);
                }
                #[cfg(windows)]
                {
                    // Browser calls stay outside gpui updates: WebView2 may pump messages.
                    let events = crate::browser::poll();
                    let out = this.update_in(cx, |app, window, cx| app.pump_sign_in(events, window, cx)).unwrap_or_default();
                    if out.close {
                        // Also deletes the profile it used.
                        crate::browser::close();
                    }
                    if let Some((data, remember)) = out.open
                        && crate::browser::open(data, remember).is_err()
                    {
                        let _ = this.update(cx, |app, cx| app.sign_in_failed(cx));
                    }
                    for call in &out.calls {
                        crate::browser::send(call);
                    }
                    if let Some(url) = out.navigate {
                        crate::browser::navigate(&url);
                    }
                    let request = this
                        .update(cx, |app, _| {
                            app.tray.borrow().poll().or_else(|| {
                                crate::instance::take_activation(&app.hub.state().directory).then_some(super::tray::Request::Show)
                            })
                        })
                        .ok()
                        .flatten();
                    match request {
                        Some(super::tray::Request::Show) => {
                            let _ = this.update_in(cx, |app, window, cx| {
                                if let Some(hwnd) = hwnd_of(window) {
                                    app.tray.borrow_mut().restore(hwnd);
                                }
                                window.activate_window();
                                cx.notify();
                            });
                        }
                        Some(super::tray::Request::Quit) => {
                            let _ = this.update_in(cx, |app, window, cx| {
                                let state = app.hub.state();
                                if state.has_running_game() || state.update_active() {
                                    if let Some(hwnd) = hwnd_of(window) {
                                        app.tray.borrow_mut().restore(hwnd);
                                    }
                                    window.activate_window();
                                    app.hub.notify(Notice::Error("quitBusy"));
                                } else {
                                    cx.quit();
                                }
                            });
                        }
                        None => {}
                    }
                }
                if dirty || !batch.is_empty() {
                    let alive = this.update(cx, |app, cx| {
                        app.snapshot = app.hub.state().snapshot();
                        if app.selected.is_none_or(|id| !installed(&app.snapshot, id)) {
                            app.selected = first_installed(&app.snapshot);
                        }
                        for notice in batch {
                            app.notice(notice);
                        }
                        if let Some(id) = app.selected {
                            app.load_playdata(id, cx);
                        }
                        app.load_icons(cx);
                        cx.notify();
                    });
                    if alive.is_err() {
                        break;
                    }
                }
            }
        })
        .detach();
        // Banner rotation and toast expiry.
        cx.spawn_in(window, async move |this, cx| {
            let mut tick = 0u64;
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                tick += 1;
                let alive = this.update(cx, |app, cx| {
                    if tick.is_multiple_of(6) {
                        app.banner_previous = Some(app.banner);
                        app.banner = app.banner.wrapping_add(1);
                        cx.notify();
                    }
                    let now = Instant::now();
                    let expired: Vec<u64> = app.toasts.iter().filter(|t| !t.leaving && t.until <= now).map(|t| t.id).collect();
                    for id in expired {
                        app.dismiss_toast(id, cx);
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn close_behavior(&self, window: &mut Window, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        #[cfg(windows)]
        let tray = Rc::clone(&self.tray);
        window.on_window_should_close(cx, move |window, _| {
            let state = hub.state();
            let to_tray = state.stored.lock().unwrap().preferences.close_to_tray;
            if !to_tray && !state.has_running_game() && !state.update_active() {
                return true;
            }
            #[cfg(windows)]
            {
                let locale = Locale::from_code(&state.stored.lock().unwrap().preferences.locale);
                let mut tray = tray.borrow_mut();
                if let Some(hwnd) = hwnd_of(window) {
                    tray.hide_to_tray(hwnd, t(locale, K::show), t(locale, K::quit));
                }
                if tray.visible() {
                    return false;
                }
            }
            // No tray available: keep running, out of the way.
            window.minimize_window();
            false
        });
    }

    fn notice(&mut self, notice: Notice) {
        let locale = self.locale();
        let title = |id: GameId| crate::catalog::descriptor(id).title;
        let lasts = Duration::from_secs(if matches!(notice, Notice::Error(_) | Notice::GameExited { .. }) { 12 } else { 6 });
        let (text, action) = match notice {
            Notice::Error(code) => (i18n::error(locale, code).to_owned(), None),
            Notice::UpdateFound(id) => (fill(t(locale, K::update_found), &[title(id)]), Some(id)),
            Notice::Installed(id) => (fill(t(locale, K::installed), &[title(id)]), (self.selected != Some(id)).then_some(id)),
            Notice::GameExited { id, code } => (fill(t(locale, K::game_exited), &[title(id), &format!("0x{code:08X}")]), None),
            Notice::SignedOut => (t(locale, K::signed_out).to_owned(), None),
        };
        self.toast_seq += 1;
        self.toasts.push(Toast { id: self.toast_seq, text, action, leaving: false, until: Instant::now() + lasts });
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
    }

    fn dismiss_toast(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(toast) = self.toasts.iter_mut().find(|t| t.id == id && !t.leaving) else { return };
        toast.leaving = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(motion::LEAVE_MS)).await;
            let _ = this.update(cx, |app, cx| {
                app.toasts.retain(|t| t.id != id);
                cx.notify();
            });
        })
        .detach();
    }

    fn open(&mut self, overlay: Overlay, cx: &mut Context<Self>) {
        self.menu = false;
        self.select = None;
        if self.closing.as_ref() == Some(&overlay) {
            self.closing = None;
        }
        if overlay == Overlay::Add {
            self.snapshot = self.hub.state().snapshot();
        }
        self.overlay = Some(overlay);
        cx.notify();
    }

    fn close_overlay(&mut self, cx: &mut Context<Self>) {
        self.select = None;
        if self.overlay == Some(Overlay::SignIn) && self.sign_in.is_some() {
            self.cancel_sign_in();
        }
        let Some(overlay) = self.overlay.take() else { return };
        self.closing = Some(overlay.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(motion::LEAVE_MS)).await;
            let _ = this.update(cx, |app, cx| {
                if app.closing.as_ref() == Some(&overlay) {
                    app.closing = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Toggle a popup; a press that just dismissed it from outside does not reopen it.
    fn toggle_popup(&mut self, key: &'static str) -> bool {
        if self.dismissed.take().is_some_and(|(k, at)| k == key && at.elapsed() < Duration::from_millis(250)) {
            return false;
        }
        true
    }

    fn hover_listener(
        &self,
        key: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> impl Fn(&bool, &mut Window, &mut gpui_kit::App) + 'static {
        let key = key.into();
        cx.listener(move |app, hovered: &bool, _, cx| {
            if *hovered {
                app.hovered = Some(key.clone());
            } else if app.hovered.as_ref() == Some(&key) {
                app.hovered = None;
            }
            cx.notify();
        })
    }

    /// A select box. The list opens below the trigger and closes on an outside press.
    fn dropdown(
        &self,
        key: &'static str,
        options: Vec<(SharedString, bool)>,
        cx: &mut Context<Self>,
        pick: impl Fn(&mut Self, usize, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let open = self.select == Some(key);
        let current = options.iter().find(|(_, on)| *on).map(|(label, _)| label.clone()).unwrap_or_default();
        let pick = std::rc::Rc::new(pick);
        let trigger = div()
            .id(SharedString::from(format!("{key}-trigger")))
            .h(px(34.))
            .min_w(px(180.))
            .pl(px(12.))
            .pr(px(8.))
            .rounded(px(7.))
            .bg(rgba(0x00000080))
            .border_1()
            .border_color(if open { Hsla::from(rgba(0xffffff40)) } else { theme::line() })
            .flex()
            .items_center()
            .justify_between()
            .gap(px(10.))
            .cursor_pointer()
            .hover(|s| s.border_color(rgba(0xffffff33)))
            .on_click(cx.listener(move |app, _, _, cx| {
                if app.toggle_popup(key) {
                    app.select = if app.select == Some(key) { None } else { Some(key) };
                }
                cx.notify();
            }))
            .child(current)
            .child(motion::spring(
                div().size(px(16.)).flex().items_center().justify_center().child(theme::icon(icons::CHEVRON_DOWN, 15., theme::muted())),
                SharedString::from(format!("{key}-chevron")),
                MOVE,
                0.,
                if open { 1. } else { 0. },
                // A quarter turn would need a transform; tilt via vertical nudge and fade instead.
                |el, t| el.opacity(1. - 0.35 * t).mt(px(-2. * t)),
            ));
        let mut wrap = div().relative().child(trigger);
        if open {
            let mut list = div()
                .id(SharedString::from(format!("{key}-list")))
                .occlude()
                .min_w(px(180.))
                .p(px(4.))
                .rounded(px(8.))
                .bg(theme::panel_solid())
                .border_1()
                .border_color(rgba(0xffffff1f))
                .shadow(theme::shadow(12., 30., 0.55))
                .on_mouse_down_out(cx.listener(move |app, _, _, cx| {
                    if app.select == Some(key) {
                        app.select = None;
                        app.dismissed = Some((key, Instant::now()));
                        cx.notify();
                    }
                }));
            for (ix, (label, on)) in options.into_iter().enumerate() {
                let pick = pick.clone();
                list = list.child(motion::rise_after(
                    div()
                        .id((SharedString::from(format!("{key}-opt")), ix))
                        .relative()
                        .h(px(32.))
                        .px(px(10.))
                        .rounded(px(5.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .cursor_pointer()
                        .text_color(if on { theme::text() } else { Hsla::from(rgb(0xc9cbd1)) })
                        .when(on, |d| d.bg(rgba(0xffffff0f)))
                        .hover(|s| s.bg(rgba(0xffffff1a)).text_color(theme::text()))
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.select = None;
                            pick(app, ix, cx);
                            cx.notify();
                        }))
                        .child(label)
                        .when(on, |d| d.child(theme::icon(icons::CHECK, 14., theme::accent()))),
                    (SharedString::from(format!("{key}-opt-in")), ix),
                    ix as u64 * 18,
                    180,
                    -4.,
                ));
            }
            wrap = wrap.child(
                div().absolute().top(px(38.)).right_0().child(
                    deferred(list.with_animation(SharedString::from(format!("{key}-drop")), motion::ease(180), |el, t| {
                        el.opacity(t).mt(px(-6. * (1. - t)))
                    }))
                    .with_priority(2),
                ),
            );
        }
        wrap.into_any_element()
    }

    fn is_hovered(&self, key: &str) -> bool {
        self.hovered.as_deref() == Some(key)
    }

    fn select(&mut self, id: GameId, cx: &mut Context<Self>) {
        if self.selected != Some(id) {
            self.previous = self.selected;
            self.banner_previous = None;
        }
        self.selected = Some(id);
        self.menu = false;
        self.banner = 0;
        self.load_news(id, cx);
        self.load_playdata(id, cx);
        let mut preferences = self.snapshot.preferences.clone();
        preferences.selected = Some(id);
        let _ = self.hub.state().commit_preferences(preferences);
        cx.notify();
    }

    fn load_news(&mut self, id: GameId, cx: &mut Context<Self>) {
        if self.news.contains_key(&id) {
            return;
        }
        self.news.insert(id, None);
        let client = self.news_client.clone();
        let runtime = self.hub.clone();
        let (tx, rx) = futures_channel();
        runtime.spawn(async move {
            let _ = tx.send(news::fetch(&client, id).await);
        });
        cx.spawn(async move |this, cx| {
            if let Ok(result) = rx.await {
                let _ = this.update(cx, |app, cx| {
                    let banners: Vec<String> =
                        result.as_ref().map(|n| n.banners.iter().map(|b| b.image.clone()).collect()).unwrap_or_default();
                    app.news.insert(id, Some(result.unwrap_or_default()));
                    for url in banners {
                        app.load_image(url, cx);
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Read a title's play data once per cache window while signed in.
    fn load_playdata(&mut self, id: GameId, cx: &mut Context<Self>) {
        if self.snapshot.account != AccountStatus::SignedIn || !playdata::supported(id) {
            return;
        }
        if self.stat_reads.contains(&id) {
            return;
        }
        // The cache decides freshness; cached data stays visible while it refreshes.
        let Some(epoch) = self.hub.state().session.playdata.begin(id) else { return };
        self.stat_reads.insert(id);
        let hub = self.hub.clone();
        let (tx, rx) = futures_channel();
        let task = hub.clone();
        hub.spawn(async move {
            let _ = tx.send(playdata::fetch(&task, id).await);
        });
        cx.spawn(async move |this, cx| {
            let data = rx.await.ok().flatten();
            let _ = this.update(cx, |app, cx| {
                app.stat_reads.remove(&id);
                app.hub.state().session.playdata.finish(id, epoch, data);
                cx.notify();
            });
        })
        .detach();
    }

    fn load_image(&mut self, url: String, cx: &mut Context<Self>) {
        if self.images.contains_key(&url) {
            return;
        }
        self.images.insert(url.clone(), None);
        let client = self.news_client.clone();
        let (tx, rx) = futures_channel();
        let fetch_url = url.clone();
        self.hub.spawn(async move {
            let bytes = async {
                let response = client.get(&fetch_url).send().await.ok()?;
                if !response.status().is_success() {
                    return None;
                }
                let bytes = response.bytes().await.ok()?;
                (bytes.len() <= 2 * 1024 * 1024).then(|| bytes.to_vec())
            }
            .await;
            let _ = tx.send(bytes);
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Some(bytes)) = rx.await {
                let format = image_format(&bytes);
                let _ = this.update(cx, |app, cx| {
                    if let Some(format) = format {
                        app.images.insert(url, Some(Arc::new(Image::from_bytes(format, bytes))));
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    /// Fallback art, cached backgrounds, and bundled service logos are decoded
    /// off the UI thread before they are shown.
    fn load_art(&mut self, cx: &mut Context<Self>) {
        let renderer = cx.svg_renderer();
        let directory = self.hub.state().directory.clone();
        let background_directory = directory.clone();
        // Soft gradients scale up cleanly; a fixed size keeps seven of them near 40 MB.
        let (width, height) = (art::FALLBACK.0 as i32, art::FALLBACK.1 as i32);
        let mut order: Vec<GameId> = CATALOG.iter().map(|g| g.id).collect();
        // The visible title first.
        order.sort_by_key(|id| Some(*id) != self.selected);
        let fallbacks = cx.background_executor().spawn(async move {
            let images = order
                .into_iter()
                .filter_map(|id| {
                    let svg = renderer.parse_svg(art::fallback_svg(id)).ok()?;
                    let size = gpui_kit::size(DevicePixels(width), DevicePixels(height));
                    renderer.render_parsed(&svg, SvgSize::ExactSize(size)).ok().map(|image| (id, image))
                })
                .collect::<Vec<_>>();
            let (w, h) = art::PASS_CARD_PX;
            let pass = renderer
                .parse_svg(art::PASS_CARD)
                .ok()
                .and_then(|svg| renderer.render_parsed(&svg, SvgSize::ExactSize(gpui_kit::size(DevicePixels(w), DevicePixels(h)))).ok());
            (images, pass)
        });
        let backgrounds = cx.background_executor().spawn(async move {
            CATALOG
                .iter()
                .filter_map(|game| {
                    let image = art::cached_background(&background_directory, game.id)?;
                    Some((game.id, art::render_image(image)))
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let (images, pass) = fallbacks.await;
            let backgrounds = backgrounds.await;
            let _ = this.update(cx, |app, cx| {
                app.fallback.extend(images);
                app.pass_card = pass;
                // A fresh download may already be in place; it wins over the cache.
                for (id, image) in backgrounds {
                    app.backgrounds.entry(id).or_insert(image);
                }
                cx.notify();
            });
        })
        .detach();

        let logos = cx.background_executor().spawn(async move {
            Service::ALL
                .into_iter()
                .filter_map(|service| Some((service, art::render_image(art::service_logo(service)?))))
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let logos = logos.await;
            let _ = this.update(cx, |app, cx| {
                app.services.extend(logos);
                cx.notify();
            });
        })
        .detach();

        // Cached backgrounds show first; a newer copy from the repository replaces them.
        for game in &CATALOG {
            let id = game.id;
            let client = self.news_client.clone();
            let cache = art::background_file(&directory, id);
            let (tx, rx) = futures_channel();
            self.hub.spawn(async move {
                let Some(bytes) = art::refresh(&client, &cache, &art::background_url(id)).await else { return };
                let image = tokio::task::spawn_blocking(move || {
                    let image = art::decode_background(&bytes);
                    if image.is_none() {
                        art::discard(&cache);
                    }
                    image.map(art::render_image)
                })
                .await
                .ok()
                .flatten();
                let _ = tx.send(image);
            });
            cx.spawn(async move |this, cx| {
                if let Ok(Some(image)) = rx.await {
                    let _ = this.update(cx, |app, cx| {
                        app.backgrounds.insert(id, image);
                        cx.notify();
                    });
                }
            })
            .detach();
        }
    }

    /// Look for a newer Hub release at startup and every few hours. A setup
    /// left by an earlier update is removed first.
    fn watch_hub_releases(&mut self, cx: &mut Context<Self>) {
        let directory = self.hub.state().directory.clone();
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let client = crate::self_update::client();
            let (tx, rx) = futures_channel();
            hub.spawn(async move {
                let _ = tokio::task::spawn_blocking(move || crate::self_update::clean(&directory)).await;
                let _ = tx.send(());
            });
            let _ = rx.await;
            loop {
                let (tx, rx) = futures_channel();
                let request = client.clone();
                hub.spawn(async move {
                    let _ = tx.send(crate::self_update::check(&request).await);
                });
                if let Ok(Some(release)) = rx.await {
                    let alive = this.update(cx, |app, cx| {
                        if app.hub_update.as_ref().is_none_or(|known| known.version < release.version) {
                            app.hub_update = Some(release);
                            cx.notify();
                        }
                    });
                    if alive.is_err() {
                        return;
                    }
                }
                cx.background_executor().timer(crate::self_update::CHECK_INTERVAL).await;
            }
        })
        .detach();
    }

    /// Download and verify the new setup, then quit so it can replace the Hub.
    /// Refused while a game, its settings, or a download is active.
    fn apply_hub_update(&mut self, cx: &mut Context<Self>) {
        let Some(release) = self.hub_update.clone() else { return };
        if self.hub_updating {
            return;
        }
        let state = self.hub.state();
        if state.has_running_game() || state.update_active() {
            self.notice(Notice::Error("hubUpdateBusy"));
            cx.notify();
            return;
        }
        self.hub_updating = true;
        cx.notify();
        let directory = state.directory.clone();
        let (tx, rx) = futures_channel();
        self.hub.spawn(async move {
            let client = crate::self_update::client();
            let _ = tx.send(crate::self_update::download(&client, &release, &directory).await);
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await.unwrap_or(Err("hubUpdateDownload"));
            let _ = this.update(cx, |app, cx| {
                // A game or download may have started while the setup was fetched.
                let state = app.hub.state();
                let result = result.and_then(|setup| {
                    if state.has_running_game() || state.update_active() { Err("hubUpdateBusy") } else { crate::self_update::run(&setup) }
                });
                match result {
                    Ok(()) => cx.quit(),
                    Err(code) => {
                        app.hub_updating = false;
                        app.notice(Notice::Error(code));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// Read each installed title's official icon from its own executables, once per install root.
    fn load_icons(&mut self, cx: &mut Context<Self>) {
        for game in &self.snapshot.games {
            let Some(root) = game.install_dir.clone() else { continue };
            if self.icon_reads.get(&game.id) == Some(&root) {
                continue;
            }
            let id = game.id;
            self.icon_reads.insert(id, root.clone());
            let task = cx.background_executor().spawn(async move { art::install_icon(&root, id).map(art::render_image) });
            cx.spawn(async move |this, cx| {
                let icon = task.await;
                let _ = this.update(cx, |app, cx| {
                    match icon {
                        Some(icon) => {
                            app.icons.insert(id, icon);
                        }
                        None => {
                            app.icons.remove(&id);
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// The stage background covers the stage using its original composition.
    fn backdrop_image(&self, id: GameId) -> Option<Arc<RenderImage>> {
        self.backgrounds.get(&id).or_else(|| self.fallback.get(&id)).cloned()
    }

    fn backdrop(&self, id: GameId) -> Div {
        let layer = div().absolute().top_0().left_0().size_full();
        match self.backdrop_image(id) {
            Some(image) => layer.child(img(image).size_full().object_fit(ObjectFit::Cover)),
            None => {
                let (a, b) = colors(id);
                layer.bg(linear_gradient(135., linear_color_stop(rgb(a), 0.), linear_color_stop(rgb(b), 1.)))
            }
        }
    }

    fn game_icon(&self, id: GameId, size: f32) -> Div {
        match self.icons.get(&id) {
            Some(icon) => div()
                .size(px(size))
                .flex_none()
                .rounded(px(size * 0.27))
                .overflow_hidden()
                .child(img(icon.clone()).size_full().rounded(px(size * 0.27)).object_fit(ObjectFit::Cover)),
            None => game_icon(id, size),
        }
    }

    fn service_mark(&self, service: Service, height: f32) -> Option<AnyElement> {
        let logo = self.services.get(&service)?.clone();
        let size = logo.size(0);
        let width = height * size.width.0 as f32 / size.height.0.max(1) as f32;
        let mark = img(logo).w(px(width)).h(px(height)).flex_none();
        Some(match service {
            Service::Paseli => mark.rounded(px(height * 0.22)).into_any_element(),
            Service::EAmusement => mark.into_any_element(),
        })
    }

    // ---------- actions ----------

    fn play(&mut self, id: GameId, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = false;
        if self.snapshot.account == AccountStatus::Unknown {
            // The saved session is still being restored.
            return;
        }
        if self.snapshot.account != AccountStatus::SignedIn {
            self.sign_in(Some(id), window, cx);
            return;
        }
        if id == GameId::Gitadora && self.snapshot.preferences.gitadora_mode.is_none() {
            self.open(Overlay::Instrument(id), cx);
            cx.notify();
            return;
        }
        self.start(id, cx);
    }

    /// Start Play. When the game login page asks something first (terms of
    /// service, a still-running game), it is shown here as a dialog.
    fn start(&mut self, id: GameId, cx: &mut Context<Self>) {
        self.launch(id, None, cx);
    }

    fn launch(&mut self, id: GameId, answer: Option<crate::account::Answer>, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        cx.spawn(async move |this, cx| {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let worker = hub.clone();
            hub.spawn(async move {
                let _ = sender.send(crate::launch::play(worker.clone(), id, answer).await);
            });
            let result = receiver.await.unwrap_or(Err(crate::state::HubError { code: "launchVerificationFailed" }));
            match result {
                Ok(()) => {}
                Err(error) if error.code == "officialPage" => {
                    let gate = hub.state().session.take_gate(id).unwrap_or(crate::account::Gate::Unknown);
                    let _ = this.update(cx, |app, cx| app.open(Overlay::Gate(id, Rc::new(gate)), cx));
                }
                Err(error) => hub.notify(Notice::Error(error.code)),
            }
        })
        .detach();
    }

    fn answer_gate(&mut self, id: GameId, answer: crate::account::Answer, cx: &mut Context<Self>) {
        self.close_overlay(cx);
        self.launch(id, Some(answer), cx);
        cx.notify();
    }

    /// Open the native sign-in dialog. The official page loads in the hidden
    /// browser on the next UI tick.
    fn sign_in(&mut self, then: Option<GameId>, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_kit::component::input::{InputEvent, InputState, OtpEvent, OtpState};
        if let Some(state) = self.sign_in.as_mut() {
            if then.is_some() {
                state.then = then;
            }
            self.open(Overlay::SignIn, cx);
            return;
        }
        let id = cx.new(|cx| InputState::new(window, cx).placeholder(t(self.locale(), K::login_id)));
        let password = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder(t(self.locale(), K::password)));
        let code = cx.new(|cx| OtpState::new(6, window, cx));
        // Redraw as the fields change, so Next enables as soon as there is input.
        let subscriptions = vec![
            cx.subscribe_in(&id, window, |_, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe_in(&password, window, |_, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe_in(&code, window, |app, _, event: &OtpEvent, _, cx| {
                cx.notify();
                if matches!(event, OtpEvent::Complete) {
                    app.sign_in_next(cx);
                }
            }),
        ];
        id.update(cx, |state, cx| state.focus(window, cx));
        self.hub.state().session.set_signing_in(true);
        // The attempt's generation, after the bump sign-in start makes. The
        // cookies it collects are adopted only while this still holds.
        let session = self.hub.state().session.generation();
        self.sign_in = Some(SignIn {
            flow: crate::signin::Flow::new(Instant::now()),
            id,
            password,
            code,
            pictures: (0, Vec::new()),
            then,
            passkey: None,
            opened: false,
            session,
            _subscriptions: subscriptions,
        });
        self.hub.publish();
        self.open(Overlay::SignIn, cx);
    }

    /// The dialog's main button, or Enter in one of its fields.
    fn sign_in_next(&mut self, cx: &mut Context<Self>) {
        use crate::signin::Step;
        let Some(state) = self.sign_in.as_mut() else { return };
        match state.flow.step() {
            Step::Id => {
                let id = state.id.read(cx).value();
                state.flow.submit_id(&id);
            }
            Step::Password => {
                let password = zeroize::Zeroizing::new(state.password.read(cx).value().to_string());
                state.flow.submit_password(&password);
            }
            Step::Code(_) => {
                let code = state.code.read(cx).value().to_string();
                state.flow.submit_code(&code);
            }
            _ => {}
        }
        cx.notify();
    }

    fn sign_in_failed(&mut self, cx: &mut Context<Self>) {
        let open = self.overlay == Some(Overlay::SignIn);
        self.cancel_sign_in();
        if open {
            self.close_overlay(cx);
        }
        self.hub.notify(Notice::Error("loginUnavailable"));
        cx.notify();
    }

    /// Stop a sign-in that has not finished. The browser closes on the next tick.
    fn cancel_sign_in(&mut self) {
        if self.sign_in.take().is_some() {
            #[cfg(windows)]
            crate::passkey::cancel();
            self.hub.state().session.set_signing_in(false);
            self.hub.publish();
        }
    }

    /// Feed browser events to the sign-in flow and collect what to send back.
    #[cfg(windows)]
    fn pump_sign_in(&mut self, events: Vec<crate::browser::Event>, window: &mut Window, cx: &mut Context<Self>) -> Outgoing {
        let mut out = Outgoing::default();
        let Some(state) = self.sign_in.as_mut() else {
            // The dialog is gone (signed out or cancelled) but the hidden
            // browser may still be closing. Shut it, and with it its profile,
            // so a late signed-in page cannot hand cookies to the session.
            out.close = crate::browser::is_open();
            return out;
        };
        if !state.opened {
            state.opened = true;
            out.open = Some((self.hub.state().directory.join("sign-in"), self.snapshot.preferences.remember));
            return out;
        }
        let now = Instant::now();
        let mut cookies = None;
        for event in events {
            match event {
                crate::browser::Event::Page(url) => state.flow.on_page(crate::signin::page(&url), now),
                crate::browser::Event::Reply { id, status, body } => state.flow.on_reply(id, status, &body, now),
                crate::browser::Event::SignedIn(list) => cookies = Some(list),
            }
        }
        if let Some(result) = state.passkey.as_ref().and_then(|r| r.try_recv().ok()) {
            state.passkey = None;
            state.flow.on_passkey(result);
        }
        state.flow.tick(now);
        if let Some(options) = state.flow.take_ceremony() {
            let owner = hwnd_of(window).map_or(0, |h| h as isize);
            let (sender, receiver) = std::sync::mpsc::channel();
            state.passkey = Some(receiver);
            std::thread::spawn(move || {
                let _ = sender.send(crate::passkey::get(owner, &options));
            });
        }
        out.calls = state.flow.take_calls();
        out.navigate = state.flow.take_navigation();
        // The attempt's generation, needed after the `state` borrow ends.
        let attempt = state.session;
        if state.flow.take_changed() {
            let generation = state.flow.captcha().generation;
            if state.pictures.0 != generation || state.pictures.1.is_empty() {
                state.pictures = (generation, vec![None; crate::signin::PICTURES + 1]);
            }
            for index in 0..=crate::signin::PICTURES {
                let bytes = if index == 0 { state.flow.captcha().sample() } else { state.flow.captcha().picture(index - 1) };
                if state.pictures.1[index].is_none()
                    && let Some(bytes) = bytes
                    && let Some(format) = image_format(bytes)
                {
                    state.pictures.1[index] = Some(Arc::new(Image::from_bytes(format, bytes.to_vec())));
                }
            }
            if state.flow.problem() == Some(crate::signin::Problem::Code) {
                state.code.update(cx, |code, cx| code.set_value("", window, cx));
            }
            cx.notify();
        }
        if let Some(cookies) = cookies {
            self.finish_sign_in(cookies, attempt, cx);
            out.close = true;
        }
        out
    }

    #[cfg(not(windows))]
    fn pump_sign_in(&mut self, _: Vec<()>, _: &mut Window, _: &mut Context<Self>) -> Outgoing {
        Outgoing::default()
    }

    /// The portal reported a signed-in page: hand its cookies to the session.
    fn finish_sign_in(&mut self, cookies: Vec<crate::jar::StoredCookie>, generation: u64, cx: &mut Context<Self>) {
        let then = self.sign_in.take().and_then(|state| state.then);
        if self.overlay == Some(Overlay::SignIn) {
            self.close_overlay(cx);
        }
        let hub = self.hub.clone();
        // A sign-out (or a new attempt) after the attempt started bumps the
        // generation, and `adopt` then refuses to bring this account back.
        cx.spawn(async move |this, cx| {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let adopt = hub.clone();
            hub.spawn(async move {
                let _ = sender.send(adopt.state().session.adopt(&adopt, cookies, generation).await.is_ok());
            });
            if receiver.await.unwrap_or(false)
                && let Some(id) = then
            {
                // Play from the UI so a question the game page asks can open.
                let _ = this.update(cx, |app, cx| {
                    let state = app.hub.state();
                    if state.session.generation() != generation || state.session.status() != AccountStatus::SignedIn {
                        return;
                    }
                    if id == GameId::Gitadora && state.snapshot().preferences.gitadora_mode.is_none() {
                        app.open(Overlay::Instrument(id), cx);
                    } else {
                        app.start(id, cx);
                    }
                });
            }
        })
        .detach();
        cx.notify();
    }

    fn sign_out(&mut self, cx: &mut Context<Self>) {
        // `sign_out` bumps the session generation, so a refresh or adoption
        // still in flight cannot put the account back.
        self.hub.state().session.sign_out();
        self.stat_reads.clear();
        self.cancel_sign_in();
        // The event loop closes the hidden WebView outside the UI update.
        self.close_overlay(cx);
        self.hub.publish();
        cx.notify();
    }

    fn set_preferences(&mut self, change: impl FnOnce(&mut crate::state::Preferences), cx: &mut Context<Self>) {
        let mut preferences = self.snapshot.preferences.clone();
        let remember = preferences.remember;
        change(&mut preferences);
        let new_remember = preferences.remember;
        match self.hub.state().commit_preferences(preferences) {
            Ok(()) if remember != new_remember => self.hub.state().session.set_remember(new_remember),
            Ok(()) => {}
            Err(error) => self.hub.notify(Notice::Error(error.code)),
        }
        self.snapshot = self.hub.state().snapshot();
        cx.notify();
    }

    fn pick_folder(&mut self, then: impl FnOnce(&mut Self, PathBuf, &mut Context<Self>) + 'static, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: None });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update(cx, |app, cx| {
                    then(app, path, cx);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn locate(&mut self, id: GameId, cx: &mut Context<Self>) {
        self.pick_folder(
            move |app, path, cx| match app.hub.state().import(id, path) {
                Ok(()) => {
                    app.close_overlay(cx);
                    app.snapshot = app.hub.state().snapshot();
                    app.select(id, cx);
                    let hub = app.hub.clone();
                    app.hub.run(async move { crate::launch::check(hub, id).await.map(|_| ()) });
                }
                Err(error) => app.hub.notify(Notice::Error(error.code)),
            },
            cx,
        );
    }

    /// Adopt a registry-detected root the user confirmed. Same contract as a
    /// folder pick: the module marker must exist.
    fn adopt(&mut self, id: GameId, root: PathBuf, cx: &mut Context<Self>) {
        match self.hub.state().import(id, root) {
            Ok(()) => {
                self.close_overlay(cx);
                self.snapshot = self.hub.state().snapshot();
                self.select(id, cx);
                if id.title().is_some() {
                    let hub = self.hub.clone();
                    self.hub.run(async move { crate::launch::check(hub, id).await.map(|_| ()) });
                }
            }
            Err(error) => self.hub.notify(Notice::Error(error.code)),
        }
    }

    fn open_game_settings(&mut self, id: GameId, _cx: &mut Context<Self>) {
        self.hub.run(crate::game_settings::open(self.hub.clone(), id));
    }

    fn open_install(&mut self, id: GameId, cx: &mut Context<Self>) {
        let base = self.snapshot.preferences.default_folder.clone().unwrap_or_else(crate::platform::default_games_folder);
        let mut name = crate::catalog::descriptor(id).title.replace(['\'', ':'], "");
        if self.game(id).is_some_and(|g| g.update_pending || g.install_dir.is_some()) {
            name.push_str(" (new)");
        }
        self.open(Overlay::Install(id), cx);
        self.set_install_path(base.join(name), cx);
        cx.notify();
    }

    fn set_install_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.install_path = path.clone();
        self.install_free = None;
        let runtime = self.hub.clone();
        let (sender, receiver) = futures_channel();
        let probe = path.clone();
        runtime.spawn(async move {
            let space = tokio::task::spawn_blocking(move || crate::update::free_space(&probe)).await.ok().flatten();
            let _ = sender.send(space);
        });
        cx.spawn(async move |this, cx| {
            if let Ok(space) = receiver.await {
                let _ = this.update(cx, |app, cx| {
                    if app.install_path == path {
                        app.install_free = space;
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    fn start_install(&mut self, id: GameId, cx: &mut Context<Self>) {
        if self.install_submitting || self.install_path.as_os_str().is_empty() {
            return;
        }
        self.install_submitting = true;
        let path = self.install_path.clone();
        let hub = self.hub.clone();
        let worker = hub.clone();
        let (sender, receiver) = futures_channel();
        hub.spawn(async move {
            let _ = sender.send(crate::update::install(worker, id, path).await);
        });
        cx.spawn(async move |this, cx| {
            let result = receiver.await.unwrap_or(Err(crate::state::HubError { code: "updateFailed" }));
            let _ = this.update(cx, |app, cx| {
                app.install_submitting = false;
                match result {
                    Ok(snapshot) => {
                        app.snapshot = snapshot;
                        if app.overlay == Some(Overlay::Install(id)) {
                            app.close_overlay(cx);
                        }
                        app.select(id, cx);
                    }
                    Err(error) => app.hub.notify(Notice::Error(error.code)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn remove(&mut self, id: GameId, cx: &mut Context<Self>) {
        if let Err(error) = self.hub.state().forget(id) {
            self.hub.notify(Notice::Error(error.code));
        }
        self.close_overlay(cx);
        self.snapshot = self.hub.state().snapshot();
        self.selected = first_installed(&self.snapshot);
        cx.notify();
    }

    // ---------- rendering ----------

    fn game(&self, id: GameId) -> Option<&GameSnapshot> {
        self.snapshot.games.iter().find(|g| g.id == id)
    }
}

fn futures_channel<T>() -> (tokio::sync::oneshot::Sender<T>, tokio::sync::oneshot::Receiver<T>) {
    tokio::sync::oneshot::channel()
}

fn image_format(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(ImageFormat::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        Some(ImageFormat::Jpeg)
    } else if bytes.starts_with(b"GIF8") {
        Some(ImageFormat::Gif)
    } else if bytes.len() > 12 && &bytes[8..12] == b"WEBP" {
        Some(ImageFormat::Webp)
    } else {
        None
    }
}

fn installed(snapshot: &Snapshot, id: GameId) -> bool {
    snapshot.games.iter().any(|g| g.id == id && (g.install_dir.is_some() || g.update_pending))
}

fn first_installed(snapshot: &Snapshot) -> Option<GameId> {
    snapshot.games.iter().find(|g| g.install_dir.is_some() || g.update_pending).map(|g| g.id)
}

fn colors(id: GameId) -> (u32, u32) {
    match id {
        GameId::BomberGirl => (0xff6a88, 0xff9a3c),
        GameId::Sdvx => (0x7b2ff7, 0xf107a3),
        GameId::Ddr => (0x3a1c71, 0xd76d77),
        GameId::Popn => (0xffb300, 0xff4f9a),
        GameId::Gitadora => (0xc31432, 0xf7971e),
        GameId::Nostalgia => (0x0f2027, 0x2c7a7b),
        GameId::Infinitas => (0x141e30, 0x3a6186),
    }
}

fn logo(id: GameId) -> (&'static [&'static str], &'static str) {
    match id {
        GameId::BomberGirl => (&["BOMBER", "GIRL"], ""),
        GameId::Sdvx => (&["SOUND", "VOLTEX"], "EXCEED GEAR"),
        GameId::Ddr => (&["DANCE DANCE", "REVOLUTION"], "GRAND PRIX"),
        GameId::Popn => (&["pop’n", "music"], "Lively"),
        GameId::Gitadora => (&["GITADORA"], ""),
        GameId::Nostalgia => (&["NOSTALGIA"], ""),
        GameId::Infinitas => (&["beatmania", "IIDX"], "INFINITAS"),
    }
}

fn mono(id: GameId) -> &'static str {
    match id {
        GameId::BomberGirl => "BG",
        GameId::Sdvx => "SV",
        GameId::Ddr => "DD",
        GameId::Popn => "PM",
        GameId::Gitadora => "GD",
        GameId::Nostalgia => "NO",
        GameId::Infinitas => "II",
    }
}

fn game_icon(id: GameId, size: f32) -> Div {
    let (a, b) = colors(id);
    div()
        .size(px(size))
        .flex_none()
        .rounded(px(12.))
        .bg(linear_gradient(135., linear_color_stop(rgb(a), 0.), linear_color_stop(rgb(b), 1.)))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(size * 0.36))
        .font_weight(FontWeight::EXTRA_BOLD)
        .text_color(white())
        .child(mono(id))
}

fn logo_block(id: GameId, scale: f32) -> Div {
    let (lines, sub) = logo(id);
    let big = if id == GameId::Ddr { 60. } else { 80. } * scale;
    let mut block = div().flex().flex_col().line_height(relative(0.95));
    for line in lines {
        block = block.child(div().text_size(px(big)).font_weight(FontWeight::BLACK).text_color(white()).child(*line));
    }
    if !sub.is_empty() {
        block = block.child(
            div()
                .mt(px(12. * scale))
                .text_size(px(18. * scale))
                .font_weight(FontWeight::BOLD)
                .text_color(rgba(0xffffffe6))
                .child(spaced(sub)),
        );
    }
    block
}

fn spaced(text: &str) -> String {
    text.chars().map(|c| c.to_string()).collect::<Vec<_>>().join(" ")
}

fn primary(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: Option<&'static str>, enabled: bool) -> Stateful<Div> {
    let mut button = div()
        .id(id)
        .h(px(56.))
        .min_w(px(220.))
        .px(px(30.))
        .rounded(px(10.))
        .bg(theme::accent())
        .shadow(theme::shadow(8., 24., 0.35))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .text_size(px(18.))
        .font_weight(FontWeight::BOLD)
        .text_color(white());
    if enabled {
        button = button.cursor_pointer().hover(|s| s.bg(theme::accent_hover())).active(|s| s.bg(theme::accent_active()));
    }
    if let Some(icon) = icon {
        button = button.child(theme::icon(icon, 18., white()));
    }
    button.child(label.into())
}

fn soft_primary(id: impl Into<ElementId>, label: impl Into<SharedString>, spinner: bool) -> Stateful<Div> {
    let mut button = div()
        .id(id)
        .h(px(56.))
        .min_w(px(220.))
        .px(px(30.))
        .rounded(px(10.))
        .bg(theme::soft())
        .flex()
        .items_center()
        .justify_center()
        .gap(px(10.))
        .text_size(px(18.))
        .font_weight(FontWeight::BOLD)
        .text_color(theme::text());
    if spinner {
        button = button.child(spinner_icon(18.));
    }
    button.child(label.into())
}

fn spinner_icon(size: f32) -> impl IntoElement {
    theme::icon(icons::LOADER, size, theme::text()).with_animation(
        "spin",
        Animation::new(Duration::from_millis(900)).repeat(),
        |svg, delta| svg.with_transformation(Transformation::rotate(percentage(delta))),
    )
}

fn compact(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: Option<&'static str>) -> Stateful<Div> {
    let mut button = div()
        .id(id)
        .h(px(36.))
        .px(px(16.))
        .rounded(px(10.))
        .bg(theme::accent())
        .flex()
        .items_center()
        .gap(px(7.))
        .text_size(px(14.))
        .font_weight(FontWeight::BOLD)
        .text_color(white())
        .cursor_pointer()
        .hover(|s| s.bg(theme::accent_hover()))
        .active(|s| s.bg(theme::accent_active()));
    if let Some(icon) = icon {
        button = button.child(theme::icon(icon, 15., white()));
    }
    button.child(label.into())
}

fn ghost(id: impl Into<ElementId>, label: impl Into<SharedString>, icon: Option<&'static str>) -> Stateful<Div> {
    let mut button = div()
        .id(id)
        .h(px(32.))
        .px(px(12.))
        .rounded(px(7.))
        .bg(theme::ghost())
        .border_1()
        .border_color(theme::line())
        .flex()
        .items_center()
        .gap(px(7.))
        .text_size(px(13.))
        .text_color(theme::text())
        .cursor_pointer()
        .hover(|s| s.bg(theme::ghost_hover()))
        .active(|s| s.bg(rgba(0xffffff2e)));
    if let Some(icon) = icon {
        button = button.child(theme::icon(icon, 15., theme::text()));
    }
    button.child(label.into())
}

fn icon_button(id: impl Into<ElementId>, icon: &'static str, size: f32) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(30.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .flex_none()
        .cursor_pointer()
        .text_color(theme::muted())
        .hover(|s| s.bg(rgba(0xffffff14)).text_color(theme::text()))
        .child(svg().path(icon).size(px(size)).text_color(theme::muted()))
}

fn bar(key: impl Into<SharedString>, ratio: f32, color: Hsla, active: bool) -> Div {
    let key = key.into();
    let ratio = ratio.clamp(0., 1.);
    let fill = motion::spring(
        div().h_full().rounded(px(3.)).bg(color).overflow_hidden().relative().when(active, |d| {
            d.child(
                div()
                    .absolute()
                    .top_0()
                    .h_full()
                    .w(px(80.))
                    .bg(linear_gradient(90., linear_color_stop(rgba(0xffffff00), 0.), linear_color_stop(rgba(0xffffff40), 0.5)))
                    .with_animation(
                        SharedString::from(format!("{key}-shine")),
                        Animation::new(Duration::from_millis(1600)).repeat().with_easing(ease_in_out),
                        |el, t| el.left(relative(-0.3 + 1.6 * t)),
                    ),
            )
        }),
        SharedString::from(format!("{key}-fill")),
        SETTLE,
        0.,
        ratio,
        |el, t| el.w(relative(t.clamp(0., 1.))),
    );
    div().h(px(6.)).w_full().rounded(px(3.)).bg(rgba(0xffffff1f)).child(fill)
}

impl Render for App {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected;
        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|app, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    if app.select.is_some() {
                        app.select = None;
                    } else if app.menu {
                        app.menu = false;
                    } else {
                        app.close_overlay(cx);
                    }
                    cx.notify();
                }
            }))
            .size_full()
            .flex()
            .bg(theme::bg())
            .font_family(theme::FONT)
            .text_color(theme::text())
            .text_size(px(14.))
            .child(self.render_rail(cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .children(self.previous.filter(|p| Some(*p) != selected).map(|id| self.backdrop(id).into_any_element()))
                    .child(match selected {
                        // Keyed by image so the loaded background fades in.
                        Some(id) => self
                            .backdrop(id)
                            .with_animation(
                                SharedString::from(format!(
                                    "art-{}-{}",
                                    id_str(id),
                                    self.backdrop_image(id).map_or(0, |image| image.id.0 + 1)
                                )),
                                motion::ease(520),
                                |el, t| el.opacity(t),
                            )
                            .into_any_element(),
                        None => div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .bg(linear_gradient(160., linear_color_stop(rgb(0x2a1b22), 0.), linear_color_stop(rgb(0x0e0f12), 0.7)))
                            .into_any_element(),
                    })
                    .when(!selected.is_some_and(|id| self.backgrounds.contains_key(&id)), |stage| {
                        stage
                            .child(div().absolute().top_0().left_0().size_full().bg(linear_gradient(
                                180.,
                                linear_color_stop(rgba(0x00000000), 0.45),
                                linear_color_stop(rgba(0x000000c7), 1.),
                            )))
                            .child(div().absolute().top_0().left_0().w_full().h(px(120.)).bg(linear_gradient(
                                180.,
                                linear_color_stop(rgba(0x00000059), 0.),
                                linear_color_stop(rgba(0x00000000), 1.),
                            )))
                    })
                    .child(self.render_titlebar(window, cx))
                    .children(self.render_maintenance())
                    .child(match selected.and_then(|id| self.game(id).cloned()) {
                        Some(game) => self.render_stage(game, window, cx).into_any_element(),
                        None => self.render_empty(cx).into_any_element(),
                    })
                    .child(self.render_toasts(cx)),
            )
            .children(self.render_overlay(window, cx))
    }
}

impl App {
    fn render_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ids: Vec<GameId> = self.snapshot.games.iter().filter(|g| g.install_dir.is_some() || g.update_pending).map(|g| g.id).collect();
        let active = self.snapshot.games.iter().find(|g| {
            matches!(
                g.phase,
                GamePhase::Downloading
                    | GamePhase::Preparing
                    | GamePhase::Verifying
                    | GamePhase::Installing
                    | GamePhase::Paused
                    | GamePhase::Finalizing
                    | GamePhase::Recovering
            ) && g.update_pending
        });
        // A queued-only rail still shows the downloads glyph with its count.
        let queued = self.snapshot.games.iter().filter(|g| g.phase == GamePhase::Queued).count();
        let slot = ids.iter().position(|id| Some(*id) == self.selected);
        let mut games = div().relative().w_full().flex().flex_col().gap(px(10.)).items_center().flex_1().pt(px(4.));
        if let Some(slot) = slot {
            let y = 4. + slot as f32 * 54. + 10.;
            games = games.child(motion::spring(
                div().absolute().left(px(0.)).w(px(4.)).h(px(24.)).rounded_r(px(3.)).bg(white()),
                "rail-indicator",
                MOVE,
                y,
                y,
                |el, y| el.top(px(y)),
            ));
        }
        for id in ids {
            let game = self.game(id).cloned();
            let on = self.selected == Some(id);
            let key = format!("rail-{}", id_str(id));
            let hover = self.is_hovered(&key);
            let lit = if on {
                1.
            } else if hover {
                0.85
            } else {
                0.55
            };
            let mut tile = div()
                .id(SharedString::from(key.clone()))
                .relative()
                .cursor_pointer()
                .rounded(px(12.))
                .on_hover(self.hover_listener(key.clone(), cx))
                .on_click(cx.listener(move |app, _, _, cx| app.select(id, cx)))
                .tooltip(move |_, cx| tooltip(crate::catalog::descriptor(id).title, cx))
                .child(motion::spring(
                    div().relative().child(motion::spring(
                        {
                            let icon = self.game_icon(id, 44.);
                            if on { icon.border_2().border_color(white()) } else { icon }
                        },
                        SharedString::from(format!("{key}-lit")),
                        SETTLE,
                        lit,
                        lit,
                        |el, t| el.opacity(t),
                    )),
                    SharedString::from(format!("{key}-lift")),
                    MOVE,
                    0.,
                    if hover && !on { 1. } else { 0. },
                    |el, t| el.top(px(-3. * t)),
                ));
            if let Some(game) = game {
                let busy = game.update_pending && !matches!(game.phase, GamePhase::Update);
                if game.phase == GamePhase::Running {
                    tile = tile.child(badge(theme::ok(), Some(icons::PLAY)));
                } else if game.phase == GamePhase::Queued {
                    tile = tile.child(
                        badge(rgba(0x000000a6).into(), None).child(
                            div()
                                .text_size(px(10.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme::text())
                                .child(game.queue_position.unwrap_or(1).to_string()),
                        ),
                    );
                } else if busy {
                    tile = tile.child(badge(rgb(0x2f7cf6).into(), Some(icons::DOWNLOAD)));
                } else if matches!(game.phase, GamePhase::Update) {
                    tile = tile.child(
                        div()
                            .absolute()
                            .right(px(-3.))
                            .top(px(-3.))
                            .size(px(14.))
                            .rounded_full()
                            .bg(theme::accent())
                            .border_2()
                            .border_color(theme::rail()),
                    );
                }
            }
            games = games.child(tile);
        }
        games = games.child(
            div()
                .id("rail-add")
                .size(px(44.))
                .rounded(px(12.))
                .border_1()
                .border_dashed()
                .border_color(rgba(0xffffff38))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.border_color(rgba(0xffffff80)))
                .tooltip({
                    let label = self.tr(K::add_game);
                    move |_, cx| tooltip(label, cx)
                })
                .on_click(cx.listener(|app, _, _, cx| {
                    app.open(Overlay::Add, cx);
                    cx.notify();
                }))
                .child(theme::icon(icons::PLUS, 20., theme::muted())),
        );
        let downloads_icon = match active {
            Some(game) => {
                let ratio = progress_ratio(game);
                div()
                    .size(px(26.))
                    .relative()
                    .child(div().absolute().top_0().left_0().size(px(26.)).rounded_full().border_2().border_color(rgba(0xffffff26)))
                    .child(
                        div()
                            .absolute()
                            .bottom(px(2.))
                            .left(px(4.))
                            .h(px(3.))
                            .w(px(18. * ratio))
                            .rounded(px(2.))
                            .bg(if game.phase == GamePhase::Paused { theme::muted() } else { theme::blue() }),
                    )
                    .child(div().absolute().top(px(5.)).left(px(5.)).child(theme::icon(icons::DOWNLOAD, 16., theme::text())))
                    .into_any_element()
            }
            None if queued > 0 => div()
                .size(px(26.))
                .relative()
                .child(div().absolute().top_0().left_0().size(px(26.)).rounded_full().border_2().border_color(rgba(0xffffff26)))
                .child(div().absolute().top(px(5.)).left(px(5.)).child(theme::icon(icons::DOWNLOAD, 16., theme::muted())))
                .child(
                    div()
                        .absolute()
                        .right(px(-4.))
                        .top(px(-4.))
                        .h(px(14.))
                        .min_w(px(14.))
                        .px(px(3.))
                        .rounded_full()
                        .bg(rgb(0x2f7cf6))
                        .border_2()
                        .border_color(theme::rail())
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(9.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(white())
                        .child(queued.to_string()),
                )
                .into_any_element(),
            None => theme::icon(icons::DOWNLOAD, 20., theme::muted()).into_any_element(),
        };
        div()
            .w(px(72.))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .bg(theme::rail())
            .border_r_1()
            .border_color(theme::line())
            .py(px(12.))
            .child(div().size(px(36.)).mb(px(18.)).child(img("icon.svg").size_full()))
            .child(games)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .pt(px(10.))
                    .border_t_1()
                    .border_color(theme::line())
                    .w(px(44.))
                    .items_center()
                    .child(
                        rail_tool("rail-downloads", downloads_icon, self.overlay == Some(Overlay::Downloads), self.tr(K::downloads))
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.open(Overlay::Downloads, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        rail_tool(
                            "rail-settings",
                            theme::icon(icons::SETTINGS, 20., theme::muted()).into_any_element(),
                            matches!(self.overlay, Some(Overlay::Settings(_))),
                            self.tr(K::settings),
                        )
                        .on_click(cx.listener(|app, _, _, cx| {
                            app.open(Overlay::Settings(app.settings_tab), cx);
                            cx.notify();
                        })),
                    ),
            )
    }

    fn render_titlebar(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let chip = match self.snapshot.account {
            AccountStatus::SignedIn => {
                let profile = self.snapshot.profile.clone().unwrap_or_default();
                let name = profile.data.name.clone().unwrap_or_else(|| self.tr(K::account).to_owned());
                let mut chip = account_chip("account-chip")
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.open(Overlay::Drawer, cx);
                        app.reveal_card = false;
                        if let Some(id) = app.selected {
                            app.hub.state().session.playdata.invalidate(id);
                        }
                        let hub = app.hub.clone();
                        app.hub.spawn(async move {
                            let _ = hub.state().session.refresh(&hub).await;
                        });
                        cx.notify();
                    }))
                    .child(avatar(&profile, &name, 26.))
                    .child(div().font_weight(FontWeight::SEMIBOLD).max_w(px(160.)).truncate().child(name));
                if let Some(balance) = profile.data.paseli_balance {
                    chip = chip.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .pl(px(8.))
                            .border_l_1()
                            .border_color(rgba(0xffffff24))
                            .text_color(theme::gold())
                            .child(
                                self.service_mark(Service::Paseli, 16.)
                                    .unwrap_or_else(|| theme::icon(icons::WALLET, 13., theme::gold()).into_any_element()),
                            )
                            .child(group_digits(balance)),
                    );
                }
                chip.into_any_element()
            }
            AccountStatus::SigningIn => account_chip("account-chip")
                .on_click(cx.listener(|app, _, _, cx| {
                    if app.sign_in.is_some() {
                        app.open(Overlay::SignIn, cx);
                    }
                }))
                .child(
                    div().size(px(26.)).rounded_full().bg(rgba(0xffffff1f)).flex().items_center().justify_center().child(spinner_icon(14.)),
                )
                .child(div().font_weight(FontWeight::SEMIBOLD).child(self.tr(K::signing_in)))
                .into_any_element(),
            // Restoring a saved session at startup. Nothing to click yet.
            AccountStatus::Unknown => div()
                .id("account-chip")
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(34.))
                .pl(px(4.))
                .pr(px(12.))
                .rounded(px(17.))
                .bg(rgba(0x00000073))
                .border_1()
                .border_color(rgba(0xffffff1a))
                .text_color(theme::muted())
                .child(
                    div().size(px(26.)).rounded_full().bg(rgba(0xffffff1f)).flex().items_center().justify_center().child(spinner_icon(14.)),
                )
                .child(div().font_weight(FontWeight::SEMIBOLD).child(format!("{}…", self.tr(K::signing_in))))
                .into_any_element(),
            _ => account_chip("account-chip")
                .on_click(cx.listener(|app, _, window, cx| app.sign_in(None, window, cx)))
                .child(div().size(px(26.)).rounded_full().bg(rgba(0xffffff1f)).flex().items_center().justify_center().child(theme::icon(
                    icons::USER,
                    15.,
                    theme::text(),
                )))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(self.tr(K::sign_in)))
                .into_any_element(),
        };
        div()
            .relative()
            .flex()
            .items_center()
            .gap(px(10.))
            .h(px(48.))
            .pl(px(16.))
            .child(div().flex_1().h_full().window_control_area(WindowControlArea::Drag))
            .children(self.hub_update.as_ref().map(|release| {
                let label = if self.hub_updating {
                    format!("{}…", self.tr(K::hub_updating))
                } else {
                    fill(self.tr(K::hub_update), &[&release.version.to_string()])
                };
                let button = account_chip("hub-update")
                    .pl(px(10.))
                    .child(if self.hub_updating {
                        spinner_icon(14.).into_any_element()
                    } else {
                        theme::icon(icons::DOWNLOAD, 14., theme::accent_text()).into_any_element()
                    })
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(label))
                    .when(!self.hub_updating, |button| button.on_click(cx.listener(|app, _, _, cx| app.apply_hub_update(cx))));
                motion::fade(button, "hub-update-in", 220)
            }))
            .child(chip)
            .child(
                div()
                    .flex()
                    .h_full()
                    .ml(px(6.))
                    .child(caption("min", icons::MINUS, 15., WindowControlArea::Min, false))
                    .child(caption("max", icons::SQUARE, 12., WindowControlArea::Max, false))
                    .child(caption("close", icons::X, 16., WindowControlArea::Close, true)),
            )
    }

    fn render_maintenance(&self) -> Option<impl IntoElement + use<>> {
        let m = &self.snapshot.maintenance;
        if m.kind == crate::maintenance::Kind::None {
            return None;
        }
        let text = match m.ends_at {
            Some(ms) => fill(self.tr(K::maintenance_until), &[&jst_time(ms)]),
            None => self.tr(K::maintenance_service).to_owned(),
        };
        Some(
            div()
                .relative()
                .mx(px(24.))
                .px(px(14.))
                .py(px(9.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .gap(px(10.))
                .bg(rgba(0x3c2800d1))
                .border_1()
                .border_color(rgba(0xffb02066))
                .text_color(rgb(0xffd98a))
                .child(theme::icon(icons::ALERT, 16., rgb(0xffd98a).into()))
                .child(text),
        )
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div().relative().flex_1().flex().items_center().justify_center().child(
            primary("empty-add", self.tr(K::add_game), Some(icons::PLUS), true).on_click(cx.listener(|app, _, _, cx| {
                app.open(Overlay::Add, cx);
                cx.notify();
            })),
        )
    }

    fn render_stage(&self, game: GameSnapshot, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = window.viewport_size().width.as_f32();
        let scale = ((width - 72.) / 1208.).clamp(0.6, 1.0);
        div()
            .relative()
            .flex_1()
            .flex()
            .flex_col()
            .px(px(32.))
            .pb(px(28.))
            .min_h_0()
            .child(div().flex_1().flex().items_center().min_h_0().when(!self.backgrounds.contains_key(&game.id), |row| {
                row.child(motion::slide(
                    logo_block(game.id, scale).relative(),
                    SharedString::from(format!("logo-{}", id_str(game.id))),
                    560,
                    -28.,
                ))
            }))
            .child(
                div()
                    .flex()
                    .items_end()
                    .justify_between()
                    .gap(px(24.))
                    .child(motion::rise_after(
                        div().relative().child(self.render_info(&game, cx)),
                        SharedString::from(format!("info-{}", id_str(game.id))),
                        80,
                        420,
                        16.,
                    ))
                    .child(motion::rise_after(
                        div().relative().child(self.render_action(&game, cx)),
                        SharedString::from(format!("action-{}", id_str(game.id))),
                        140,
                        420,
                        16.,
                    )),
            )
    }

    fn render_info(&self, game: &GameSnapshot, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self.tab;
        let tabs = div()
            .flex()
            .gap(px(18.))
            .px(px(16.))
            .pt(px(10.))
            .border_b_1()
            .border_color(theme::line())
            .relative()
            .child(info_tab("tab-news", self.tr(K::news), tab == Tab::News).on_click(cx.listener(|app, _, _, cx| {
                app.tab = Tab::News;
                cx.notify();
            })))
            .child(info_tab("tab-details", self.tr(K::details), tab == Tab::Details).on_click(cx.listener(|app, _, _, cx| {
                app.tab = Tab::Details;
                cx.notify();
            })));
        let mut panel =
            div().w(px(400.)).flex_none().rounded(px(10.)).bg(theme::panel()).border_1().border_color(theme::line()).overflow_hidden();
        if tab == Tab::News {
            panel = panel.children(self.render_banner(game.id, cx));
        }
        panel = panel.child(tabs);
        panel = match tab {
            Tab::News => panel.child(self.render_news(game.id, cx)),
            Tab::Details => panel.child(self.render_details(game, cx)),
        };
        panel.children(self.render_playdata(game.id))
    }

    /// Compact headline numbers under the panel body. Nothing when signed out,
    /// unsupported, or the account has no data for this title.
    fn render_playdata(&self, id: GameId) -> Option<impl IntoElement + use<>> {
        if self.snapshot.account != AccountStatus::SignedIn || !playdata::supported(id) {
            return None;
        }
        let data = self.hub.state().session.playdata.data(id)?;
        Some(playdata_section(id, &data, self.locale()))
    }

    fn render_banner(&self, id: GameId, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let news = self.news.get(&id)?.as_ref()?;
        let loaded: Vec<_> =
            news.banners.iter().filter_map(|b| self.images.get(&b.image).cloned().flatten().map(|img| (img, b.link.clone()))).collect();
        if loaded.is_empty() {
            return None;
        }
        let index = self.banner % loaded.len();
        let (image, link) = loaded[index].clone();
        let count = loaded.len();
        let behind = self.banner_previous.map(|p| p % count).filter(|p| *p != index).map(|p| loaded[p].0.clone());
        let mut pager = div().absolute().right(px(14.)).bottom(px(12.)).flex().gap(px(5.));
        for i in 0..count {
            pager = pager.child(
                div()
                    .id(("pager", i))
                    .h(px(3.))
                    .w(px(if i == index { 22. } else { 14. }))
                    .rounded(px(2.))
                    .bg(if i == index { white() } else { Hsla::from(rgba(0xffffff66)) })
                    .cursor_pointer()
                    .on_click(cx.listener(move |app, _, _, cx| {
                        if app.banner % count != i {
                            app.banner_previous = Some(app.banner);
                            app.banner = i;
                        }
                        cx.notify();
                    })),
            );
        }
        Some(
            div()
                .id("banner")
                .relative()
                .overflow_hidden()
                .h(px(150.))
                .w_full()
                .bg(rgb(0x000000))
                .cursor_pointer()
                .on_click(move |_, _, _| {
                    if let Some(link) = &link {
                        let _ = crate::platform::open_url(link);
                    }
                })
                .children(
                    behind.map(|image| {
                        div().absolute().top_0().left_0().size_full().child(img(image).size_full().object_fit(ObjectFit::Cover))
                    }),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .child(img(image).size_full().object_fit(ObjectFit::Cover))
                        .with_animation(("banner-slide", self.banner), motion::ease(600), |el, t| el.opacity(t)),
                )
                .when(count > 1, |d| d.child(pager)),
        )
    }

    fn render_news(&self, id: GameId, _cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div().flex().flex_col().px(px(8.)).pt(px(6.)).pb(px(8.));
        match self.news.get(&id) {
            Some(Some(news)) if !news.items.is_empty() => {
                for (i, item) in news.items.iter().take(4).enumerate() {
                    let link = item.link.clone();
                    let date = short_date(&item.date);
                    list = list.child(
                        div()
                            .id(("news", i))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .px(px(8.))
                            .py(px(7.))
                            .rounded(px(6.))
                            .when(link.is_some(), |d| d.cursor_pointer().hover(|s| s.bg(rgba(0xffffff0f))))
                            .on_click(move |_, _, _| {
                                if let Some(link) = &link {
                                    let _ = crate::platform::open_url(link);
                                }
                            })
                            .child(div().flex_1().min_w_0().truncate().child(item.title.clone()))
                            .child(div().text_size(px(12.)).text_color(theme::muted()).child(date)),
                    );
                }
            }
            Some(Some(_)) => list = list.child(div().px(px(8.)).py(px(10.)).text_color(theme::muted()).child(self.tr(K::no_news))),
            _ => {
                for i in 0..3 {
                    list = list.child(motion::pulse(
                        div().mx(px(8.)).my(px(9.)).h(px(12.)).w(relative(0.9 - i as f32 * 0.15)).rounded(px(4.)).bg(rgba(0xffffff1a)),
                        ("news-skeleton", i as usize),
                        0.35,
                    ));
                }
            }
        }
        if let Some(site) = news::official_site(id) {
            list = list.child(
                div()
                    .id("news-more")
                    .mx(px(8.))
                    .mt(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .cursor_pointer()
                    .hover(|s| s.text_color(theme::text()))
                    .on_click(move |_, _, _| {
                        let _ = crate::platform::open_url(site);
                    })
                    .child(self.tr(K::more))
                    .child(theme::icon(icons::EXTERNAL, 12., theme::muted())),
            );
        }
        list
    }

    fn render_details(&self, game: &GameSnapshot, cx: &mut Context<Self>) -> impl IntoElement {
        let idle = !game.update_pending && matches!(game.phase, GamePhase::Ready | GamePhase::Update | GamePhase::Blocked);
        let id = game.id;
        let row = |label: &'static str, value: String| {
            div()
                .flex()
                .gap(px(12.))
                .child(div().w(px(80.)).flex_none().text_color(theme::muted()).child(label))
                .child(div().flex_1().min_w_0().truncate().child(value))
        };
        let path = game.install_dir.as_ref().map(|p| p.display().to_string().trim_start_matches(r"\\?\").to_owned()).unwrap_or_default();
        let mut actions = div().flex().flex_wrap().gap(px(6.));
        actions =
            actions.child(ghost("d-folder", self.tr(K::open_folder), Some(icons::FOLDER)).on_click(cx.listener(move |app, _, _, _| {
                if let Some(dir) = app.game(id).and_then(|g| g.install_dir.clone()) {
                    let _ = crate::platform::open_folder(&dir);
                }
            })));
        if idle {
            if game.supported {
                actions =
                    actions.child(ghost("d-verify", self.tr(K::verify), Some(icons::SHIELD)).on_click(cx.listener(move |app, _, _, _| {
                        let hub = app.hub.clone();
                        app.hub.run(async move { crate::update::repair(hub, id).await.map(|_| ()) });
                    })));
            }
            if id.has_settings() {
                actions = actions.child(ghost("d-settings", self.tr(K::game_settings), Some(icons::SLIDERS)).on_click(cx.listener(
                    move |app, _, _, cx| {
                        app.open_game_settings(id, cx);
                    },
                )));
            }
            actions = actions.child(ghost("d-remove", self.tr(K::remove), Some(icons::TRASH)).text_color(theme::danger_text()).on_click(
                cx.listener(move |app, _, _, cx| {
                    app.open(Overlay::Remove(id), cx);
                    cx.notify();
                }),
            ));
        }
        let mut panel = div()
            .px(px(16.))
            .pt(px(12.))
            .pb(px(14.))
            .flex()
            .flex_col()
            .gap(px(7.))
            .child(row(self.tr(K::version), game.installed_version.clone().unwrap_or_else(|| "—".into())))
            .children(game.resource_revision.map(|r| row(self.tr(K::content), r.to_string())))
            .child(row(self.tr(K::location), path));
        if id == GameId::Gitadora {
            panel = panel.child(div().mt(px(5.)).child(self.instrument_toggle(None, cx)));
        }
        panel.child(div().mt(px(5.)).child(actions))
    }

    fn instrument_toggle(&self, play: Option<GameId>, cx: &mut Context<Self>) -> impl IntoElement {
        use konaste_runtime::GitadoraMode;
        let current = self.snapshot.preferences.gitadora_mode;
        let option = |mode: GitadoraMode, label: &'static str, cx: &mut Context<Self>| {
            let on = current == Some(mode);
            div()
                .id(SharedString::from(format!("mode-{label}")))
                .px(px(12.))
                .py(px(5.))
                .rounded(px(6.))
                .text_size(px(13.))
                .cursor_pointer()
                .text_color(if on { theme::text() } else { theme::muted() })
                .when(on, |d| d.bg(rgba(0xffffff24)))
                .on_click(cx.listener(move |app, _, window, cx| {
                    // Saved for the next Play. The update check does not depend on it.
                    app.set_preferences(|p| p.gitadora_mode = Some(mode), cx);
                    if let Some(id) = play {
                        app.close_overlay(cx);
                        app.play(id, window, cx);
                    }
                }))
                .child(label)
        };
        div()
            .flex()
            .p(px(3.))
            .rounded(px(8.))
            .bg(rgba(0x00000080))
            .border_1()
            .border_color(theme::line())
            .child(option(GitadoraMode::GuitarFreaks, self.tr(K::guitar), cx))
            .child(option(GitadoraMode::DrumMania, self.tr(K::drum), cx))
    }

    fn render_action(&self, game: &GameSnapshot, cx: &mut Context<Self>) -> impl IntoElement {
        let id = game.id;
        if game.phase == GamePhase::Queued {
            // Nothing is running for this title yet; offer the queue exit.
            let label = match game.queue_position {
                Some(position) => format!("{}  {}", self.tr(K::queued), position),
                None => self.tr(K::queued).to_owned(),
            };
            return div()
                .w(px(440.))
                .flex()
                .flex_col()
                .items_end()
                .gap(px(8.))
                .child(soft_primary("primary", label, false))
                .child(ghost("queue-leave", self.tr(K::remove_from_queue), Some(icons::X)).on_click(cx.listener(move |app, _, _, _| {
                    let hub = app.hub.clone();
                    app.hub.run(async move { crate::update::dequeue(&hub, id) });
                })))
                .into_any_element();
        }
        let transfer = game.update_pending
            && matches!(
                game.phase,
                GamePhase::Checking
                    | GamePhase::Preparing
                    | GamePhase::Downloading
                    | GamePhase::Paused
                    | GamePhase::Verifying
                    | GamePhase::Installing
                    | GamePhase::Recovering
                    | GamePhase::Finalizing
                    | GamePhase::Blocked
            );
        if transfer {
            return div().w(px(440.)).flex().flex_col().items_end().child(self.progress_card(game, true, cx)).into_any_element();
        }
        let mut sub = SharedString::default();
        let button = match game.phase {
            GamePhase::Checking => soft_primary("primary", self.tr(K::checking), true).into_any_element(),
            GamePhase::Launching => soft_primary("primary", self.tr(K::starting), true).into_any_element(),
            GamePhase::Running => soft_primary("primary", self.tr(K::running), false).into_any_element(),
            GamePhase::OpeningSettings => soft_primary("primary", self.tr(K::opening_settings), true).into_any_element(),
            GamePhase::Configuring => soft_primary("primary", self.tr(K::configuring), false).into_any_element(),
            GamePhase::Update => {
                sub = self.tr(K::update_size).into();
                primary("primary", self.tr(K::update), Some(icons::DOWNLOAD), true)
                    .on_click(cx.listener(move |app, _, _, _| {
                        let hub = app.hub.clone();
                        app.hub.run(async move { crate::update::start_selected(hub, id).await.map(|_| ()) });
                    }))
                    .into_any_element()
            }
            GamePhase::Ready if self.snapshot.maintenance.kind != crate::maintenance::Kind::None => {
                soft_primary("primary", self.tr(K::maintenance), false).into_any_element()
            }
            GamePhase::Ready => {
                sub = game.installed_version.clone().unwrap_or_default().into();
                primary("primary", self.tr(K::play), Some(icons::PLAY), true)
                    .on_click(cx.listener(move |app, _, window, cx| app.play(id, window, cx)))
                    .into_any_element()
            }
            _ => {
                // A failed check: show why and offer Retry.
                sub = game.error.map(|code| i18n::error(self.locale(), code)).unwrap_or_default().into();
                primary("primary", self.tr(K::retry), Some(icons::ROTATE), true)
                    .on_click(cx.listener(move |app, _, _, _| {
                        let hub = app.hub.clone();
                        app.hub.run(async move { crate::launch::check(hub, id).await.map(|_| ()) });
                    }))
                    .into_any_element()
            }
        };
        let menu_open = self.menu;
        let mut menu_wrap = div().relative().child(
            div()
                .id("more")
                .size(px(56.))
                .rounded(px(10.))
                .bg(rgba(0x14151ad1))
                .border_1()
                .border_color(theme::line())
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.bg(rgba(0x282930eb)))
                .on_click(cx.listener(|app, _, _, cx| {
                    if app.toggle_popup("menu") {
                        app.menu = !app.menu;
                    }
                    cx.notify();
                }))
                .child(theme::icon(icons::MORE, 20., theme::text())),
        );
        if menu_open {
            menu_wrap = menu_wrap.child(
                div().absolute().bottom(px(64.)).right_0().child(div().relative().child(self.render_menu(game, cx)).with_animation(
                    "menu-in",
                    motion::ease(200),
                    |el, t| el.opacity(t).top(px(8. * (1. - t))),
                )),
            );
        }
        let mut area = div().flex().flex_col().items_end().gap(px(8.));
        if id == GameId::Gitadora && game.phase == GamePhase::Ready {
            area = area.child(self.instrument_toggle(None, cx));
        }
        let state_key = SharedString::from(format!("primary-{}-{:?}", id_str(id), game.phase));
        let button = div().relative().child(button).with_animation(state_key, motion::ease(260), |el, t| el.opacity(0.4 + 0.6 * t));
        area.child(div().flex().gap(px(10.)).child(menu_wrap).child(button))
            .child(div().min_h(px(16.)).max_w(px(420.)).text_size(px(12.)).text_color(rgba(0xffffffb3)).child(sub))
            .into_any_element()
    }

    fn render_menu(&self, game: &GameSnapshot, cx: &mut Context<Self>) -> impl IntoElement {
        let id = game.id;
        let idle = matches!(game.phase, GamePhase::Ready | GamePhase::Update | GamePhase::Blocked);
        let item = |key: &'static str, label: &'static str, icon: &'static str, enabled: bool, danger: bool| {
            div()
                .id(key)
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(10.))
                .py(px(8.))
                .rounded(px(6.))
                .text_color(if danger { theme::danger_text() } else { theme::text() })
                .opacity(if enabled { 1. } else { 0.45 })
                .when(enabled, |d| d.cursor_pointer().hover(|s| s.bg(rgba(0xffffff14))))
                .child(theme::icon(icon, 15., if danger { theme::danger_text() } else { theme::text() }))
                .child(label)
        };
        let mut menu = div()
            .id("menu")
            .occlude()
            .min_w(px(210.))
            .p(px(6.))
            .rounded(px(10.))
            .bg(theme::panel_solid())
            .border_1()
            .border_color(theme::line())
            .shadow(theme::shadow(12., 30., 0.5))
            .on_mouse_down_out(cx.listener(|app, _, _, cx| {
                if app.menu {
                    app.menu = false;
                    app.dismissed = Some(("menu", Instant::now()));
                    cx.notify();
                }
            }));
        if id.has_settings() {
            menu = menu.child(item("m-settings", self.tr(K::game_settings), icons::SLIDERS, idle, false).on_click(cx.listener(
                move |app, _, _, cx| {
                    if !idle {
                        return;
                    }
                    app.menu = false;
                    app.open_game_settings(id, cx);
                    cx.notify();
                },
            )));
        }
        if game.supported {
            menu =
                menu.child(item("m-verify", self.tr(K::verify), icons::SHIELD, idle, false).on_click(cx.listener(move |app, _, _, cx| {
                    if !idle {
                        return;
                    }
                    app.menu = false;
                    let hub = app.hub.clone();
                    app.hub.run(async move { crate::update::repair(hub, id).await.map(|_| ()) });
                    cx.notify();
                })));
        }
        menu = menu
            .child(item("m-folder", self.tr(K::open_folder), icons::FOLDER, true, false).on_click(cx.listener(move |app, _, _, cx| {
                app.menu = false;
                if let Some(dir) = app.game(id).and_then(|g| g.install_dir.clone()) {
                    let _ = crate::platform::open_folder(&dir);
                }
                cx.notify();
            })))
            .child(item("m-site", self.tr(K::official_site), icons::EXTERNAL, true, false).on_click(cx.listener(move |app, _, _, cx| {
                app.menu = false;
                if let Some(site) = news::official_site(id) {
                    let _ = crate::platform::open_url(site);
                }
                cx.notify();
            })))
            .child(div().my(px(4.)).h(px(1.)).bg(theme::line()))
            .child(item("m-remove", self.tr(K::remove), icons::TRASH, idle, true).on_click(cx.listener(move |app, _, _, cx| {
                if !idle {
                    return;
                }
                app.menu = false;
                app.open(Overlay::Remove(id), cx);
                cx.notify();
            })));
        menu
    }

    fn progress_card(&self, game: &GameSnapshot, framed: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let id = game.id;
        let ratio = progress_ratio(game);
        let failed = game.phase == GamePhase::Blocked;
        let waiting = game.phase == GamePhase::Downloading && game.transfer.as_ref().is_some_and(|t| t.waiting_for_network);
        let status = if failed {
            game.error.map(|c| i18n::error(self.locale(), c)).unwrap_or(self.tr(K::paused)).to_owned()
        } else if waiting {
            self.tr(K::waiting_for_network).to_owned()
        } else {
            self.tr(match game.phase {
                GamePhase::Checking => K::checking,
                GamePhase::Preparing => K::preparing,
                GamePhase::Downloading => K::downloading,
                GamePhase::Paused => K::paused,
                GamePhase::Verifying => K::verifying,
                GamePhase::Installing => K::installing,
                GamePhase::Recovering => K::recovering,
                _ => K::finalizing,
            })
            .to_owned()
        };
        let (left, middle) = match &game.transfer {
            Some(t) if matches!(game.phase, GamePhase::Downloading | GamePhase::Paused | GamePhase::Blocked) && t.bytes_total > 0 => {
                let amount = format!("{} / {}", i18n::bytes(t.bytes_received), i18n::bytes(t.bytes_total));
                let speed = match t.bytes_per_second.filter(|s| *s > 0 && game.phase == GamePhase::Downloading) {
                    Some(rate) => {
                        let remaining = t.bytes_total.saturating_sub(t.bytes_received) / rate;
                        format!("{}/s   {}", i18n::bytes(rate), fill(self.tr(K::left), &[&i18n::duration(remaining)]))
                    }
                    None => String::new(),
                };
                (amount, speed)
            }
            _ => match &game.work {
                Some(w) if w.files_total > 0 => (format!("{} / {}", w.files_completed, w.files_total), String::new()),
                _ => (String::new(), String::new()),
            },
        };
        let bar_color = if failed || waiting || game.phase == GamePhase::Paused {
            Hsla::from(rgb(0x6b6e78))
        } else if matches!(game.phase, GamePhase::Verifying | GamePhase::Preparing | GamePhase::Finalizing) {
            theme::blue()
        } else {
            theme::accent()
        };
        let mut buttons = div().flex().gap(px(4.));
        if game.phase == GamePhase::Downloading {
            buttons = buttons.child(icon_button(("pause", id as usize), icons::PAUSE, 16.).on_click(cx.listener(move |app, _, _, _| {
                let _ = crate::update::pause(&app.hub, id);
            })));
        }
        let can_resume = matches!(game.phase, GamePhase::Paused | GamePhase::Blocked);
        if can_resume && game.error != Some("updateRecoveryRequired") {
            buttons = buttons.child(
                div()
                    .id(("resume", id as usize))
                    .size(px(30.))
                    .rounded(px(6.))
                    .bg(theme::accent())
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::accent_hover()))
                    .on_click(cx.listener(move |app, _, _, _| {
                        let hub = app.hub.clone();
                        app.hub.run(async move { crate::update::start_selected(hub, id).await.map(|_| ()) });
                    }))
                    .child(theme::icon(if failed { icons::ROTATE } else { icons::PLAY }, 15., white())),
            );
        }
        let mut card = div()
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_end()
                    .mb(px(8.))
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .min_w_0()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if failed { theme::danger_text() } else { theme::text() })
                            .when(failed, |d| d.child(theme::icon(icons::ALERT, 14., theme::danger_text())))
                            .when(waiting, |d| d.child(theme::icon(icons::ALERT, 14., theme::warn())))
                            .child(div().truncate().child(status)),
                    )
                    .child(div().text_size(px(20.)).font_weight(FontWeight::BOLD).child(format!("{}%", (ratio * 100.).floor() as u32))),
            )
            .child(bar(
                SharedString::from(format!("bar-{}-{}", id_str(id), if framed { "card" } else { "list" })),
                ratio,
                bar_color,
                !waiting
                    && matches!(game.phase, GamePhase::Downloading | GamePhase::Verifying | GamePhase::Installing | GamePhase::Preparing),
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .mt(px(6.))
                    .min_h(px(30.))
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child(div().child(left))
                    .child(div().flex_1().child(middle))
                    .child(buttons),
            )
            .when(can_resume, |card| {
                card.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .mt(px(6.))
                        .child(
                            ghost(("install-elsewhere", id as usize), self.tr(K::install_elsewhere), Some(icons::FOLDER))
                                .on_click(cx.listener(move |app, _, _, cx| app.open_install(id, cx))),
                        )
                        .when(game.install_dir.is_none(), |actions| {
                            let hint = self.tr(K::discard_install_hint);
                            actions.child(
                                ghost(("discard-install", id as usize), self.tr(K::discard_install), None)
                                    .tooltip(move |_, cx| tooltip(hint, cx))
                                    .on_click(cx.listener(move |app, _, _, _| {
                                        let hub = app.hub.clone();
                                        app.hub.run(async move { crate::update::discard_install(&hub, id) });
                                    })),
                            )
                        }),
                )
            });
        if framed {
            card = card.px(px(16.)).pt(px(14.)).pb(px(10.)).rounded(px(10.)).bg(theme::panel()).border_1().border_color(theme::line());
        }
        card
    }

    fn render_toasts(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut column = div().absolute().top(px(58.)).left_0().w_full().flex().flex_col().items_center().gap(px(8.));
        for toast in &self.toasts {
            let id = toast.id;
            let leaving = toast.leaving;
            let mut row = div()
                .relative()
                .flex()
                .items_center()
                .gap(px(12.))
                .pl(px(14.))
                .pr(px(8.))
                .py(px(8.))
                .rounded(px(8.))
                .bg(theme::panel_solid())
                .border_1()
                .border_color(theme::line())
                .shadow(theme::shadow(10., 28., 0.5))
                .max_w(px(640.))
                .child(div().child(toast.text.clone()));
            if let Some(game) = toast.action {
                row = row.child(
                    div()
                        .id(("toast-action", id as usize))
                        .px(px(8.))
                        .py(px(4.))
                        .text_color(theme::accent_text())
                        .font_weight(FontWeight::SEMIBOLD)
                        .cursor_pointer()
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.dismiss_toast(id, cx);
                            app.select(game, cx);
                        }))
                        .child(self.tr(K::details)),
                );
            }
            row = row.child(icon_button(("toast-close", id as usize), icons::X, 14.).on_click(cx.listener(move |app, _, _, cx| {
                app.dismiss_toast(id, cx);
            })));
            column = column.child(if leaving {
                motion::leave(row, ("toast-out", id as usize), -10.).into_any_element()
            } else {
                row.with_animation(("toast-in", id as usize), motion::ease(320), |el, t| el.opacity(t).top(px(-14. * (1. - t))))
                    .into_any_element()
            });
        }
        column
    }

    // ---------- overlays ----------

    fn render_overlay(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut layers = Vec::new();
        if let Some(closing) = self.closing.clone().filter(|c| Some(c) != self.overlay.as_ref()) {
            layers.push(self.render_layer(closing, true, window, cx));
        }
        if let Some(overlay) = self.overlay.clone() {
            layers.push(self.render_layer(overlay, false, window, cx));
        }
        layers
    }

    fn render_layer(&self, overlay: Overlay, leaving: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let phase = if leaving { "out" } else { "in" };
        if overlay == Overlay::Drawer {
            let layer = div().absolute().top_0().left_0().size_full().child(self.render_drawer(cx));
            return if leaving {
                layer.with_animation("drawer-leave", motion::ease(motion::LEAVE_MS), |el, t| el.opacity(1. - t)).into_any_element()
            } else {
                motion::fade(layer, "drawer-scrim-in", 180).into_any_element()
            };
        }
        let key = overlay_key(&overlay);
        let content = match overlay {
            Overlay::Drawer => unreachable!(),
            Overlay::Downloads => self.render_downloads(cx).into_any_element(),
            Overlay::Settings(section) => self.render_settings(section, window, cx).into_any_element(),
            Overlay::Add => self.render_add(cx).into_any_element(),
            Overlay::Install(id) => self.render_install(id, cx).into_any_element(),
            Overlay::Remove(id) => self.render_remove(id, cx).into_any_element(),
            Overlay::Gate(id, gate) => self.render_gate(id, &gate, cx).into_any_element(),
            Overlay::SignIn => self.render_sign_in(window, cx).into_any_element(),
            Overlay::Instrument(id) => dialog(self.tr(K::instrument), 420., cx)
                .child(
                    div()
                        .flex()
                        .gap(px(10.))
                        .child(self.pick_mode(id, konaste_runtime::GitadoraMode::GuitarFreaks, self.tr(K::guitar), cx))
                        .child(self.pick_mode(id, konaste_runtime::GitadoraMode::DrumMania, self.tr(K::drum), cx)),
                )
                .into_any_element(),
        };
        let content = div().relative().child(content);
        let content = if leaving {
            motion::leave(content, SharedString::from(format!("{key}-{phase}")), 8.).into_any_element()
        } else {
            content
                .with_animation(SharedString::from(format!("{key}-{phase}")), motion::ease(260), |el, t| {
                    el.opacity(t).top(px(14. * (1. - t)))
                })
                .into_any_element()
        };
        let layer = scrim(cx).child(content);
        if leaving {
            layer.with_animation("scrim-out", motion::ease(motion::LEAVE_MS), |el, t| el.opacity(1. - t)).into_any_element()
        } else {
            layer.with_animation("scrim-in", motion::ease(160), |el, t| el.opacity(t)).into_any_element()
        }
    }

    fn pick_mode(&self, id: GameId, mode: konaste_runtime::GitadoraMode, label: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(SharedString::from(format!("pick-{label}")))
            .flex_1()
            .h(px(88.))
            .rounded(px(10.))
            .bg(rgba(0xffffff0d))
            .border_1()
            .border_color(theme::line())
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(16.))
            .font_weight(FontWeight::BOLD)
            .cursor_pointer()
            .hover(|s| s.border_color(theme::accent()).bg(rgba((theme::ACCENT << 8) | 0x29)))
            .on_click(cx.listener(move |app, _, _, cx| {
                app.close_overlay(cx);
                app.set_preferences(|p| p.gitadora_mode = Some(mode), cx);
                if app.hub.state().snapshot().games.iter().any(|g| g.id == id && g.phase == GamePhase::Ready) {
                    app.start(id, cx);
                }
            }))
            .child(label)
    }

    fn render_drawer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let profile = self.snapshot.profile.clone().unwrap_or_default();
        let name = profile.data.name.clone().unwrap_or_else(|| self.tr(K::account).to_owned());
        let card = profile.data.card_number.clone();
        let reveal = self.reveal_card;
        let card_row = match card {
            Some(card) => div()
                .flex()
                .items_center()
                .justify_between()
                .font_family("Cascadia Mono")
                .text_size(px(17.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(PASS_INK))
                .child(if reveal { format_card(&card) } else { mask_card(&card) })
                .child(
                    div()
                        .id("reveal")
                        .size(px(30.))
                        .rounded_full()
                        .bg(rgba(0xffffffe6))
                        .shadow(theme::shadow(1., 4., 0.18))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|s| s.bg(white()))
                        .child(theme::icon(if reveal { icons::EYE_OFF } else { icons::EYE }, 16., rgb(0x55575f).into()))
                        .tooltip({
                            let label = self.tr(if reveal { K::hide_card } else { K::show_card });
                            move |_, cx| tooltip(label, cx)
                        })
                        .on_click(cx.listener(|app, _, _, cx| {
                            app.reveal_card = !app.reveal_card;
                            cx.notify();
                        })),
                )
                .into_any_element(),
            None => div().text_color(rgb(0x6b6d75)).child(self.tr(K::no_pass)).into_any_element(),
        };
        let value = |v: Option<u64>| v.map(group_digits).unwrap_or_else(|| "—".into());
        let drawer = div()
            .relative()
            .id("drawer")
            .occlude()
            .w(px(360.))
            .h_full()
            .px(px(20.))
            .pt(px(12.))
            .pb(px(20.))
            .flex()
            .flex_col()
            .gap(px(16.))
            .bg(theme::panel_solid())
            .border_l_1()
            .border_color(theme::line())
            .shadow(vec![BoxShadow::new(px(-20.), px(0.), hsla(0., 0., 0., 0.5)).blur_radius(px(50.))])
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(4.))
                    .child(icon_button("refresh", icons::ROTATE, 16.).on_click(cx.listener(|app, _, _, cx| {
                        if let Some(id) = app.selected {
                            app.hub.state().session.playdata.invalidate(id);
                        }
                        let hub = app.hub.clone();
                        app.hub.run(async move { hub.state().session.refresh(&hub).await });
                        cx.notify();
                    })))
                    .child(icon_button("drawer-close", icons::X, 18.).on_click(cx.listener(|app, _, _, cx| {
                        app.close_overlay(cx);
                        cx.notify();
                    }))),
            )
            .child(
                div().flex().items_center().gap(px(14.)).child(avatar(&profile, &name, 56.)).child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().text_size(px(20.)).font_weight(FontWeight::BOLD).truncate().child(name))
                        .children(
                            profile
                                .data
                                .konami_id
                                .clone()
                                .map(|id| div().text_size(px(13.)).text_color(theme::muted()).truncate().child(id)),
                        ),
                ),
            )
            .child(
                div()
                    .relative()
                    .rounded(px(12.))
                    .overflow_hidden()
                    .bg(rgb(0xf6f6f8))
                    .shadow(theme::shadow(6., 18., 0.35))
                    .children(self.pass_card.clone().map(|face| {
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .child(img(face).size_full().rounded(px(12.)).object_fit(ObjectFit::Cover))
                    }))
                    .child(
                        div()
                            .relative()
                            .px(px(16.))
                            .py(px(14.))
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    // The mark is white; the pass prints it on a dark tile.
                                    .children(
                                        self.service_mark(Service::EAmusement, 14.)
                                            .map(|mark| div().px(px(7.)).py(px(4.)).rounded(px(6.)).bg(rgb(0x1d1d22)).child(mark)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(rgb(0x4a4c55))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(self.tr(K::eapass)),
                                    ),
                            )
                            .child(card_row)
                            .children(
                                profile
                                    .data
                                    .card_memo
                                    .clone()
                                    .map(|memo| div().mt(px(-4.)).text_size(px(12.)).text_color(rgb(0x5f616a)).truncate().child(memo)),
                            ),
                    ),
            )
            .child(
                div()
                    .rounded(px(12.))
                    .px(px(16.))
                    .py(px(14.))
                    .border_1()
                    .border_color(rgba(0xffffff26))
                    .bg(linear_gradient(135., linear_color_stop(rgb(PASELI_GREEN), 0.), linear_color_stop(rgb(0x0f7a45), 1.)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .mb(px(8.))
                            // The mark is a green tile; a white ring keeps it visible on the green card.
                            .children(
                                self.service_mark(Service::Paseli, 20.)
                                    .map(|mark| div().p(px(1.5)).rounded(px(6.)).bg(rgba(0xffffffd9)).child(mark)),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgba(0xffffffe0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.tr(K::paseli)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap(px(22.))
                            .child(
                                div()
                                    .child(
                                        div()
                                            .text_size(px(28.))
                                            .font_weight(FontWeight::EXTRA_BOLD)
                                            .text_color(white())
                                            .child(value(profile.data.paseli_balance)),
                                    )
                                    .child(div().text_size(px(12.)).text_color(rgba(0xffffffb8)).child(self.tr(K::balance))),
                            )
                            .child(
                                div()
                                    .child(
                                        div()
                                            .text_size(px(20.))
                                            .font_weight(FontWeight::EXTRA_BOLD)
                                            .text_color(white())
                                            .child(value(profile.data.paseli_points)),
                                    )
                                    .child(div().text_size(px(12.)).text_color(rgba(0xffffffb8)).child(self.tr(K::points))),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .id("charge")
                                    .h(px(36.))
                                    .px(px(16.))
                                    .rounded(px(10.))
                                    .bg(white())
                                    .text_color(rgb(0x0b5e35))
                                    .font_weight(FontWeight::BOLD)
                                    .flex()
                                    .items_center()
                                    .gap(px(7.))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(0xe3f7ec)))
                                    .on_click(|_, _, _| {
                                        let _ = crate::platform::open_url(PASELI_CHARGE);
                                    })
                                    .child(self.tr(K::charge))
                                    .child(theme::icon(icons::EXTERNAL, 14., rgb(0x0b5e35).into())),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(account_link("link-mypage", self.tr(K::mypage), MYPAGE))
                    .child(account_link("link-my-konami", self.tr(K::my_konami), MY_KONAMI))
                    .child(account_link("link-pass-menu", self.tr(K::pass_menu), PASS_MENU))
                    .child(account_link("link-coop", self.tr(K::coop), COOP))
                    .child(account_link("link-course", self.tr(K::course), COURSE)),
            )
            .child(div().flex_1())
            .child(div().child(
                ghost("sign-out", self.tr(K::sign_out), Some(icons::LOG_OUT)).on_click(cx.listener(|app, _, _, cx| app.sign_out(cx))),
            ));
        div()
            .id("drawer-scrim")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .justify_end()
            .bg(rgba(0x00000066))
            .on_click(cx.listener(|app, _, _, cx| {
                app.close_overlay(cx);
                cx.notify();
            }))
            .child(if self.overlay == Some(Overlay::Drawer) {
                drawer.with_animation("drawer-in", motion::ease(300), |el, t| el.left(px(48. * (1. - t)))).into_any_element()
            } else {
                drawer.with_animation("drawer-away", motion::ease(motion::LEAVE_MS), |el, t| el.left(px(48. * t))).into_any_element()
            })
    }

    fn render_downloads(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // Active first, then the queue in its stored order.
        let mut active: Vec<GameSnapshot> = self.snapshot.games.iter().filter(|g| g.update_pending).cloned().collect();
        active.sort_by_key(|g| (g.phase == GamePhase::Queued, g.queue_position.unwrap_or(0)));
        let queue: Vec<GameId> = active.iter().filter(|g| g.phase == GamePhase::Queued).map(|g| g.id).collect();
        let mut body = div().flex().flex_col().gap(px(4.));
        if active.is_empty() {
            body = body.child(div().py(px(50.)).flex().justify_center().text_color(theme::muted()).child(self.tr(K::no_downloads)));
        }
        for (index, game) in active.into_iter().enumerate() {
            let label = if game.install_dir.is_some() { self.tr(K::update) } else { self.tr(K::install) };
            let queued = game.phase == GamePhase::Queued;
            let waiting = game.phase == GamePhase::Downloading && game.transfer.as_ref().is_some_and(|t| t.waiting_for_network);
            let slot = queue.iter().position(|id| *id == game.id);
            let mut controls = div().flex().items_center().gap(px(2.));
            if waiting {
                controls = controls.child(motion::fade(
                    div()
                        .mr(px(6.))
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .text_size(px(12.))
                        .text_color(theme::warn())
                        .child(theme::icon(icons::ALERT, 13., theme::warn()))
                        .child(self.tr(K::waiting_for_network)),
                    ("dl-wait", game.id as usize),
                    220,
                ));
            }
            if let Some(slot) = slot {
                controls = controls
                    .child(
                        div()
                            .mr(px(6.))
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child(fill(self.tr(K::queue_position), &[&game.queue_position.unwrap_or(slot + 1).to_string()])),
                    )
                    .child(icon_button(("queue-top", game.id as usize), icons::ARROW_UP_TO_LINE, 15.).when(slot > 0, |b| {
                        b.on_click(cx.listener(move |app, _, _, _| {
                            let hub = app.hub.clone();
                            app.hub.run(async move { crate::update::reorder(&hub, game.id, true) });
                        }))
                        .tooltip({
                            let label = self.tr(K::move_to_front);
                            move |_, cx| tooltip(label, cx)
                        })
                    }))
                    .child(icon_button(("queue-up", game.id as usize), icons::ARROW_UP_NARROW_WIDE, 15.).when(slot > 0, |b| {
                        b.on_click(cx.listener(move |app, _, _, _| {
                            let hub = app.hub.clone();
                            app.hub.run(async move { crate::update::reorder(&hub, game.id, false) });
                        }))
                        .tooltip({
                            let label = self.tr(K::move_up);
                            move |_, cx| tooltip(label, cx)
                        })
                    }))
                    .child(
                        icon_button(("queue-remove", game.id as usize), icons::X, 15.)
                            .on_click(cx.listener(move |app, _, _, _| {
                                let hub = app.hub.clone();
                                app.hub.run(async move { crate::update::dequeue(&hub, game.id) });
                            }))
                            .tooltip({
                                let label = self.tr(K::remove_from_queue);
                                move |_, cx| tooltip(label, cx)
                            }),
                    );
            }
            body = body.child(motion::rise_after(
                div()
                    .id(("dl-row", game.id as usize))
                    .flex()
                    .gap(px(14.))
                    .p(px(12.))
                    .rounded(px(10.))
                    .bg(rgba(0xffffff08))
                    .child(self.game_icon(game.id, 48.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .gap(px(10.))
                                    .items_end()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(crate::catalog::descriptor(game.id).title)
                                    .child(
                                        div().text_size(px(12.)).font_weight(FontWeight::NORMAL).text_color(theme::muted()).child(label),
                                    ),
                            )
                            .child(div().pt(px(6.)).child(if queued {
                                self.queued_card(&game, cx).into_any_element()
                            } else {
                                self.progress_card(&game, false, cx).into_any_element()
                            })),
                    )
                    .child(controls),
                ("dl-in", game.id as usize),
                index as u64 * 35,
                240,
                8.,
            ));
        }
        dialog(self.tr(K::downloads), 760., cx).child(body)
    }

    /// A queued item has no transfer yet: show its place, not a fake 0% bar.
    fn queued_card(&self, game: &GameSnapshot, _cx: &mut Context<Self>) -> impl IntoElement {
        let position = game.queue_position.unwrap_or(1);
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .text_size(px(12.))
            .text_color(theme::muted())
            .child(theme::icon(icons::CLOCK, 14., theme::muted()))
            .child(self.tr(K::queued))
            .child(fill(self.tr(K::queue_position), &[&position.to_string()]))
    }

    fn render_add(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut grid = div().flex().flex_wrap().gap(px(12.));
        let mut available: Vec<&GameSnapshot> =
            self.snapshot.games.iter().filter(|g| g.install_dir.is_none() && !g.update_pending).collect();
        // Detected installs first: they need one click, not a folder picker.
        available.sort_by_key(|g| (g.detected_dir.is_none(), !g.supported));
        for (ix, game) in available.into_iter().enumerate() {
            let id = game.id;
            let supported = game.supported;
            let detected = game.detected_dir.clone();
            let available = supported || detected.is_some();
            let key = format!("add-{}", id_str(id));
            let hover = self.is_hovered(&key);
            let band = if self.backgrounds.contains_key(&id) { 0. } else { -39. };
            let body = if let Some(root) = detected {
                let shown = root.display().to_string().trim_start_matches(r"\\?\").to_owned();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(div().text_size(px(12.)).text_color(theme::muted()).truncate().child(fill(self.tr(K::found_at), &[&shown])))
                    .child(
                        compact(("add-found", id as usize), self.tr(K::add), Some(icons::FOLDER))
                            .on_click(cx.listener(move |app, _, _, cx| app.adopt(id, root.clone(), cx))),
                    )
                    .into_any_element()
            } else if supported {
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        compact(("install", id as usize), self.tr(K::install), Some(icons::DOWNLOAD))
                            .on_click(cx.listener(move |app, _, _, cx| app.open_install(id, cx))),
                    )
                    .child(
                        ghost(("locate", id as usize), self.tr(K::locate), Some(icons::FOLDER))
                            .h(px(36.))
                            .on_click(cx.listener(move |app, _, _, cx| app.locate(id, cx))),
                    )
                    .into_any_element()
            } else {
                div().text_color(theme::muted()).child(self.tr(K::not_supported)).into_any_element()
            };
            grid = grid.child(motion::rise_after(
                div()
                    .id(SharedString::from(key.clone()))
                    .relative()
                    .on_hover(self.hover_listener(key.clone(), cx))
                    .w(px(352.))
                    .rounded(px(10.))
                    .overflow_hidden()
                    .bg(rgba(0xffffff0a))
                    .border_1()
                    .border_color(if hover && available { Hsla::from(rgba(0xffffff26)) } else { theme::line() })
                    .child(
                        div()
                            .relative()
                            .h(px(120.))
                            .overflow_hidden()
                            .opacity(if available { 1. } else { 0.45 })
                            .child(motion::spring(
                                {
                                    let layer = div().absolute();
                                    match self.backdrop_image(id) {
                                        Some(image) => layer.child(img(image).size_full().object_fit(ObjectFit::Cover)),
                                        None => {
                                            let (a, b) = colors(id);
                                            layer.bg(linear_gradient(135., linear_color_stop(rgb(a), 0.), linear_color_stop(rgb(b), 1.)))
                                        }
                                    }
                                },
                                SharedString::from(format!("{key}-zoom")),
                                SETTLE,
                                0.,
                                if hover && available { 1. } else { 0. },
                                move |el, t| {
                                    // Grow around the center by up to 5%. Local backgrounds
                                    // keep their top band, where the title logo sits.
                                    let (w, h) = (352. * (1. + 0.05 * t), 198. * (1. + 0.05 * t));
                                    el.w(px(w)).h(px(h)).left(px((352. - w) / 2.)).top(px(band + (198. - h) / 2.))
                                },
                            ))
                            .when(!self.backgrounds.contains_key(&id), |card| {
                                card.child(div().absolute().top_0().left_0().size_full().bg(linear_gradient(
                                    90.,
                                    linear_color_stop(rgba(0x000000a6), 0.),
                                    linear_color_stop(rgba(0x00000000), 0.7),
                                )))
                                .child(div().absolute().top_0().left(px(18.)).h_full().flex().items_center().child(logo_block(id, 0.38)))
                            }),
                    )
                    .child(
                        div()
                            .px(px(14.))
                            .pt(px(12.))
                            .pb(px(14.))
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(crate::catalog::descriptor(id).title))
                            .child(body),
                    ),
                SharedString::from(format!("{key}-in")),
                60 + ix as u64 * 45,
                320,
                12.,
            ));
        }
        dialog(self.tr(K::add_game), 760., cx).child(grid)
    }

    fn render_install(&self, id: GameId, cx: &mut Context<Self>) -> impl IntoElement {
        let path = self.install_path.display().to_string();
        let free = self.install_free.map(i18n::bytes).unwrap_or_else(|| "—".into());
        let busy = self.hub.state().updates.any_active() || self.snapshot.games.iter().any(|g| g.phase == GamePhase::Queued);
        dialog(fill(self.tr(K::install_title), &[crate::catalog::descriptor(id).title]), 520., cx)
            .when(busy, |d| d.child(div().text_size(px(13.)).text_color(theme::muted()).mb(px(10.)).child(self.tr(K::queued_note))))
            .child(div().text_size(px(13.)).text_color(theme::muted()).mb(px(6.)).child(self.tr(K::install_folder)))
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(px(34.))
                            .px(px(10.))
                            .rounded(px(7.))
                            .bg(rgb(0x0d0e11))
                            .border_1()
                            .border_color(rgba(0xffffff1f))
                            .flex()
                            .items_center()
                            .child(div().truncate().child(path.clone())),
                    )
                    .child(ghost("browse", self.tr(K::browse), None).h(px(34.)).on_click(cx.listener(|app, _, _, cx| {
                        app.pick_folder(|app, folder, cx| app.set_install_path(folder, cx), cx);
                    }))),
            )
            .child(div().mt(px(12.)).text_size(px(13.)).text_color(theme::muted()).child(format!("{}  {}", self.tr(K::free_space), free)))
            .child(
                div()
                    .mt(px(18.))
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(ghost("cancel", self.tr(K::cancel), None).h(px(36.)).px(px(16.)).on_click(cx.listener(|app, _, _, cx| {
                        app.open(Overlay::Add, cx);
                        cx.notify();
                    })))
                    .child(
                        compact("confirm-install", self.tr(K::install), None)
                            .when(self.install_submitting, |button| button.opacity(0.55).child(spinner_icon(14.)))
                            .when(!self.install_submitting, |button| {
                                button.on_click(cx.listener(move |app, _, _, cx| app.start_install(id, cx)))
                            }),
                    ),
            )
    }

    /// The game login page's question, natively. The terms are the page's own
    /// text; the buttons send the same answer its links send.
    fn render_gate(&self, id: GameId, gate: &crate::account::Gate, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::account::{Answer, Gate, TermsBlock};
        let title = crate::catalog::descriptor(id).title;
        let buttons = |confirm: Option<(&'static str, Answer)>, cx: &mut Context<Self>| {
            let mut row = div().mt(px(18.)).flex().justify_end().gap(px(8.)).child(
                ghost("gate-cancel", self.tr(K::cancel), None).h(px(36.)).px(px(16.)).on_click(cx.listener(|app, _, _, cx| {
                    app.close_overlay(cx);
                    cx.notify();
                })),
            );
            if let Some((label, answer)) = confirm {
                row = row.child(
                    compact("gate-confirm", label, None).on_click(cx.listener(move |app, _, _, cx| app.answer_gate(id, answer, cx))),
                );
            }
            row
        };
        match gate {
            Gate::Terms(blocks) => {
                let mut text = div().id("terms-text").flex().flex_col().gap(px(8.)).max_h(px(420.)).overflow_y_scroll().pr(px(8.));
                for block in blocks {
                    text = text.child(match block {
                        TermsBlock::Heading(line) => div().mt(px(6.)).font_weight(FontWeight::BOLD).child(line.clone()),
                        TermsBlock::Text(line) => div().text_size(px(13.)).text_color(rgb(0xc9cbd1)).child(line.clone()),
                        TermsBlock::Item(line) => div().pl(px(14.)).text_size(px(13.)).text_color(rgb(0xc9cbd1)).child(line.clone()),
                    });
                }
                dialog(self.tr(K::terms_title), 640., cx)
                    .child(div().text_size(px(13.)).text_color(theme::muted()).mb(px(12.)).child(fill(self.tr(K::terms_body), &[title])))
                    .child(div().rounded(px(8.)).bg(rgb(0x0d0e11)).border_1().border_color(theme::line()).p(px(14.)).child(text))
                    .child(buttons(Some((self.tr(K::terms_accept), Answer::AcceptTerms)), cx))
            }
            Gate::StillRunning => dialog(self.tr(K::still_running_title), 480., cx)
                .child(div().text_color(rgb(0xc9cbd1)).child(self.tr(K::still_running_body)))
                .child(buttons(Some((self.tr(K::still_running_continue), Answer::Continue)), cx)),
            Gate::Unknown => dialog(self.tr(K::gate_unknown_title), 480., cx)
                .child(div().text_color(rgb(0xc9cbd1)).child(self.tr(K::gate_unknown_body)))
                .child(buttons(None, cx).child(compact("gate-site", self.tr(K::official_site), None).on_click(cx.listener(
                    move |app, _, _, cx| {
                        if let Some(url) = crate::account::game_login_url(id) {
                            let _ = crate::platform::open_url(&url);
                        }
                        app.close_overlay(cx);
                        cx.notify();
                    },
                )))),
        }
    }

    fn render_sign_in(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::signin::{CodeKind, Problem, Step};
        use gpui_kit::component::{
            Disableable as _,
            input::{Input, OtpInput},
        };
        let card = dialog(self.tr(K::sign_in_window), 420., cx);
        let Some(state) = self.sign_in.as_ref() else {
            return card;
        };
        let flow = &state.flow;
        let busy = flow.busy();
        let problem = flow.problem().map(|p| {
            self.tr(match p {
                Problem::Credentials => K::e_credentials,
                Problem::Captcha => K::e_captcha,
                Problem::Code => K::e_code,
                Problem::Passkey => K::e_passkey,
                Problem::Refused => K::e_refused,
                Problem::Unavailable => K::e_signInUnavailable,
            })
        });
        let error = problem.map(|text| {
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .mt(px(10.))
                .text_size(px(13.))
                .text_color(theme::danger_text())
                .child(theme::icon(icons::ALERT, 14., theme::danger_text()))
                .child(text)
        });
        let next = |enabled: bool, cx: &mut Context<Self>| {
            let mut button = div()
                .id("signin-next")
                .h(px(40.))
                .w_full()
                .rounded(px(10.))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .font_weight(FontWeight::BOLD)
                .text_color(white());
            button = if busy {
                button.bg(theme::accent()).opacity(0.7).child(spinner_icon(16.))
            } else if enabled {
                button
                    .bg(theme::accent())
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::accent_hover()))
                    .active(|s| s.bg(theme::accent_active()))
                    .on_click(cx.listener(|app, _, _, cx| app.sign_in_next(cx)))
            } else {
                button.bg(theme::accent()).opacity(0.4)
            };
            button.child(self.tr(K::next))
        };
        let link = |id: &'static str, label: &'static str| {
            div().id(id).text_size(px(13.)).text_color(theme::muted()).cursor_pointer().hover(|s| s.text_color(theme::text())).child(label)
        };
        let back = |cx: &mut Context<Self>| {
            div()
                .id("signin-back")
                .flex()
                .items_center()
                .gap(px(6.))
                .mb(px(12.))
                .text_size(px(13.))
                .text_color(theme::muted())
                .cursor_pointer()
                .hover(|s| s.text_color(theme::text()))
                .child(theme::icon(icons::BACK, 14., theme::muted()))
                .child(flow.login_id().to_owned())
                .on_click(cx.listener(|app, _, window, cx| {
                    if let Some(state) = app.sign_in.as_mut() {
                        #[cfg(windows)]
                        crate::passkey::cancel();
                        state.flow.back();
                        state.id.update(cx, |input, cx| input.focus(window, cx));
                    }
                    cx.notify();
                }))
        };
        let step = flow.step();
        let body = match step {
            Step::Id | Step::Passkey => {
                let waiting = step == Step::Passkey;
                let passkey = div()
                    .id("signin-passkey")
                    .h(px(40.))
                    .w_full()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(theme::line())
                    .bg(theme::ghost())
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(8.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(if waiting {
                        spinner_icon(16.).into_any_element()
                    } else {
                        theme::icon(icons::KEY, 16., theme::text()).into_any_element()
                    })
                    .child(self.tr(K::passkey));
                let passkey = if waiting {
                    passkey.opacity(0.7)
                } else {
                    passkey.cursor_pointer().hover(|s| s.bg(theme::ghost_hover())).on_click(cx.listener(|app, _, _, cx| {
                        if let Some(state) = app.sign_in.as_mut() {
                            let id = state.id.read(cx).value();
                            state.flow.start_passkey(&id);
                        }
                        cx.notify();
                    }))
                };
                let has_id = !state.id.read(cx).value().trim().is_empty();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(Input::new(&state.id).disabled(waiting || busy))
                    .children(error)
                    .child(if waiting { div().w_full().child(div()) } else { div().w_full().child(next(has_id, cx)) })
                    .child(passkey)
                    .children(waiting.then(|| div().text_size(px(13.)).text_color(theme::muted()).child(self.tr(K::passkey_wait))))
                    .into_any_element()
            }
            Step::Password => {
                let captcha = flow.captcha();
                let picture = |index: usize, image: Option<&Arc<Image>>, size: f32| {
                    let tile = div().size(px(size)).rounded(px(8.)).overflow_hidden().bg(rgba(0xffffff0f)).flex_none();
                    match image {
                        Some(image) => tile.child(img(image.clone()).size_full().object_fit(ObjectFit::Contain)).into_any_element(),
                        None => motion::pulse(tile, SharedString::from(format!("pic-wait-{index}")), 0.4).into_any_element(),
                    }
                };
                let images = &state.pictures.1;
                let mut grid = div().flex().gap(px(8.));
                for index in 0..crate::signin::PICTURES {
                    let on = captcha.selected[index];
                    let tile = div()
                        .id(SharedString::from(format!("pic-{index}")))
                        .relative()
                        .rounded(px(10.))
                        .border_2()
                        .border_color(if on { theme::accent() } else { Hsla::from(rgba(0x00000000)) })
                        .cursor_pointer()
                        .child(picture(index + 1, images.get(index + 1).and_then(Option::as_ref), 62.))
                        .children(on.then(|| {
                            motion::fade(
                                div()
                                    .absolute()
                                    .top(px(4.))
                                    .right(px(4.))
                                    .size(px(18.))
                                    .rounded_full()
                                    .bg(theme::accent())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(theme::icon(icons::CHECK, 12., white())),
                                SharedString::from(format!("pic-on-{index}")),
                                120,
                            )
                        }))
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if let Some(state) = app.sign_in.as_mut() {
                                state.flow.toggle(index);
                            }
                            cx.notify();
                        }));
                    grid = grid.child(tile);
                }
                let has_password = !state.password.read(cx).value().is_empty();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(back(cx))
                    .child(Input::new(&state.password).mask_toggle().disabled(busy))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(picture(0, images.first().and_then(Option::as_ref), 56.))
                            .child(div().flex_1().text_size(px(13.)).text_color(rgb(0xc9cbd1)).child(self.tr(K::captcha)))
                            .child(
                                icon_button("signin-pictures", icons::REFRESH, 15.)
                                    .tooltip({
                                        let label = self.tr(K::new_pictures);
                                        move |_, cx| tooltip(label, cx)
                                    })
                                    .on_click(cx.listener(|app, _, _, cx| {
                                        if let Some(state) = app.sign_in.as_mut() {
                                            state.flow.load_captcha();
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(grid)
                    .children(error)
                    .child(next(has_password && captcha.ready(), cx))
                    .child(link("signin-forgot", self.tr(K::forgot_password)).on_click(|_, _, _| {
                        let _ = crate::platform::open_url(crate::signin::PASSWORD_HELP);
                    }))
                    .into_any_element()
            }
            Step::Code(kind) => {
                let text = self.tr(if kind == CodeKind::Sms { K::code_sms } else { K::code_email });
                let complete = state.code.read(cx).value().len() == 6;
                let mut column = div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(back(cx))
                    .child(div().text_size(px(13.)).text_color(rgb(0xc9cbd1)).child(text))
                    .child(div().flex().justify_center().child(OtpInput::new(&state.code).groups(1).disabled(busy)))
                    .children(error);
                if kind == CodeKind::TwoStep {
                    let trust = flow.trust_device;
                    column = column.child(
                        div().flex().items_center().justify_between().text_size(px(13.)).child(self.tr(K::trust_device)).child(
                            switch("signin-trust", trust).on_click(cx.listener(|app, _, _, cx| {
                                if let Some(state) = app.sign_in.as_mut() {
                                    state.flow.trust_device = !state.flow.trust_device;
                                }
                                cx.notify();
                            })),
                        ),
                    );
                }
                column = column.child(next(complete, cx));
                if kind != CodeKind::TwoStep {
                    column = column.child(link("signin-resend", self.tr(K::resend_code)).on_click(cx.listener(|app, _, _, cx| {
                        if let Some(state) = app.sign_in.as_mut() {
                            state.flow.resend_code();
                        }
                        cx.notify();
                    })));
                }
                if kind == CodeKind::Email && flow.passkey_account() {
                    column = column.child(link("signin-passkey-again", self.tr(K::passkey)).on_click(cx.listener(|app, _, _, cx| {
                        if let Some(state) = app.sign_in.as_mut() {
                            let id = state.flow.login_id().to_owned();
                            state.flow.start_passkey(&id);
                        }
                        cx.notify();
                    })));
                }
                column.into_any_element()
            }
            Step::Finishing => div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(12.))
                .py(px(28.))
                .child(spinner_icon(26.))
                .child(div().text_color(theme::muted()).child(format!("{}…", self.tr(K::signing_in))))
                .into_any_element(),
            Step::Website => div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .child(div().text_color(rgb(0xc9cbd1)).child(self.tr(K::website_step)))
                .child(div().flex().justify_end().child(compact("signin-website", self.tr(K::my_konami), Some(icons::EXTERNAL)).on_click(
                    cx.listener(|app, _, _, cx| {
                        let _ = crate::platform::open_url(MY_KONAMI);
                        app.close_overlay(cx);
                        cx.notify();
                    }),
                )))
                .into_any_element(),
        };
        // Keyed by step so each step slides in.
        let key = match step {
            Step::Id | Step::Passkey => "id".to_owned(),
            Step::Password => "password".to_owned(),
            Step::Code(kind) => format!("code-{kind:?}"),
            Step::Finishing => "finishing".to_owned(),
            Step::Website => "website".to_owned(),
        };
        // Enter in any field moves to the next step. Watched here as well as
        // on the fields, since a field may handle the key before it bubbles.
        card.on_key_down(cx.listener(|app, event: &KeyDownEvent, _, cx| {
            if event.keystroke.key == "enter" {
                app.sign_in_next(cx);
            }
        }))
        .child(motion::slide(div().relative().child(body), SharedString::from(format!("signin-{key}")), 220, 12.))
    }

    fn render_remove(&self, id: GameId, cx: &mut Context<Self>) -> impl IntoElement {
        let title = crate::catalog::descriptor(id).title;
        let path = self
            .game(id)
            .and_then(|g| g.install_dir.clone())
            .map(|p| p.display().to_string().trim_start_matches(r"\\?\").to_owned())
            .unwrap_or_default();
        dialog(fill(self.tr(K::remove_title), &[title]), 440., cx)
            .child(div().text_color(rgb(0xc9cbd1)).child(fill(self.tr(K::remove_body), &[&path])))
            .child(
                div()
                    .mt(px(18.))
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(ghost("cancel", self.tr(K::cancel), None).h(px(36.)).px(px(16.)).on_click(cx.listener(|app, _, _, cx| {
                        app.close_overlay(cx);
                        cx.notify();
                    })))
                    .child(
                        compact("confirm-remove", self.tr(K::remove), None)
                            .bg(theme::accent())
                            .on_click(cx.listener(move |app, _, _, cx| app.remove(id, cx))),
                    ),
            )
    }

    fn render_settings(&self, section: Section, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let preferences = self.snapshot.preferences.clone();
        let nav_item = |key: &'static str, label: &'static str, target: Section, cx: &mut Context<Self>| {
            let on = section == target;
            div()
                .id(key)
                .px(px(10.))
                .py(px(8.))
                .rounded(px(7.))
                .cursor_pointer()
                .text_color(if on { theme::text() } else { theme::muted() })
                .when(on, |d| d.bg(rgba(0xffffff14)))
                .hover(|s| s.text_color(theme::text()))
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.settings_tab = target;
                    app.open(Overlay::Settings(target), cx);
                    cx.notify();
                }))
                .child(label)
        };
        let nav = div()
            .w(px(190.))
            .flex_none()
            .px(px(10.))
            .py(px(18.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .bg(rgba(0x00000033))
            .border_r_1()
            .border_color(theme::line())
            .child(div().px(px(10.)).pb(px(14.)).text_size(px(17.)).font_weight(FontWeight::BOLD).child(self.tr(K::settings)))
            .child(nav_item("s-general", self.tr(K::general), Section::General, cx))
            .child(nav_item("s-downloads", self.tr(K::downloads), Section::Downloads, cx))
            .child(nav_item("s-account", self.tr(K::account), Section::Account, cx))
            .child(nav_item("s-about", self.tr(K::about), Section::About, cx));
        let mut body = div().relative().flex_1().px(px(28.)).pt(px(54.)).pb(px(24.)).flex().flex_col().gap(px(4.)).child(
            div().absolute().top(px(12.)).right(px(12.)).child(icon_button("settings-close", icons::X, 18.).on_click(cx.listener(
                |app, _, _, cx| {
                    app.close_overlay(cx);
                    cx.notify();
                },
            ))),
        );
        match section {
            Section::General => {
                let locale = self.locale();
                let options = Locale::ALL.iter().map(|l| (SharedString::from(l.label()), *l == locale)).collect();
                let langs = self.dropdown("language", options, cx, |app, ix, cx| {
                    let code = Locale::ALL[ix].code();
                    app.set_preferences(|p| p.locale = code.into(), cx);
                });
                body = body.child(setting_row(self.tr(K::language), langs)).child(setting_row(
                    self.tr(K::close_button),
                    div()
                        .flex()
                        .gap(px(4.))
                        .child(
                            segment("tray", self.tr(K::close_tray), preferences.close_to_tray)
                                .on_click(cx.listener(|app, _, _, cx| app.set_preferences(|p| p.close_to_tray = true, cx))),
                        )
                        .child(
                            segment("exit", self.tr(K::close_exit), !preferences.close_to_tray)
                                .on_click(cx.listener(|app, _, _, cx| app.set_preferences(|p| p.close_to_tray = false, cx))),
                        ),
                ));
            }
            Section::Downloads => {
                let folder = preferences.default_folder.clone().unwrap_or_else(crate::platform::default_games_folder);
                const PARALLEL: [usize; 4] = [2, 4, 6, 8];
                let options = PARALLEL.iter().map(|n| (SharedString::from(n.to_string()), preferences.parallel == *n)).collect();
                let parallel = self.dropdown("parallel", options, cx, |app, ix, cx| {
                    app.set_preferences(|p| p.parallel = PARALLEL[ix], cx);
                });
                body = body
                    .child(setting_row(
                        self.tr(K::default_folder),
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().max_w(px(260.)).truncate().text_color(theme::muted()).child(folder.display().to_string()))
                            .child(ghost("folder-browse", self.tr(K::browse), None).on_click(cx.listener(|app, _, _, cx| {
                                app.pick_folder(|app, folder, cx| app.set_preferences(|p| p.default_folder = Some(folder), cx), cx);
                            }))),
                    ))
                    .child(setting_row(self.tr(K::parallel), parallel));
            }
            Section::Account => {
                let action = if self.snapshot.account == AccountStatus::SignedIn {
                    ghost("s-signout", self.tr(K::sign_out), Some(icons::LOG_OUT))
                        .on_click(cx.listener(|app, _, _, cx| app.sign_out(cx)))
                        .into_any_element()
                } else if matches!(self.snapshot.account, AccountStatus::Unknown | AccountStatus::SigningIn) {
                    div().text_color(theme::muted()).child(format!("{}…", self.tr(K::signing_in))).into_any_element()
                } else {
                    compact("s-signin", self.tr(K::sign_in), None)
                        .on_click(cx.listener(|app, _, window, cx| {
                            app.close_overlay(cx);
                            app.sign_in(None, window, cx);
                        }))
                        .into_any_element()
                };
                body = body
                    .child(setting_row(
                        self.tr(K::remember),
                        switch("remember", preferences.remember)
                            .on_click(cx.listener(|app, _, _, cx| app.set_preferences(|p| p.remember = !p.remember, cx))),
                    ))
                    .child(setting_row(self.tr(K::account), action));
            }
            Section::About => {
                body = body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(
                            div().flex().gap(px(12.)).items_center().child(div().size(px(40.)).child(img("icon.svg").size_full())).child(
                                div()
                                    .child(div().font_weight(FontWeight::BOLD).child("Konaste Hub"))
                                    .child(div().text_color(theme::muted()).child(env!("CARGO_PKG_VERSION"))),
                            ),
                        )
                        .child(self.tr(K::unofficial))
                        .child(div().text_color(theme::muted()).child(self.tr(K::license)))
                        .child({
                            let (stage, line) = match self.konami {
                                0 => (0, "© Konami Amusement"),
                                1..5 => (1, "© Konmai Amusement"),
                                _ => (2, "F**K YOU KONAMI 🖕"),
                            };
                            motion::fade(
                                div().id("konami").text_color(theme::muted()).child(line).on_click(cx.listener(|app, _, _, cx| {
                                    app.konami = app.konami.saturating_add(1);
                                    cx.notify();
                                })),
                                SharedString::from(format!("konami-{stage}")),
                                180,
                            )
                        })
                        .child(div().child(ghost("data", self.tr(K::data_folder), Some(icons::FOLDER)).on_click(cx.listener(
                            |app, _, _, _| {
                                let _ = crate::platform::open_folder(&app.hub.state().directory);
                            },
                        )))),
                );
            }
        }
        div()
            .id("settings")
            .occlude()
            .w(px(760.))
            .h(px(500.))
            .flex()
            .rounded(px(12.))
            .bg(theme::panel_solid())
            .border_1()
            .border_color(theme::line())
            .shadow(theme::shadow(24., 70., 0.6))
            .overflow_hidden()
            .child(nav)
            .child(body.with_animation(SharedString::from(format!("settings-{}", section as u8)), motion::ease(220), |el, t| {
                el.opacity(t).pl(px(28. + 8. * (1. - t)))
            }))
    }
}

fn overlay_key(overlay: &Overlay) -> &'static str {
    match overlay {
        Overlay::Drawer => "drawer",
        Overlay::Downloads => "downloads",
        Overlay::Settings(_) => "settings",
        Overlay::Add => "add",
        Overlay::Install(_) => "install",
        Overlay::Remove(_) => "remove",
        Overlay::Instrument(_) => "instrument",
        Overlay::Gate(..) => "gate",
        Overlay::SignIn => "signin",
    }
}

fn id_str(id: GameId) -> &'static str {
    match id {
        GameId::BomberGirl => "bomber-girl",
        GameId::Sdvx => "sdvx",
        GameId::Ddr => "ddr",
        GameId::Popn => "popn",
        GameId::Gitadora => "gitadora",
        GameId::Nostalgia => "nostalgia",
        GameId::Infinitas => "infinitas",
    }
}

fn progress_ratio(game: &GameSnapshot) -> f32 {
    match (&game.transfer, &game.work) {
        (Some(t), _) if matches!(game.phase, GamePhase::Downloading | GamePhase::Paused | GamePhase::Blocked) && t.bytes_total > 0 => {
            t.bytes_received as f32 / t.bytes_total as f32
        }
        _ => game.progress as f32 / 100.,
    }
}

fn badge(color: Hsla, icon: Option<&'static str>) -> Div {
    let mut b = div()
        .absolute()
        .right(px(-5.))
        .top(px(-5.))
        .size(px(18.))
        .rounded_full()
        .bg(color)
        .border_2()
        .border_color(theme::rail())
        .flex()
        .items_center()
        .justify_center();
    if let Some(icon) = icon {
        b = b.child(theme::icon(icon, 9., white()));
    }
    b
}

fn rail_tool(id: &'static str, icon: AnyElement, on: bool, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(44.))
        .rounded(px(10.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .when(on, |d| d.bg(rgba(0xffffff0f)))
        .hover(|s| s.bg(rgba(0xffffff0f)))
        .tooltip(move |_, cx| tooltip(label, cx))
        .child(icon)
}

fn tooltip(text: &'static str, cx: &mut gpui_kit::App) -> AnyView {
    cx.new(|_| Tip(text)).into()
}

struct Tip(&'static str);
impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(10.))
            .py(px(6.))
            .rounded(px(6.))
            .bg(rgb(0x000000))
            .text_color(white())
            .text_size(px(12.))
            .font_family(theme::FONT)
            .shadow(theme::shadow(4., 14., 0.5))
            .child(self.0)
    }
}

const PASELI_GREEN: u32 = 0x1bb367;
/// Card number on the white pass face.
const PASS_INK: u32 = 0x1d1d22;

/// A drawer row that opens an official page in the default browser.
fn account_link(id: &'static str, label: &'static str, url: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(36.))
        .px(px(10.))
        .mx(px(-10.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_between()
        .text_color(theme::text())
        .cursor_pointer()
        .hover(|s| s.bg(rgba(0xffffff0f)))
        .on_click(move |_, _, _| {
            let _ = crate::platform::open_url(url);
        })
        .child(label)
        .child(theme::icon(icons::EXTERNAL, 14., theme::muted()))
}

fn account_chip(id: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(8.))
        .h(px(34.))
        .pl(px(4.))
        .pr(px(12.))
        .rounded(px(17.))
        .bg(rgba(0x00000073))
        .border_1()
        .border_color(rgba(0xffffff1a))
        .cursor_pointer()
        .hover(|s| s.bg(rgba(0x000000a6)))
}

fn avatar(profile: &crate::account::Profile, name: &str, size: f32) -> AnyElement {
    if let Some(bytes) = &profile.avatar
        && let Some(format) = image_format(bytes)
    {
        return div()
            .size(px(size))
            .flex_none()
            .rounded_full()
            .overflow_hidden()
            .child(img(Arc::new(Image::from_bytes(format, bytes.to_vec()))).size_full().rounded_full().object_fit(ObjectFit::Cover))
            .into_any_element();
    }
    let initial: String = name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_default();
    div()
        .size(px(size))
        .flex_none()
        .rounded_full()
        .bg(linear_gradient(135., linear_color_stop(rgb(theme::ACCENT), 0.), linear_color_stop(rgb(0xe2493a), 1.)))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(size * 0.45))
        .font_weight(FontWeight::BOLD)
        .text_color(white())
        .child(initial)
        .into_any_element()
}

fn caption(id: &'static str, icon: &'static str, size: f32, area: WindowControlArea, close: bool) -> impl IntoElement {
    div()
        .id(id)
        .w(px(46.))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .window_control_area(area)
        .hover(move |s| s.bg(if close { theme::accent() } else { Hsla::from(rgba(0xffffff1a)) }))
        .child(theme::icon(icon, size, rgba(0xffffffd9).into()))
}

/// Numbers first, one compact line, small labels. Fades in when it arrives.
fn playdata_section(id: GameId, data: &PlayData, locale: Locale) -> impl IntoElement + use<> {
    let mut section = div().px(px(16.)).py(px(13.)).border_t_1().border_color(theme::line()).flex().flex_col().gap(px(9.));
    if let Some(name) = &data.name {
        section = section
            .child(div().font_family(theme::FONT).text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).truncate().child(name.clone()));
    }
    let mut metrics = div().flex().flex_wrap().gap_x(px(20.)).gap_y(px(8.));
    for stat in &data.stats {
        metrics = metrics.child(
            div()
                .flex()
                .items_baseline()
                .gap(px(6.))
                .child(
                    div()
                        .font_family(theme::FONT)
                        .text_size(px(18.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::text())
                        .child(stat.value.clone()),
                )
                .child(
                    div().font_family(theme::FONT).text_size(px(11.)).text_color(theme::muted()).child(t(locale, stat_label(stat.kind))),
                ),
        );
    }
    section = section.child(metrics);
    motion::fade(section, SharedString::from(format!("playdata-{}", id_str(id))), 320)
}

fn stat_label(kind: StatKind) -> K {
    match kind {
        StatKind::Volforce => K::volforce,
        StatKind::Skill => K::skill_level,
        StatKind::PlayCount => K::play_count,
        StatKind::LastPlay => K::last_play,
    }
}

fn info_tab(id: &'static str, label: &'static str, on: bool) -> impl IntoElement + StatefulInteractiveElement {
    div()
        .id(id)
        .relative()
        .pt(px(4.))
        .pb(px(9.))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .text_color(if on { theme::text() } else { theme::muted() })
        .hover(|s| s.text_color(theme::text()))
        .child(label)
        .child(motion::spring(
            div().absolute().bottom_0().left_0().h(px(2.)).w_full().rounded(px(1.)).bg(theme::accent()),
            SharedString::from(format!("{id}-line")),
            MOVE,
            if on { 1. } else { 0. },
            if on { 1. } else { 0. },
            |el, t| el.opacity(t.clamp(0., 1.)).w(relative(t.clamp(0., 1.2))),
        ))
}

fn scrim(cx: &mut Context<App>) -> Stateful<Div> {
    div().id("scrim").absolute().top_0().left_0().size_full().flex().items_center().justify_center().bg(rgba(0x0000008c)).on_click(
        cx.listener(|app, _, _, cx| {
            app.close_overlay(cx);
            cx.notify();
        }),
    )
}

fn dialog(title: impl Into<SharedString>, width: f32, cx: &mut Context<App>) -> Stateful<Div> {
    div()
        .id("dialog")
        .occlude()
        .on_click(|_, _, cx| cx.stop_propagation())
        .w(px(width))
        .max_h(relative(0.88))
        .flex()
        .flex_col()
        .rounded(px(12.))
        .bg(theme::panel_solid())
        .border_1()
        .border_color(theme::line())
        .shadow(theme::shadow(24., 70., 0.6))
        .px(px(20.))
        .pt(px(18.))
        .pb(px(20.))
        .overflow_y_scroll()
        .child(
            // A long title (a game name in Remove or Install) wraps beside the
            // close button instead of pushing it out of the dialog.
            div()
                .flex()
                .items_start()
                .justify_between()
                .gap(px(12.))
                .mb(px(14.))
                .child(div().flex_1().min_w_0().pt(px(3.)).text_size(px(17.)).font_weight(FontWeight::BOLD).child(title.into()))
                .child(icon_button("dialog-close", icons::X, 18.).on_click(cx.listener(|app, _, _, cx| {
                    app.close_overlay(cx);
                    cx.notify();
                }))),
        )
}

fn setting_row(label: &'static str, control: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(20.))
        .min_h(px(52.))
        .py(px(8.))
        .border_b_1()
        .border_color(theme::line())
        .child(label)
        .child(control)
}

fn segment(id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(12.))
        .py(px(5.))
        .rounded(px(6.))
        .text_size(px(13.))
        .cursor_pointer()
        .border_1()
        .border_color(theme::line())
        .text_color(if on { theme::text() } else { theme::muted() })
        .bg(if on { Hsla::from(rgba(0xffffff24)) } else { Hsla::from(rgba(0x00000080)) })
        .hover(|s| s.text_color(theme::text()))
        .child(label.into())
}

fn switch(id: &'static str, on: bool) -> Stateful<Div> {
    let target = if on { 1. } else { 0. };
    let track = motion::spring(
        div().absolute().top_0().left_0().size_full().rounded(px(11.)),
        SharedString::from(format!("{id}-track")),
        SETTLE,
        target,
        target,
        |el, t| el.bg(motion::mix(rgba(0xffffff2e).into(), theme::accent(), t)),
    );
    let knob = motion::spring(
        div().absolute().top(px(3.)).size(px(16.)).rounded_full().bg(white()).shadow(theme::shadow(1., 3., 0.3)),
        SharedString::from(format!("{id}-knob")),
        MOVE,
        target,
        target,
        |el, t| el.left(px(3. + 18. * t)),
    );
    div().id(id).relative().w(px(40.)).h(px(22.)).cursor_pointer().child(track).child(knob)
}

fn group_digits(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn short_date(date: &str) -> String {
    let parts: Vec<&str> = date.split('/').collect();
    if parts.len() == 3 { format!("{}/{}", parts[1].trim_start_matches('0'), parts[2].trim_start_matches('0')) } else { date.to_owned() }
}

fn jst_time(ms: u64) -> String {
    let seconds = ms / 1000 + 9 * 3600;
    format!("{:02}:{:02}", (seconds / 3600) % 24, (seconds / 60) % 60)
}

#[allow(dead_code)]
fn _catalog_len() -> usize {
    CATALOG.len()
}

/// The native handle behind a gpui window.
#[cfg(windows)]
fn hwnd_of(window: &Window) -> Option<windows_sys::Win32::Foundation::HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as windows_sys::Win32::Foundation::HWND),
        _ => None,
    }
}
