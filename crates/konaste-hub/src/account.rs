//! KONAMI ID session.
//!
//! The official sign-in page runs in a WebView2 window (see `browser.rs`), only
//! for entering credentials. After sign-in its cookies move into a native HTTP
//! client. Profile, PASELI, and the per-Play launch token are then fetched and
//! parsed natively. A launch token is never stored.
use crate::{
    catalog::{GameId, descriptor},
    hub::Hub,
    jar::{Jar, StoredCookie},
    profile::{AccountProfile, Collector, LoginFlag, login_flag},
    state::HubError,
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use zeroize::Zeroizing;

pub const MYPAGE: &str = "https://p.eagate.573.jp/gate/p/mypage/index.html";
pub const PASS_MENU: &str = "https://p.eagate.573.jp/gate/eapass/menu.html";
/// Header templates; signed in, they carry the KONAMI ID.
const HEADER_TEMPLATES: &str = "https://p.eagate.573.jp/gate/p/common/multi/ea_common_multi_lang.html";
/// JSON list of the account's passes, with their memos.
const PASS_LIST: &str = "https://p.eagate.573.jp/gate/eapass/api/getlist.html";
pub const PASELI_CHARGE: &str = "https://paseli.konami.net/charge/top.html";
pub const MY_KONAMI: &str = "https://my.konami.net/";
/// 購買部: in-game items bought with PASELI.
pub const COOP: &str = "https://p.eagate.573.jp/gate/p/eamusement/coop/index.html";
/// コース加入: some titles need a subscription for their normal mode.
pub const COURSE: &str = "https://p.eagate.573.jp/payment/p/select_course.html";
const GAME_LOGIN: &str = "https://p.eagate.573.jp/game/konasteapp/API/login/login.html";
const GAME_LOGIN_PATH: &str = "/game/konasteapp/API/login/login.html";
const TERMS_PATH: &str = "/game/konasteapp/API/page/terms_of_service/index.html";
const AGREE_PATH: &str = "/game/konasteapp/API/page/terms_of_service/agree.html";

/// The official per-game login page, for opening in the user's own browser.
pub fn game_login_url(id: GameId) -> Option<String> {
    Some(format!("{GAME_LOGIN}?game_id={}", descriptor(id).login_id?))
}

/// What the game login page asked before showing its start button. Play
/// stops with `officialPage`; the window shows this natively and the user's
/// answer comes back as an [`Answer`].
#[derive(Clone, Debug, PartialEq)]
pub enum Gate {
    /// The Konaste terms of service, as text blocks read from the page.
    Terms(Vec<TermsBlock>),
    /// "A previous game may still be running." Continuing ends that game.
    StillRunning,
    /// Something the Hub does not know how to show.
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TermsBlock {
    Heading(String),
    Text(String),
    Item(String),
}

/// The user's answer to a [`Gate`], sent the way the official page sends it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// 同意する on the terms page.
    AcceptTerms,
    /// 起動処理を続ける on the still-running page.
    Continue,
}

/// The link the official page follows for `answer`, and that page's address.
fn answer_url(answer: Answer, login_id: &str) -> (String, String) {
    match answer {
        Answer::AcceptTerms => (
            format!("https://p.eagate.573.jp{AGREE_PATH}?game_id={login_id}"),
            format!("https://p.eagate.573.jp{TERMS_PATH}?game_id={login_id}"),
        ),
        Answer::Continue => (format!("{GAME_LOGIN}?game_id={login_id}&refresh=true"), format!("{GAME_LOGIN}?game_id={login_id}")),
    }
}
const MAX_PAGE: usize = 512 * 1024;
const MAX_AVATAR: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AccountStatus {
    #[default]
    Unknown,
    SignedOut,
    SigningIn,
    SignedIn,
}

#[derive(Clone, Default)]
pub struct Profile {
    pub data: AccountProfile,
    pub avatar: Option<Arc<Vec<u8>>>,
}

pub struct Session {
    jar: Arc<Jar>,
    /// The HTTP client and the jar epoch it was built for.
    client: Mutex<Option<(reqwest::Client, u64)>>,
    status: Mutex<AccountStatus>,
    profile: Mutex<Option<Profile>>,
    refresh: tokio::sync::Mutex<()>,
    /// Bumped by every sign-out. Work started on an older generation must not
    /// write into the session that replaced it.
    generation: AtomicU64,
    /// Makes adopting cookies and signing out mutually exclusive.
    lifecycle: Mutex<()>,
    /// The page the last Play stopped at, for the window to show.
    gate: Mutex<Option<(GameId, Gate)>>,
    /// The answer that page offered. An answer is sent only if offered.
    offered: Mutex<Option<(GameId, Answer)>>,
    /// Per-title play data, cleared whenever the sign-in changes.
    pub playdata: crate::playdata::Cache,
}

impl Default for Session {
    fn default() -> Self {
        Self::new(std::env::temp_dir().join("konaste-hub-session"))
    }
}

impl Session {
    pub fn new(file: std::path::PathBuf) -> Self {
        Self {
            jar: Arc::new(Jar::new(file)),
            client: Mutex::new(None),
            status: Mutex::new(AccountStatus::Unknown),
            profile: Mutex::new(None),
            refresh: tokio::sync::Mutex::new(()),
            generation: AtomicU64::new(0),
            lifecycle: Mutex::new(()),
            gate: Mutex::new(None),
            offered: Mutex::new(None),
            playdata: crate::playdata::Cache::default(),
        }
    }

    pub fn status(&self) -> AccountStatus {
        *self.status.lock().unwrap()
    }

    /// The current sign-in generation. A read that started on an older one
    /// must not publish its result.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    fn current(&self, generation: u64) -> bool {
        self.generation() == generation
    }

    fn for_current<T>(&self, generation: u64, apply: impl FnOnce() -> T) -> Result<T, HubError> {
        let _guard = self.lifecycle.lock().unwrap();
        if !self.current(generation) {
            return Err(HubError { code: "signInRequired" });
        }
        Ok(apply())
    }

    pub fn profile(&self) -> Option<Profile> {
        self.profile.lock().unwrap().clone()
    }

    pub fn load(&self, remember: bool) {
        self.jar.load(remember);
        if self.jar.is_empty() {
            *self.status.lock().unwrap() = AccountStatus::SignedOut;
        }
    }

    pub fn set_remember(&self, remember: bool) {
        self.jar.set_persist(remember);
    }

    pub fn set_signing_in(&self, active: bool) {
        if active {
            // A new attempt: fence every read still in flight from the session
            // before it, so its late answer cannot land on this sign-in.
            let _guard = self.lifecycle.lock().unwrap();
            self.generation.fetch_add(1, Ordering::AcqRel);
            *self.status.lock().unwrap() = AccountStatus::SigningIn;
            return;
        }
        if self.status() != AccountStatus::SigningIn {
            return;
        }
        // Cancelling is a sign-out from the half-finished attempt's point of
        // view: it bumps the generation and clears the jar, so an adoption
        // still in flight cannot put the account back.
        let _guard = self.lifecycle.lock().unwrap();
        if self.status() != AccountStatus::SigningIn {
            return;
        }
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.jar.clear();
        *self.status.lock().unwrap() = AccountStatus::SignedOut;
    }

    fn client(&self) -> Result<reqwest::Client, HubError> {
        // A client is bound to the jar generation it was built for. After a
        // sign-out its cookie store is fenced, so the next call builds a fresh
        // client and no late answer can write into the cleared jar.
        let mut client = self.client.lock().unwrap();
        if let Some((client, _)) = client.as_ref().filter(|(_, epoch)| self.jar.same_epoch(*epoch)) {
            return Ok(client.clone());
        }
        let epoch = self.jar.epoch();
        let built = reqwest::Client::builder()
            .https_only(true)
            .cookie_provider(Arc::new(crate::jar::Fenced { jar: Arc::clone(&self.jar), epoch }))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                let allowed = attempt.url().scheme() == "https"
                    && matches!(attempt.url().host_str(), Some("p.eagate.573.jp"));
                if !allowed || attempt.previous().len() > 5 { attempt.stop() } else { attempt.follow() }
            }))
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36 KonasteHub/0.3")
            .build()
            .map_err(|_| HubError { code: "accountUnavailable" })?;
        *client = Some((built.clone(), epoch));
        Ok(built)
    }

    /// Accept cookies from the sign-in browser and read the account.
    ///
    /// `generation` is the one the cookies were collected under. A sign-out
    /// that happened after they were handed over bumps it and wins.
    pub async fn adopt(&self, hub: &Hub, cookies: Vec<StoredCookie>, generation: u64) -> Result<(), HubError> {
        // Adoption and sign-out cannot interleave: either the cookies are in
        // place before the sign-out clears them, or the sign-out wins and this
        // adoption is abandoned. This is what stops a completed sign-in from
        // repopulating a session the user just closed.
        self.for_current(generation, || {
            self.jar.replace(cookies);
            self.playdata.clear();
        })?;
        let result = self.refresh_for(hub, generation).await;
        if !self.current(generation) {
            return Err(HubError { code: "signInRequired" });
        }
        // The browser saw a signed-in page but the portal didn't answer here.
        // Don't stay on "Signing in": Play fetches a token and that decides.
        if result.is_err() {
            self.for_current(generation, || {
                let mut status = self.status.lock().unwrap();
                if *status == AccountStatus::SigningIn && !self.jar.is_empty() {
                    *status = AccountStatus::SignedIn;
                }
            })?;
            hub.publish();
        }
        result
    }

    /// The page the last Play for `id` stopped at, once.
    pub fn take_gate(&self, id: GameId) -> Option<Gate> {
        let mut gate = self.gate.lock().unwrap();
        if gate.as_ref().is_some_and(|(at, _)| *at == id) { gate.take().map(|(_, gate)| gate) } else { None }
    }

    pub fn sign_out(&self) {
        // Waits for any adoption to place its cookies; the generation bump and
        // the clear below then apply to the result.
        let _guard = self.lifecycle.lock().unwrap();
        self.clear_locked();
    }

    fn clear_locked(&self) {
        // Bumping the generation first fences every in-flight read: once it
        // completes it finds a newer generation and drops its result.
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.jar.clear();
        *self.gate.lock().unwrap() = None;
        *self.offered.lock().unwrap() = None;
        *self.status.lock().unwrap() = AccountStatus::SignedOut;
        *self.profile.lock().unwrap() = None;
        self.playdata.clear();
    }

    /// Read one signed-in page with the same client and error mapping as
    /// `refresh`. Used only for read-only play data pages.
    pub(crate) async fn page(&self, url: &str) -> Result<(url::Url, String), HubError> {
        let generation = self.generation();
        if self.status() != AccountStatus::SignedIn {
            return Err(HubError { code: "signInRequired" });
        }
        let result = self.get(url, generation).await;
        // A sign-out in between must not hand a stale page to the caller.
        if !self.current(generation) {
            return Err(HubError { code: "signInRequired" });
        }
        result
    }

    async fn get(&self, url: &str, generation: u64) -> Result<(url::Url, String), HubError> {
        self.get_from(url, None, generation).await
    }

    /// `referer` is the official page whose link this request follows.
    async fn get_from(&self, url: &str, referer: Option<&str>, generation: u64) -> Result<(url::Url, String), HubError> {
        let mut request = self.for_current(generation, || self.client())??.get(url);
        if let Some(referer) = referer {
            request = request.header(reqwest::header::REFERER, referer);
        }
        let response = request.send().await.map_err(|_| HubError { code: "accountUnavailable" })?;
        let final_url = response.url().clone();
        if response.status().as_u16() == 503 {
            return Err(HubError { code: "maintenance" });
        }
        if !response.status().is_success() {
            return Err(HubError { code: "accountUnavailable" });
        }
        let bytes = response.bytes().await.map_err(|_| HubError { code: "accountUnavailable" })?;
        if bytes.len() > MAX_PAGE {
            return Err(HubError { code: "accountUnavailable" });
        }
        Ok((final_url, String::from_utf8_lossy(&bytes).into_owned()))
    }

    /// Read My Page and the pass menu. A signed-out page clears the session.
    pub async fn refresh(&self, hub: &Hub) -> Result<(), HubError> {
        self.refresh_for(hub, self.generation()).await
    }

    /// `refresh` for one sign-in generation. A result that arrives after the
    /// session moved on (sign-out, a new sign-in) is discarded.
    async fn refresh_for(&self, hub: &Hub, generation: u64) -> Result<(), HubError> {
        let _guard = self.refresh.lock().await;
        let empty = self.for_current(generation, || {
            let empty = self.jar.is_empty();
            if empty {
                *self.status.lock().unwrap() = AccountStatus::SignedOut;
            }
            empty
        })?;
        if empty {
            hub.publish();
            return Ok(());
        }
        let (mut mypage, mut menu) = tokio::join!(self.get(MYPAGE, generation), self.get(PASS_MENU, generation));
        let (mut signed_in, mut signed_out) = read_pages(&[&mypage, &menu], &mut Collector::default());
        if !signed_in && (signed_out || mypage.is_err() || menu.is_err()) {
            // One signed-out or failed answer is not enough to drop a session:
            // the portal occasionally serves a bare page or times out. Ask once more.
            tokio::time::sleep(Duration::from_millis(800)).await;
            (mypage, menu) = tokio::join!(self.get(MYPAGE, generation), self.get(PASS_MENU, generation));
            (signed_in, signed_out) = read_pages(&[&mypage, &menu], &mut Collector::default());
        }
        // The pages may have taken a while; a sign-out in between wins.
        if !self.current(generation) {
            return Err(HubError { code: "signInRequired" });
        }
        let mut collector = Collector::default();
        read_pages(&[&mypage, &menu], &mut collector);
        if !signed_in {
            if signed_out {
                let was_signed_in = self.for_current(generation, || {
                    let was_signed_in = self.status() == AccountStatus::SignedIn;
                    self.clear_locked();
                    was_signed_in
                })?;
                hub.publish();
                if was_signed_in {
                    hub.notify(crate::hub::Notice::SignedOut);
                }
                return Ok(());
            }
            // No answer either way (network, maintenance). Keep the saved session
            // usable: Play fetches a fresh token and that request decides.
            self.for_current(generation, || {
                let mut status = self.status.lock().unwrap();
                if *status == AccountStatus::Unknown {
                    *status = AccountStatus::SignedIn;
                }
            })?;
            hub.publish();
            return Err(mypage.err().or(menu.err()).unwrap_or(HubError { code: "accountUnavailable" }));
        }
        let mut data = collector.finish();
        // Optional extras: a failure leaves the field empty, not the session.
        let (templates, list) = tokio::join!(self.get(HEADER_TEMPLATES, generation), async {
            match &data.card_number {
                Some(_) => Some(self.get(PASS_LIST, generation).await),
                None => None,
            }
        });
        let on_portal = |url: &url::Url| url.host_str() == Some("p.eagate.573.jp") && !url.path().starts_with("/gate/p/login");
        data.konami_id = templates.ok().filter(|(url, _)| on_portal(url)).and_then(|(_, body)| crate::profile::konami_id(&body));
        data.card_memo = match (list, &data.card_number) {
            (Some(Ok((url, body))), Some(card)) if on_portal(&url) => crate::profile::card_memo(&body, card),
            _ => None,
        };
        let avatar = match &data.avatar_url {
            Some(url) => self.avatar(url, generation).await,
            None => None,
        };
        self.for_current(generation, || {
            *self.profile.lock().unwrap() = Some(Profile { data, avatar });
            *self.status.lock().unwrap() = AccountStatus::SignedIn;
        })?;
        hub.publish();
        Ok(())
    }

    async fn avatar(&self, url: &str, generation: u64) -> Option<Arc<Vec<u8>>> {
        let response = self.for_current(generation, || self.client()).ok()?.ok()?.get(url).send().await.ok()?;
        let kind = response.headers().get(reqwest::header::CONTENT_TYPE)?.to_str().ok()?.to_ascii_lowercase();
        if !["image/png", "image/jpeg", "image/gif", "image/webp"].iter().any(|t| kind.starts_with(t)) {
            return None;
        }
        let bytes = response.bytes().await.ok()?;
        (bytes.len() <= MAX_AVATAR).then(|| Arc::new(bytes.to_vec()))
    }

    /// A fresh launch token for one Play attempt.
    ///
    /// The official game login page is server-rendered. Its start button carries
    /// `gamBootAndHref('<scheme>://login?tk=…', 'boot.html')`. When it asks
    /// something first, Play stops with `officialPage` and [`Self::take_gate`]
    /// has the question. `answer` is the user's reply to that question. It is
    /// sent only if that page offered it, by the same link the page has.
    pub async fn launch_token(&self, hub: &Hub, id: GameId, answer: Option<Answer>) -> Result<Zeroizing<String>, HubError> {
        let generation = self.generation();
        if self.status() != AccountStatus::SignedIn {
            return Err(HubError { code: "signInRequired" });
        }
        let game = descriptor(id);
        let login_id = game.login_id.ok_or(HubError { code: "unsupportedTitle" })?;
        let scheme = game.scheme.ok_or(HubError { code: "unsupportedTitle" })?;
        let offered = self.for_current(generation, || {
            *self.gate.lock().unwrap() = None;
            self.offered.lock().unwrap().take()
        })?;
        let (url, referer) = match answer {
            None => (format!("{GAME_LOGIN}?game_id={login_id}"), None),
            Some(answer) if offered == Some((id, answer)) => {
                crate::log::line(format!("token {login_id}: answered {answer:?}"));
                let (url, referer) = answer_url(answer, login_id);
                (url, Some(referer))
            }
            Some(_) => return Err(HubError { code: "launchVerificationFailed" }),
        };
        let (final_url, html) = match self.get_from(&url, referer.as_deref(), generation).await {
            Ok(page) => page,
            Err(error) => {
                if error.code == "maintenance" {
                    hub.state().maintenance.confirm_503();
                    hub.publish();
                }
                return Err(error);
            }
        };
        // A sign-out while the page was loading wins over the token it held.
        if !self.current(generation) {
            return Err(HubError { code: "signInRequired" });
        }
        let path = final_url.path().to_owned();
        if final_url.host_str() != Some("p.eagate.573.jp") || path.starts_with("/gate/p/login") {
            // Possibly ended between the last refresh and Play. `refresh` confirms
            // with two reads before it drops the session.
            crate::log::line(format!("token {login_id}: redirected to {path}"));
            let _ = self.refresh_for(hub, generation).await;
            return Err(HubError {
                code: if self.status() == AccountStatus::SignedIn { "launchVerificationFailed" } else { "signInRequired" },
            });
        }
        let gate = if path == TERMS_PATH {
            read_terms(&html, login_id).map_or(Gate::Unknown, Gate::Terms)
        } else if path == GAME_LOGIN_PATH {
            if let Some(token) = token_from_page(&html, scheme) {
                return Ok(token);
            }
            if login_flag(&html) == LoginFlag::SignedOut {
                let _ = self.refresh_for(hub, generation).await;
                return Err(HubError {
                    code: if self.status() == AccountStatus::SignedIn { "launchVerificationFailed" } else { "signInRequired" },
                });
            }
            if offers_continue(&html, login_id) { Gate::StillRunning } else { Gate::Unknown }
        } else {
            Gate::Unknown
        };
        crate::log::line(format!(
            "token {login_id}: {} ({path}, {} bytes)",
            match gate {
                Gate::Terms(_) => "terms of service",
                Gate::StillRunning => "previous game may be running",
                Gate::Unknown => "unknown page",
            },
            html.len(),
        ));
        let offer = match gate {
            Gate::Terms(_) => Some(Answer::AcceptTerms),
            Gate::StillRunning => Some(Answer::Continue),
            Gate::Unknown => None,
        };
        self.for_current(generation, || {
            *self.offered.lock().unwrap() = offer.map(|answer| (id, answer));
            *self.gate.lock().unwrap() = Some((id, gate));
        })?;
        Err(HubError { code: "officialPage" })
    }
}

/// The page offers 起動処理を続ける: exactly one link to this title's login page
/// with `refresh=true`.
fn offers_continue(html: &str, login_id: &str) -> bool {
    count_links(html, &format!("{GAME_LOGIN_PATH}?game_id={login_id}&refresh=true")) == 1
}

fn count_links(html: &str, path_and_query: &str) -> usize {
    let document = scraper::Html::parse_document(html);
    let selector = scraper::Selector::parse("a[href]").expect("fixed selector");
    let absolute = format!("https://p.eagate.573.jp{path_and_query}");
    document
        .select(&selector)
        .take(512)
        .filter(|a| a.value().attr("href").is_some_and(|href| href == path_and_query || href == absolute))
        .count()
}

const MAX_TERMS_TEXT: usize = 128 * 1024;

/// The terms text as plain text blocks. Only a page with exactly one terms
/// body and exactly one 同意する link for this title is read. No markup or
/// links are kept.
fn read_terms(html: &str, login_id: &str) -> Option<Vec<TermsBlock>> {
    if count_links(html, &format!("{AGREE_PATH}?game_id={login_id}")) != 1 {
        return None;
    }
    let document = scraper::Html::parse_document(html);
    let selector = scraper::Selector::parse("div.tos").expect("fixed selector");
    let mut found = document.select(&selector);
    let tos = found.next()?;
    if found.next().is_some() {
        return None;
    }
    let collapse = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut blocks = Vec::new();
    let mut line = String::new();
    let mut size = 0usize;
    let join = |line: &mut String, text: &str| {
        if text.is_empty() {
            return;
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(text);
    };
    let flush = |line: &mut String, blocks: &mut Vec<TermsBlock>| {
        if !line.is_empty() {
            blocks.push(TermsBlock::Text(std::mem::take(line)));
        }
    };
    for child in tos.children() {
        if let Some(text) = child.value().as_text() {
            size += text.len();
            join(&mut line, &collapse(text));
        } else if let Some(element) = scraper::ElementRef::wrap(child) {
            let text = collapse(&element.text().collect::<String>());
            size += text.len();
            let is = |name: &str| element.value().classes().any(|c| c == name);
            match element.value().name() {
                "br" => flush(&mut line, &mut blocks),
                "script" | "style" => {}
                // The page's own caption; the dialog has a title.
                "p" if element.value().id() == Some("caption") => {}
                "div" | "p" => {
                    flush(&mut line, &mut blocks);
                    if !text.is_empty() {
                        blocks.push(if is("tos_midashi13") {
                            TermsBlock::Heading(text)
                        } else if is("indent2") {
                            TermsBlock::Item(text)
                        } else {
                            TermsBlock::Text(text)
                        });
                    }
                }
                _ => join(&mut line, &text),
            }
        }
        if size > MAX_TERMS_TEXT || blocks.len() > 2000 {
            return None;
        }
    }
    flush(&mut line, &mut blocks);
    blocks.iter().any(|b| matches!(b, TermsBlock::Heading(_))).then_some(blocks)
}

/// `(signed_in, signed_out)` across My Page and the pass menu. Pages that say
/// signed in feed `collector`.
fn read_pages(pages: &[&Result<(url::Url, String), HubError>], collector: &mut Collector) -> (bool, bool) {
    let mut signed_in = false;
    let mut signed_out = false;
    for page in pages {
        let Ok((url, html)) = page else { continue };
        // My Page redirects to the sign-in path when the session ended.
        if url.path().starts_with("/gate/p/login") || url.host_str() != Some("p.eagate.573.jp") {
            signed_out = true;
            continue;
        }
        match login_flag(html) {
            LoginFlag::SignedIn { .. } => {
                signed_in = true;
                collector.read(html, url);
            }
            LoginFlag::SignedOut => signed_out = true,
            LoginFlag::Unknown => {}
        }
    }
    (signed_in, signed_out)
}

/// Exactly one visible start button with a well-formed callback for this game.
pub fn token_from_page(html: &str, scheme: &str) -> Option<Zeroizing<String>> {
    let document = scraper::Html::parse_document(html);
    let selector = scraper::Selector::parse("button.link-btn[onclick]").expect("fixed selector");
    let mut found = Vec::new();
    for button in document.select(&selector).take(64) {
        let element = button.value();
        if element.attr("disabled").is_some()
            || element.attr("hidden").is_some()
            || element.attr("aria-disabled").is_some_and(|v| v.eq_ignore_ascii_case("true"))
            || element.attr("style").is_some_and(|s| {
                let s = s.replace(' ', "").to_ascii_lowercase();
                s.contains("display:none") || s.contains("visibility:hidden")
            })
        {
            continue;
        }
        let Some(handler) = element.attr("onclick") else { continue };
        if handler.len() > 1024 {
            continue;
        }
        if let Some(token) = parse_handler(handler, scheme) {
            found.push(token);
        }
    }
    if found.len() == 1 { found.pop() } else { None }
}

fn parse_handler(handler: &str, scheme: &str) -> Option<Zeroizing<String>> {
    let body = handler.trim().trim_end_matches(';').trim();
    let args = body.strip_prefix("gamBootAndHref")?.trim_start().strip_prefix('(')?.strip_suffix(')')?;
    let (first, second) = args.split_once(',')?;
    let unquote = |text: &str| -> Option<String> {
        let text = text.trim();
        let quote = text.chars().next()?;
        if !matches!(quote, '"' | '\'') {
            return None;
        }
        let inner = text.strip_prefix(quote)?.strip_suffix(quote)?;
        (!inner.contains(['"', '\'', '\\', '\r', '\n'])).then(|| inner.to_owned())
    };
    if unquote(second)? != "boot.html" {
        return None;
    }
    token_from_callback(&unquote(first)?, scheme)
}

/// The token in one well-formed `<scheme>://login?tk=…` callback URL.
pub fn token_from_callback(text: &str, scheme: &str) -> Option<Zeroizing<String>> {
    let callback = url::Url::parse(text).ok()?;
    if callback.scheme() != scheme
        || callback.host_str() != Some("login")
        || !callback.username().is_empty()
        || callback.password().is_some()
        || callback.port().is_some()
        || !["", "/"].contains(&callback.path())
        || callback.fragment().is_some()
    {
        return None;
    }
    let pairs: Vec<_> = callback.query_pairs().collect();
    if pairs.len() != 1 || pairs[0].0 != "tk" {
        return None;
    }
    let token = pairs[0].1.as_ref();
    let valid = !token.is_empty() && token.len() <= 64 && token.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-+/=".contains(&b));
    valid.then(|| Zeroizing::new(token.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TERMS: &str = r#"<html><body><div id="id_ea_common_content">
  <div class="tos"><p id="caption">コナステ 利用規約</p>
<div class="tos_midashi13">第１条（本規約）</div>
本規約は、条件を定めるものです。<br><br>
<div class="tos_midashi13">第２条（定義）</div>
本規約において、次の各号に掲げる用語の意義は、
<div class="indent2">(1) 「e-amusement pass等」とは、媒体をいいます。</div>
<div class="clear"></div>
KONAMI ID利用規約 ： <a href="https://my.konami.net/agreement.html" target="_new">https://my.konami.net/agreement.html</a><br><br>
2018年7月25日 制定<br>
</div>
  <div class="center"><p><a href="/game/konasteapp/API/page/terms_of_service/agree.html?game_id=ddr">同意する</a> / <a href="/game/konasteapp/API/page/terms_of_service/not_agree.html?game_id=ddr">同意しない</a></p></div>
</div></body></html>"#;

    #[test]
    fn the_terms_page_is_read_as_plain_text() {
        assert_eq!(
            read_terms(TERMS, "ddr").unwrap(),
            vec![
                TermsBlock::Heading("第１条（本規約）".into()),
                TermsBlock::Text("本規約は、条件を定めるものです。".into()),
                TermsBlock::Heading("第２条（定義）".into()),
                TermsBlock::Text("本規約において、次の各号に掲げる用語の意義は、".into()),
                TermsBlock::Item("(1) 「e-amusement pass等」とは、媒体をいいます。".into()),
                TermsBlock::Text("KONAMI ID利用規約 ： https://my.konami.net/agreement.html".into()),
                TermsBlock::Text("2018年7月25日 制定".into()),
            ]
        );
        // The agree link must be this title's, and there must be one.
        assert!(read_terms(TERMS, "popn-music").is_none());
        assert!(read_terms(&TERMS.replace("agree.html?game_id=ddr\"", "x\""), "ddr").is_none());
    }

    #[test]
    fn the_still_running_page_offers_continue_for_this_title_only() {
        let page = r#"<font color="red"><p>前のゲームが起動中の可能性があります。</p></font>
            <a href="/game/konasteapp/API/login/login.html?game_id=ddr&refresh=true" class="link-btn">起動処理を続ける</a>
            <button class="link-btn" onclick="closeWindowAsap()">キャンセルしてページを閉じる</button>"#;
        assert!(offers_continue(page, "ddr"));
        assert!(offers_continue(&page.replace("&refresh", "&amp;refresh"), "ddr"));
        assert!(!offers_continue(page, "sdvx"));
        assert!(token_from_page(page, "konaste.ddr").is_none());
        assert_eq!(
            answer_url(Answer::Continue, "ddr").0,
            "https://p.eagate.573.jp/game/konasteapp/API/login/login.html?game_id=ddr&refresh=true"
        );
        assert_eq!(
            answer_url(Answer::AcceptTerms, "ddr").0,
            "https://p.eagate.573.jp/game/konasteapp/API/page/terms_of_service/agree.html?game_id=ddr"
        );
    }

    #[test]
    fn exactly_one_well_formed_start_button_yields_a_token() {
        let page = r#"<button class="link-btn" onclick="gamBootAndHref('konaste.sdvx://login?tk=AbC_12-3', 'boot.html');">Start</button>"#;
        assert_eq!(token_from_page(page, "konaste.sdvx").unwrap().as_str(), "AbC_12-3");
        assert!(token_from_page(page, "konaste.bomber-girl").is_none());
        let two = format!("{page}{page}");
        assert!(token_from_page(&two, "konaste.sdvx").is_none());
        for bad in [
            r#"<button class="link-btn" disabled onclick="gamBootAndHref('konaste.sdvx://login?tk=x', 'boot.html')">"#,
            r#"<button class="link-btn" style="display: none" onclick="gamBootAndHref('konaste.sdvx://login?tk=x', 'boot.html')">"#,
            r#"<button class="link-btn" onclick="gamBootAndHref('konaste.sdvx://login?tk=x&tk=y', 'boot.html')">"#,
            r#"<button class="link-btn" onclick="gamBootAndHref('konaste.sdvx://evil?tk=x', 'boot.html')">"#,
            r#"<button class="link-btn" onclick="gamBootAndHref('konaste.sdvx://login?tk=x', 'other.html')">"#,
            r#"<button class="link-btn" onclick="alert(1);gamBootAndHref('konaste.sdvx://login?tk=x', 'boot.html')">"#,
        ] {
            assert!(token_from_page(bad, "konaste.sdvx").is_none(), "{bad}");
        }
    }

    fn session() -> (Session, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let session = Session::new(dir.path().join("session.dat"));
        (session, dir)
    }

    #[test]
    fn sign_out_fences_a_read_started_before_it() {
        let (session, _dir) = session();
        let started = session.generation();
        assert!(session.current(started));
        // A refresh captured this generation before any pages were read.
        session.sign_out();
        assert_eq!(
            session
                .for_current(started, || {
                    *session.profile.lock().unwrap() = Some(Profile::default());
                    *session.status.lock().unwrap() = AccountStatus::SignedIn;
                })
                .unwrap_err()
                .code,
            "signInRequired"
        );
        assert_eq!(session.status(), AccountStatus::SignedOut);
        assert!(session.profile().is_none());
    }

    #[test]
    fn a_newer_sign_in_does_not_accept_an_older_read() {
        let (session, _dir) = session();
        let first = session.generation();
        session.sign_out();
        let second = session.generation();
        assert_ne!(first, second);
        assert!(session.current(second));
        assert!(!session.current(first));
    }

    #[test]
    fn cancelling_a_sign_in_fences_a_late_adoption() {
        let (session, _dir) = session();
        session.set_signing_in(true);
        let started = session.generation();
        session.set_signing_in(false);
        // Cancelling bumps the generation, so cookies handed over afterwards
        // cannot repopulate the session.
        assert!(!session.current(started));
        assert_eq!(session.status(), AccountStatus::SignedOut);
    }
}
