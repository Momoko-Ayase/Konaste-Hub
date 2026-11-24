//! Native KONAMI ID sign-in.
//!
//! The Hub draws the form itself. Each step sends the same JSON request the
//! official page at my.konami.net sends, from inside that page, which stays
//! loaded in a hidden browser (see `browser.rs`). KONAMI's own scripts and
//! cookies keep running there, so its bot check sees the page it expects.
//! When a step succeeds the page follows the returned address back to
//! e-amusement, and `browser.rs` hands the signed-in cookies to the native
//! session.
//!
//! This module only decides what to send and what an answer means. It does
//! no I/O and is tested without a browser.
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub const RP_ID: &str = "my.konami.net";
pub const ORIGIN: &str = "https://my.konami.net";
/// The e-amusement sign-in page; it forwards to my.konami.net.
pub const START: &str = "https://p.eagate.573.jp/gate/p/login.html";
pub const PASSWORD_HELP: &str = "https://my.konami.net/password-reminder/input-email-address";
/// Pictures to choose from; the sample comes in addition.
pub const PICTURES: usize = 5;
const MAX_PICTURE: usize = 256 * 1024;
/// How long the official page may take to load before giving up.
const LOAD_LIMIT: Duration = Duration::from_secs(40);
/// How long the return to e-amusement may take before the sign-in is
/// considered stuck and the user can try again.
const FINISH_LIMIT: Duration = Duration::from_secs(20);
/// A refused sign-in request is sent once more after this delay.
const RETRY_AFTER: Duration = Duration::from_secs(2);

// Error codes from my.konami.net, as its own page reads them.
const CAPTCHA_INVALID: i64 = 400_005_003;
const CREDENTIALS_INVALID: i64 = 400_008_003;
const TWO_STEP_INVALID: i64 = 400_014_003;
const CODE_INVALID: i64 = 400_061_001;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeKind {
    /// Sign-in by a code sent to the account's email address.
    Email,
    /// Two-step verification after the password.
    TwoStep,
    Sms,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Id,
    Password,
    Code(CodeKind),
    /// Waiting for the Windows passkey dialog.
    Passkey,
    /// Following the return to e-amusement.
    Finishing,
    /// my.konami.net asks for something the Hub does not draw (consent,
    /// birthday, account lock). The user finishes it on the website.
    Website,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    Credentials,
    Captcha,
    Code,
    Passkey,
    /// KONAMI turned the request down (HTTP 403, the e-amusement error page).
    Refused,
    /// No answer, or an answer the Hub cannot read.
    Unavailable,
}

/// Where a full page load in the sign-in browser ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    SignIn,
    TwoStep,
    Sms,
    Portal,
    Error,
    /// e-amusement's own sign-in pages on the way back to the portal.
    Passing,
    Other,
}

pub fn page(url: &str) -> Page {
    let Ok(url) = url::Url::parse(url) else { return Page::Other };
    if url.scheme() != "https" || url.port().is_some() {
        return Page::Other;
    }
    match url.host_str() {
        Some("p.eagate.573.jp") => {
            let path = url.path();
            if path.starts_with("/gate/k/login_error") {
                Page::Error
            } else if path.starts_with("/gate/p/login") || path.starts_with("/gate/k/login") {
                Page::Passing
            } else {
                Page::Portal
            }
        }
        Some("my.konami.net") => {
            // Pages carry a locale first: /ja/signin, /en_GB/signin/two-step-verification.
            let parts: Vec<&str> = url.path().split('/').filter(|p| !p.is_empty()).collect();
            let rest = match parts.first() {
                Some(first) if is_locale(first) => &parts[1..],
                _ => &parts[..],
            };
            match rest {
                ["signin"] => Page::SignIn,
                ["signin", "two-step-verification"] => Page::TwoStep,
                ["signin", "sms-verifications"] => Page::Sms,
                _ => Page::Other,
            }
        }
        _ => Page::Other,
    }
}

fn is_locale(part: &str) -> bool {
    let bytes = part.as_bytes();
    match bytes.len() {
        2 => bytes.iter().all(u8::is_ascii_lowercase),
        5 => bytes[..2].iter().all(u8::is_ascii_lowercase) && bytes[2] == b'_' && bytes[3..].iter().all(u8::is_ascii_uppercase),
        _ => false,
    }
}

/// One request for the sign-in page to send. `path` is on my.konami.net.
#[derive(Clone)]
pub struct Call {
    pub id: u64,
    pub method: &'static str,
    pub path: String,
    /// JSON text. May hold the password.
    pub body: Option<Zeroizing<String>>,
    /// Answer wanted as base64 (pictures).
    pub binary: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    AuthType,
    Captcha(u32),
    Picture { generation: u32, index: usize },
    Login { by_code: bool },
    SendCode,
    TwoStep,
    Sms,
    ResendSms,
    PasskeyOptions,
    Passkey,
}

#[derive(Default)]
pub struct Captcha {
    pub generation: u32,
    /// The sample first, then the pictures.
    tokens: Vec<String>,
    images: Vec<Option<Arc<Vec<u8>>>>,
    pub selected: [bool; PICTURES],
}

impl Captcha {
    pub fn sample(&self) -> Option<&Arc<Vec<u8>>> {
        self.images.first().and_then(Option::as_ref)
    }

    pub fn picture(&self, index: usize) -> Option<&Arc<Vec<u8>>> {
        self.images.get(index + 1).and_then(Option::as_ref)
    }

    pub fn ready(&self) -> bool {
        self.images.len() == PICTURES + 1 && self.images.iter().all(Option::is_some)
    }

    fn answers(&self) -> String {
        self.selected.iter().map(|on| if *on { '1' } else { '0' }).collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasskeyFailure {
    Cancelled,
    Failed,
}

pub struct Flow {
    step: Step,
    login_id: String,
    problem: Option<Problem>,
    busy: bool,
    ready: bool,
    started: Instant,
    /// Two-step verification: don't ask again on this device (the official default).
    pub trust_device: bool,
    /// The account also accepts a passkey (offered on the code step).
    passkey_account: bool,
    captcha: Captcha,
    next: u64,
    pending: HashMap<u64, Pending>,
    /// Requests made before the page finished loading.
    waiting: Vec<Call>,
    outbox: Vec<Call>,
    navigate: Option<String>,
    ceremony: Option<String>,
    /// The last sign-in request, for one retry after an HTTP 403.
    last: Option<Call>,
    retry_at: Option<Instant>,
    retried: bool,
    /// When the return to e-amusement started, to tell a slow return from a stuck one.
    finishing_since: Option<Instant>,
    changed: bool,
}

impl Flow {
    pub fn new(now: Instant) -> Self {
        Self {
            step: Step::Id,
            login_id: String::new(),
            problem: None,
            busy: false,
            ready: false,
            started: now,
            trust_device: true,
            passkey_account: false,
            captcha: Captcha::default(),
            next: 1,
            pending: HashMap::new(),
            waiting: Vec::new(),
            outbox: Vec::new(),
            navigate: None,
            ceremony: None,
            last: None,
            retry_at: None,
            retried: false,
            finishing_since: None,
            changed: true,
        }
    }

    pub fn step(&self) -> Step {
        self.step
    }
    pub fn login_id(&self) -> &str {
        &self.login_id
    }
    pub fn problem(&self) -> Option<Problem> {
        self.problem
    }
    pub fn busy(&self) -> bool {
        self.busy
    }
    pub fn captcha(&self) -> &Captcha {
        &self.captcha
    }
    pub fn passkey_account(&self) -> bool {
        self.passkey_account
    }

    /// True once after anything visible changed.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }
    /// Requests to send now.
    pub fn take_calls(&mut self) -> Vec<Call> {
        std::mem::take(&mut self.outbox)
    }
    /// An address the sign-in page should load.
    pub fn take_navigation(&mut self) -> Option<String> {
        self.navigate.take()
    }
    /// Passkey request options (JSON) for the Windows dialog.
    pub fn take_ceremony(&mut self) -> Option<String> {
        self.ceremony.take()
    }

    fn send(&mut self, pending: Pending, method: &'static str, path: String, body: Option<Zeroizing<String>>, binary: bool) {
        let id = self.next;
        self.next += 1;
        self.pending.insert(id, pending);
        let call = Call { id, method, path, body, binary };
        if matches!(pending, Pending::Login { .. }) {
            self.last = Some(call.clone());
        }
        if self.ready { self.outbox.push(call) } else { self.waiting.push(call) }
    }

    fn fail(&mut self, problem: Problem) {
        self.problem = Some(problem);
        self.busy = false;
        self.changed = true;
    }

    /// Drop answers still on their way; they belong to a step left behind.
    fn forget(&mut self) {
        self.pending.clear();
        self.waiting.clear();
        self.ceremony = None;
        self.last = None;
        self.retry_at = None;
        self.finishing_since = None;
    }

    /// Back to the first step, keeping the typed ID.
    pub fn back(&mut self) {
        self.forget();
        self.step = Step::Id;
        self.problem = None;
        self.busy = false;
        self.changed = true;
    }

    pub fn submit_id(&mut self, id: &str) {
        let id: String = id.chars().filter(|c| !c.is_whitespace()).collect();
        if id.is_empty() || self.busy || self.step != Step::Id {
            return;
        }
        self.login_id = id;
        self.problem = None;
        self.busy = true;
        self.changed = true;
        let body = format!(r#"{{"id":{}}}"#, json_string(&self.login_id));
        self.send(Pending::AuthType, "POST", "/api/auths/login/authTypes".into(), Some(Zeroizing::new(body)), false);
    }

    /// A new set of pictures.
    pub fn load_captcha(&mut self) {
        self.captcha.generation += 1;
        self.captcha.tokens.clear();
        self.captcha.images.clear();
        self.captcha.selected = [false; PICTURES];
        self.changed = true;
        let generation = self.captcha.generation;
        self.send(Pending::Captcha(generation), "GET", "/api/captchas".into(), None, false);
    }

    pub fn toggle(&mut self, index: usize) {
        if let Some(on) = self.captcha.selected.get_mut(index) {
            *on = !*on;
            self.changed = true;
        }
    }

    pub fn submit_password(&mut self, password: &str) {
        if password.is_empty() || self.busy || self.step != Step::Password || !self.captcha.ready() {
            return;
        }
        self.problem = None;
        self.busy = true;
        self.changed = true;
        let password = Zeroizing::new(json_string(password));
        let body = Zeroizing::new(format!(
            r#"{{"id":{},"password":{},"captchaAnswers":"{}"}}"#,
            json_string(&self.login_id),
            password.as_str(),
            self.captcha.answers()
        ));
        self.retried = false;
        self.send(Pending::Login { by_code: false }, "POST", "/api/logins".into(), Some(body), false);
    }

    fn ask_email_code(&mut self) {
        self.step = Step::Code(CodeKind::Email);
        self.busy = true;
        self.changed = true;
        self.send_code();
    }

    fn send_code(&mut self) {
        let body = format!(r#"{{"id":{}}}"#, json_string(&self.login_id));
        self.send(Pending::SendCode, "POST", "/api/auths/login/codes".into(), Some(Zeroizing::new(body)), false);
    }

    pub fn submit_code(&mut self, code: &str) {
        let Step::Code(kind) = self.step else { return };
        if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) || self.busy {
            return;
        }
        self.problem = None;
        self.busy = true;
        self.changed = true;
        match kind {
            CodeKind::Email => {
                let body = format!(r#"{{"id":{},"captchaAnswers":"","code":"{code}"}}"#, json_string(&self.login_id));
                self.retried = false;
                self.send(Pending::Login { by_code: true }, "POST", "/api/logins".into(), Some(Zeroizing::new(body)), false);
            }
            CodeKind::TwoStep => {
                let path = format!("/api/auths/twoStepVerification?code={code}&isSkipped={}", self.trust_device);
                self.send(Pending::TwoStep, "GET", path, None, false);
            }
            CodeKind::Sms => self.send(Pending::Sms, "GET", format!("/api/auths/smsCodeVerification/codes/{code}"), None, false),
        }
    }

    pub fn resend_code(&mut self) {
        match self.step {
            Step::Code(CodeKind::Sms) => self.send(Pending::ResendSms, "POST", "/api/auths/smsCodeVerification/codes".into(), None, false),
            Step::Code(CodeKind::Email) => self.send_code(),
            _ => {}
        }
    }

    /// Sign in with a passkey. `id` is what the ID field holds; it may be
    /// empty, and the Windows dialog then lists every passkey for the site.
    pub fn start_passkey(&mut self, id: &str) {
        if matches!(self.step, Step::Passkey | Step::Finishing) {
            return;
        }
        self.forget();
        let id: String = id.chars().filter(|c| !c.is_whitespace()).collect();
        if !id.is_empty() {
            self.login_id = id.clone();
        }
        self.step = Step::Passkey;
        self.problem = None;
        self.busy = true;
        self.changed = true;
        let body = format!(r#"{{"loginId":{}}}"#, json_string(&id));
        self.send(Pending::PasskeyOptions, "POST", "/api/tokens/passkeyAuthentication".into(), Some(Zeroizing::new(body)), false);
    }

    /// The Windows dialog finished. `Ok` is the assertion as JSON.
    pub fn on_passkey(&mut self, result: Result<String, PasskeyFailure>) {
        if self.step != Step::Passkey {
            return;
        }
        match result {
            Ok(response) => {
                let body = format!(r#"{{"authenticationResponse":{}}}"#, json_string(&response));
                self.send(Pending::Passkey, "POST", "/api/auths/passkey".into(), Some(Zeroizing::new(body)), false);
            }
            Err(failure) => self.passkey_failed(failure == PasskeyFailure::Failed),
        }
    }

    /// A passkey account falls back to an email code; others go back to the ID.
    /// A cancel by the user is not an error.
    fn passkey_failed(&mut self, report: bool) {
        self.forget();
        self.problem = None;
        if self.passkey_account && !self.login_id.is_empty() {
            self.ask_email_code();
        } else {
            self.back();
        }
        if report {
            self.problem = Some(Problem::Passkey);
        }
    }

    /// Time passes: the load limit, the finishing limit, and the delayed retry.
    pub fn tick(&mut self, now: Instant) {
        // A return to e-amusement that never lands would otherwise leave the
        // dialog on "Signing in" forever, with no way to retry.
        if self.step == Step::Finishing && self.finishing_since.is_some_and(|at| now.duration_since(at) > FINISH_LIMIT) {
            // `back` keeps the typed ID. Load the official login page again
            // before sending any retry requests; the old page may be a portal.
            self.back();
            self.ready = false;
            self.started = now;
            self.navigate = Some(START.into());
            self.fail(Problem::Unavailable);
        }
        if !self.ready && !self.waiting.is_empty() && now.duration_since(self.started) > LOAD_LIMIT {
            self.waiting.clear();
            self.pending.clear();
            self.started = now;
            self.fail(Problem::Unavailable);
        }
        if self.retry_at.is_some_and(|at| now >= at) {
            self.retry_at = None;
            if let Some(mut call) = self.last.clone() {
                let by_code = call.body.as_ref().is_some_and(|b| b.contains("\"code\":"));
                call.id = self.next;
                self.next += 1;
                self.pending.insert(call.id, Pending::Login { by_code });
                self.outbox.push(call);
            }
        }
    }

    pub fn on_page(&mut self, page: Page, now: Instant) {
        match page {
            Page::SignIn => {
                let first = !self.ready;
                self.ready = true;
                self.outbox.append(&mut self.waiting);
                // Sent back to the form after a step: the step did not take.
                if !first && self.step == Step::Finishing {
                    self.back();
                    self.fail(Problem::Refused);
                }
            }
            Page::TwoStep | Page::Sms => {
                self.forget();
                self.step = Step::Code(if page == Page::TwoStep { CodeKind::TwoStep } else { CodeKind::Sms });
                self.problem = None;
                self.busy = false;
                self.changed = true;
            }
            Page::Portal => {
                if self.step != Step::Finishing {
                    self.step = Step::Finishing;
                    self.finishing_since = Some(now);
                    self.busy = true;
                    self.changed = true;
                }
            }
            Page::Error => {
                // The return to e-amusement failed. Start over from the official page.
                self.ready = false;
                self.started = now;
                self.back();
                self.navigate = Some(START.into());
                self.fail(Problem::Refused);
            }
            Page::Passing => {}
            Page::Other => {
                if self.step == Step::Finishing {
                    self.forget();
                    self.step = Step::Website;
                    self.busy = false;
                    self.changed = true;
                }
            }
        }
    }

    /// An answer to [`Call`] `id`. `status` 0 means no answer.
    pub fn on_reply(&mut self, id: u64, status: u16, body: &str, now: Instant) {
        let Some(pending) = self.pending.remove(&id) else { return };
        let json = serde_json::from_str::<serde_json::Value>(body).ok();
        let field = |name: &str| json.as_ref().and_then(|j| j.get(name));
        let code = field("code").and_then(serde_json::Value::as_i64);
        match pending {
            Pending::AuthType => {
                self.busy = false;
                self.changed = true;
                if status != 201 {
                    return self.fail(if status == 403 { Problem::Refused } else { Problem::Unavailable });
                }
                let kind = field("authType").and_then(|v| v.as_str()).unwrap_or("");
                self.passkey_account = kind == "PASSKEY";
                if kind == "PASSKEY" {
                    // As on the official page: the passkey first, an email code if that fails.
                    let id = self.login_id.clone();
                    self.start_passkey(&id);
                } else if kind == "CODE" {
                    self.ask_email_code();
                } else {
                    self.step = Step::Password;
                    self.load_captcha();
                }
            }
            Pending::Captcha(generation) => {
                if generation != self.captcha.generation {
                    return;
                }
                let sample = field("correctPictureUri").and_then(|v| v.as_str()).map(str::to_owned);
                let pictures: Vec<String> = field("testPictureUris")
                    .and_then(|v| v.as_array())
                    .map(|list| list.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                    .unwrap_or_default();
                let (Some(sample), true) = (sample, status == 200 && pictures.len() == PICTURES) else {
                    return self.fail(Problem::Unavailable);
                };
                self.captcha.tokens = std::iter::once(sample).chain(pictures).collect();
                self.captcha.images = vec![None; PICTURES + 1];
                for index in 0..=PICTURES {
                    let path = format!("/api/captchas/picture?token={}", query(&self.captcha.tokens[index]));
                    self.send(Pending::Picture { generation, index }, "GET", path, None, true);
                }
                self.changed = true;
            }
            Pending::Picture { generation, index } => {
                if generation != self.captcha.generation {
                    return;
                }
                use base64::Engine as _;
                let bytes = (status == 200)
                    .then(|| base64::engine::general_purpose::STANDARD.decode(body.trim()).ok())
                    .flatten()
                    .filter(|b| b.len() <= MAX_PICTURE && is_picture(b));
                match bytes {
                    Some(bytes) => {
                        self.captcha.images[index] = Some(Arc::new(bytes));
                        self.changed = true;
                    }
                    None => self.fail(Problem::Unavailable),
                }
            }
            Pending::Login { by_code } => {
                if status == 200 {
                    self.last = None;
                    return self.follow(field("redirectUri"), now);
                }
                if status == 403 && !self.retried && self.last.is_some() {
                    // Usually KONAMI's bot check, before its sensor reported. One retry.
                    self.retried = true;
                    self.retry_at = Some(now + RETRY_AFTER);
                    return;
                }
                self.last = None;
                let problem = match (status, code) {
                    (400, Some(CAPTCHA_INVALID)) => Problem::Captcha,
                    (400, Some(CREDENTIALS_INVALID)) => Problem::Credentials,
                    (400, Some(CODE_INVALID)) => Problem::Code,
                    (403, _) => Problem::Refused,
                    _ => Problem::Unavailable,
                };
                self.fail(problem);
                // The official page shows new pictures after every failed try.
                if !by_code {
                    self.load_captcha();
                }
            }
            Pending::SendCode => {
                self.busy = false;
                self.changed = true;
                if status != 201 || field("isIssued").and_then(|v| v.as_bool()) != Some(true) {
                    self.fail(Problem::Unavailable);
                }
            }
            Pending::TwoStep => match (status, code) {
                (200, _) => self.follow(field("redirectUri"), now),
                (_, Some(TWO_STEP_INVALID)) => self.fail(Problem::Code),
                _ => self.fail(Problem::Unavailable),
            },
            Pending::Sms => match status {
                200 => self.follow(field("redirectUri"), now),
                400 => self.fail(Problem::Code),
                _ => self.fail(Problem::Unavailable),
            },
            Pending::ResendSms => {
                if status != 200 {
                    self.fail(Problem::Unavailable);
                }
            }
            Pending::PasskeyOptions => match (status, field("passkeyAuthenticationRequest").and_then(|v| v.as_str())) {
                (201, Some(request)) => self.ceremony = Some(request.to_owned()),
                _ => self.passkey_failed(true),
            },
            Pending::Passkey => {
                if status == 200 {
                    return self.follow(field("redirectUri"), now);
                }
                self.passkey_failed(true);
            }
        }
    }

    /// Load the `redirectUri` from a successful answer, if it stays on the
    /// official hosts.
    fn follow(&mut self, uri: Option<&serde_json::Value>, now: Instant) {
        let target = uri
            .and_then(|v| v.as_str())
            .and_then(|uri| url::Url::parse(ORIGIN).ok()?.join(uri).ok())
            .map(String::from)
            .filter(|url| crate::browser::allowed(url));
        match target {
            Some(url) => {
                self.navigate = Some(url);
                self.step = Step::Finishing;
                // The return just started; if it never lands, `tick` times out.
                self.finishing_since = Some(now);
                self.problem = None;
                self.busy = true;
                self.changed = true;
            }
            None => self.fail(Problem::Unavailable),
        }
    }
}

/// A JSON string literal.
fn json_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into())
}

fn query(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes()).collect()
}

fn is_picture(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G']) || bytes.starts_with(&[0xFF, 0xD8]) || bytes.starts_with(b"GIF8")
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0];

    fn png64() -> String {
        base64::engine::general_purpose::STANDARD.encode(PNG)
    }

    fn ready(now: Instant) -> Flow {
        let mut flow = Flow::new(now);
        flow.on_page(Page::SignIn, now);
        flow
    }

    fn one(flow: &mut Flow) -> Call {
        let mut calls = flow.take_calls();
        assert_eq!(calls.len(), 1, "expected one request");
        calls.remove(0)
    }

    fn to_password(flow: &mut Flow, now: Instant) {
        flow.submit_id(" someone@example.test ");
        let call = one(flow);
        assert_eq!(call.path, "/api/auths/login/authTypes");
        assert_eq!(call.body.as_deref().map(String::as_str), Some(r#"{"id":"someone@example.test"}"#));
        flow.on_reply(call.id, 201, r#"{"authType":"PASSWORD"}"#, now);
        assert_eq!(flow.step(), Step::Password);
        let captcha = one(flow);
        assert_eq!(captcha.path, "/api/captchas");
        flow.on_reply(captcha.id, 200, r#"{"correctPictureUri":"S","testPictureUris":["a","b","c","d","e+"]}"#, now);
        let pictures = flow.take_calls();
        assert_eq!(pictures.len(), PICTURES + 1);
        assert!(pictures.iter().all(|c| c.binary));
        assert_eq!(pictures[5].path, "/api/captchas/picture?token=e%2B");
        for call in pictures {
            flow.on_reply(call.id, 200, &png64(), now);
        }
        assert!(flow.captcha().ready());
    }

    #[test]
    fn pages_are_recognised() {
        assert_eq!(page("https://my.konami.net/ja/signin"), Page::SignIn);
        assert_eq!(page("https://my.konami.net/en_GB/signin?x=1"), Page::SignIn);
        assert_eq!(page("https://my.konami.net/signin/two-step-verification"), Page::TwoStep);
        assert_eq!(page("https://my.konami.net/zh_CN/signin/sms-verifications"), Page::Sms);
        assert_eq!(page("https://my.konami.net/ja/signin/agreement/konami-id/terms-of-service"), Page::Other);
        assert_eq!(page("https://p.eagate.573.jp/gate/p/mypage/"), Page::Portal);
        assert_eq!(page("https://p.eagate.573.jp/gate/k/login_error.html"), Page::Error);
        assert_eq!(page("https://p.eagate.573.jp/gate/k/login_exec.html?code=x"), Page::Passing);
        assert_eq!(page("http://my.konami.net/ja/signin"), Page::Other);
        assert_eq!(page("https://my.konami.net:8443/ja/signin"), Page::Other);
    }

    #[test]
    fn requests_wait_for_the_page() {
        let now = Instant::now();
        let mut flow = Flow::new(now);
        flow.submit_id("someone");
        assert!(flow.take_calls().is_empty());
        flow.on_page(Page::SignIn, now);
        assert_eq!(one(&mut flow).path, "/api/auths/login/authTypes");
    }

    #[test]
    fn a_page_that_never_loads_fails() {
        let now = Instant::now();
        let mut flow = Flow::new(now);
        flow.submit_id("someone");
        flow.tick(now + LOAD_LIMIT + Duration::from_secs(1));
        assert_eq!(flow.problem(), Some(Problem::Unavailable));
        assert!(!flow.busy());
    }

    #[test]
    fn password_sign_in_sends_the_official_body_and_follows_the_return() {
        let now = Instant::now();
        let mut flow = ready(now);
        to_password(&mut flow, now);
        flow.toggle(1);
        flow.toggle(4);
        flow.submit_password("pa\"ss");
        let call = one(&mut flow);
        assert_eq!(call.path, "/api/logins");
        assert_eq!(
            call.body.as_deref().map(String::as_str),
            Some(r#"{"id":"someone@example.test","password":"pa\"ss","captchaAnswers":"01001"}"#)
        );
        flow.on_reply(call.id, 200, r#"{"redirectUri":"/api/authorizations?state=x"}"#, now);
        assert_eq!(flow.step(), Step::Finishing);
        assert_eq!(flow.take_navigation().as_deref(), Some("https://my.konami.net/api/authorizations?state=x"));
    }

    #[test]
    fn wrong_answers_reload_pictures() {
        let now = Instant::now();
        let mut flow = ready(now);
        to_password(&mut flow, now);
        flow.submit_password("x");
        let call = one(&mut flow);
        flow.on_reply(call.id, 400, r#"{"code":400005003}"#, now);
        assert_eq!(flow.problem(), Some(Problem::Captcha));
        assert_eq!(one(&mut flow).path, "/api/captchas");
        assert!(!flow.captcha().ready());
    }

    #[test]
    fn wrong_password_is_reported() {
        let now = Instant::now();
        let mut flow = ready(now);
        to_password(&mut flow, now);
        flow.submit_password("x");
        let call = one(&mut flow);
        flow.on_reply(call.id, 400, r#"{"code":400008003}"#, now);
        assert_eq!(flow.problem(), Some(Problem::Credentials));
        assert_eq!(flow.step(), Step::Password);
    }

    #[test]
    fn a_403_is_retried_once() {
        let now = Instant::now();
        let mut flow = ready(now);
        to_password(&mut flow, now);
        flow.submit_password("x");
        let first = one(&mut flow);
        flow.on_reply(first.id, 403, "<html>", now);
        assert!(flow.busy());
        assert!(flow.take_calls().is_empty());
        flow.tick(now + RETRY_AFTER);
        let second = one(&mut flow);
        assert_eq!(second.body.as_deref(), first.body.as_deref());
        flow.on_reply(second.id, 403, "<html>", now);
        assert_eq!(flow.problem(), Some(Problem::Refused));
    }

    #[test]
    fn a_stale_answer_is_ignored() {
        let now = Instant::now();
        let mut flow = ready(now);
        flow.submit_id("someone");
        let call = one(&mut flow);
        flow.back();
        flow.on_reply(call.id, 201, r#"{"authType":"PASSWORD"}"#, now);
        assert_eq!(flow.step(), Step::Id);
    }

    #[test]
    fn passkey_accounts_try_the_passkey_then_a_code() {
        let now = Instant::now();
        let mut flow = ready(now);
        flow.submit_id("someone");
        let call = one(&mut flow);
        flow.on_reply(call.id, 201, r#"{"authType":"PASSKEY"}"#, now);
        assert_eq!(flow.step(), Step::Passkey);
        let options = one(&mut flow);
        assert_eq!(options.body.as_deref().map(String::as_str), Some(r#"{"loginId":"someone"}"#));
        flow.on_reply(options.id, 201, r#"{"passkeyAuthenticationRequest":"{}"}"#, now);
        assert!(flow.take_ceremony().is_some());
        flow.on_passkey(Err(PasskeyFailure::Cancelled));
        assert_eq!(flow.step(), Step::Code(CodeKind::Email));
        assert_eq!(flow.problem(), None);
        assert_eq!(one(&mut flow).path, "/api/auths/login/codes");
    }

    #[test]
    fn code_accounts_get_an_email_code() {
        let now = Instant::now();
        let mut flow = ready(now);
        flow.submit_id("someone");
        let call = one(&mut flow);
        flow.on_reply(call.id, 201, r#"{"authType":"CODE"}"#, now);
        assert_eq!(flow.step(), Step::Code(CodeKind::Email));
        assert!(!flow.passkey_account());
        let send = one(&mut flow);
        assert_eq!(send.path, "/api/auths/login/codes");
        flow.on_reply(send.id, 201, r#"{"isIssued":true}"#, now);
        flow.submit_code("12345");
        assert!(flow.take_calls().is_empty(), "five digits are not sent");
        flow.submit_code("123456");
        let login = one(&mut flow);
        assert_eq!(login.body.as_deref().map(String::as_str), Some(r#"{"id":"someone","captchaAnswers":"","code":"123456"}"#));
        flow.on_reply(login.id, 400, r#"{"code":400061001}"#, now);
        assert_eq!(flow.problem(), Some(Problem::Code));
        assert!(flow.take_calls().is_empty(), "no pictures for code sign-in");
    }

    #[test]
    fn two_step_uses_the_official_query() {
        let now = Instant::now();
        let mut flow = ready(now);
        to_password(&mut flow, now);
        flow.submit_password("x");
        let call = one(&mut flow);
        flow.on_reply(call.id, 200, r#"{"redirectUri":"https://my.konami.net/ja/signin/two-step-verification"}"#, now);
        assert!(flow.take_navigation().is_some());
        flow.on_page(Page::TwoStep, now);
        assert_eq!(flow.step(), Step::Code(CodeKind::TwoStep));
        flow.trust_device = false;
        flow.submit_code("000111");
        assert_eq!(one(&mut flow).path, "/api/auths/twoStepVerification?code=000111&isSkipped=false");
    }

    #[test]
    fn returns_off_the_official_hosts_are_not_followed() {
        let now = Instant::now();
        let mut flow = ready(now);
        to_password(&mut flow, now);
        flow.submit_password("x");
        let call = one(&mut flow);
        flow.on_reply(call.id, 200, r#"{"redirectUri":"https://evil.test/"}"#, now);
        assert_eq!(flow.take_navigation(), None);
        assert_eq!(flow.problem(), Some(Problem::Unavailable));
    }

    #[test]
    fn passkey_round_trip() {
        let now = Instant::now();
        let mut flow = ready(now);
        flow.start_passkey("");
        let options = one(&mut flow);
        assert_eq!(options.path, "/api/tokens/passkeyAuthentication");
        assert_eq!(options.body.as_deref().map(String::as_str), Some(r#"{"loginId":""}"#));
        flow.on_reply(options.id, 201, r#"{"passkeyAuthenticationRequest":"{\"publicKey\":{}}"}"#, now);
        assert_eq!(flow.take_ceremony().as_deref(), Some(r#"{"publicKey":{}}"#));
        flow.on_passkey(Ok(r#"{"id":"x"}"#.into()));
        let auth = one(&mut flow);
        assert_eq!(auth.body.as_deref().map(String::as_str), Some(r#"{"authenticationResponse":"{\"id\":\"x\"}"}"#));
        flow.on_reply(auth.id, 200, r#"{"redirectUri":"https://p.eagate.573.jp/gate/k/login_exec.html?code=c"}"#, now);
        assert_eq!(flow.step(), Step::Finishing);
    }

    #[test]
    fn a_cancelled_passkey_goes_back_quietly() {
        let now = Instant::now();
        let mut flow = ready(now);
        flow.start_passkey("someone");
        let options = one(&mut flow);
        flow.on_reply(options.id, 201, r#"{"passkeyAuthenticationRequest":"{}"}"#, now);
        flow.on_passkey(Err(PasskeyFailure::Cancelled));
        assert_eq!(flow.step(), Step::Id);
        assert_eq!(flow.problem(), None);
    }

    #[test]
    fn an_unknown_page_after_success_hands_over_to_the_website() {
        let now = Instant::now();
        let mut flow = ready(now);
        flow.start_passkey("");
        let options = one(&mut flow);
        flow.on_reply(options.id, 201, r#"{"passkeyAuthenticationRequest":"{}"}"#, now);
        flow.on_passkey(Ok("{}".into()));
        let auth = one(&mut flow);
        flow.on_reply(auth.id, 200, r#"{"redirectUri":"/api/authorizations"}"#, now);
        flow.on_page(Page::Other, now);
        assert_eq!(flow.step(), Step::Website);
    }

    #[test]
    fn a_return_that_never_lands_lets_the_user_retry() {
        let now = Instant::now();
        let mut flow = ready(now);
        to_password(&mut flow, now);
        flow.submit_password("x");
        let call = one(&mut flow);
        flow.on_reply(call.id, 200, r#"{"redirectUri":"/api/authorizations?state=x"}"#, now);
        assert_eq!(flow.step(), Step::Finishing);
        // The browser is told to follow the return; the UI would load it.
        assert!(flow.take_navigation().is_some());
        // Still waiting before the limit.
        flow.tick(now + FINISH_LIMIT - Duration::from_secs(1));
        assert_eq!(flow.step(), Step::Finishing);
        assert_eq!(flow.problem(), None);
        // Past it, the dialog is usable again.
        flow.tick(now + FINISH_LIMIT + Duration::from_secs(1));
        assert_eq!(flow.step(), Step::Id);
        assert_eq!(flow.problem(), Some(Problem::Unavailable));
        assert!(!flow.busy());
        assert_eq!(flow.take_navigation().as_deref(), Some(START));
        // Defer the next submit until the browser is back on the sign-in page.
        flow.submit_id(" someone@example.test ");
        assert!(flow.take_calls().is_empty());
        flow.on_page(Page::SignIn, now + FINISH_LIMIT + Duration::from_secs(2));
        assert_eq!(one(&mut flow).path, "/api/auths/login/authTypes");
    }

    #[test]
    fn a_stuck_return_can_be_retried_from_the_id_step() {
        let now = Instant::now();
        let mut flow = ready(now);
        flow.start_passkey("");
        let options = one(&mut flow);
        flow.on_reply(options.id, 201, r#"{"passkeyAuthenticationRequest":"{}"}"#, now);
        flow.on_passkey(Ok("{}".into()));
        let auth = one(&mut flow);
        flow.on_reply(auth.id, 200, r#"{"redirectUri":"/api/authorizations"}"#, now);
        assert_eq!(flow.step(), Step::Finishing);
        flow.tick(now + FINISH_LIMIT + Duration::from_secs(1));
        assert_eq!(flow.step(), Step::Id);
        assert_eq!(flow.problem(), Some(Problem::Unavailable));
    }

    #[test]
    fn the_error_page_starts_over() {
        let now = Instant::now();
        let mut flow = ready(now);
        flow.on_page(Page::Error, now);
        assert_eq!(flow.take_navigation().as_deref(), Some(START));
        assert_eq!(flow.problem(), Some(Problem::Refused));
        flow.submit_id("someone");
        assert!(flow.take_calls().is_empty(), "waits for the new page");
    }
}
