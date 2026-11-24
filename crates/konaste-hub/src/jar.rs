//! Cookie jar for the native e-amusement client.
//!
//! Filled from the sign-in browser, then kept current from responses. Saved
//! with Windows DPAPI for the current user when "Stay signed in" is on.
use reqwest::header::HeaderValue;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredCookie {
    pub name: String,
    pub value: String,
    /// Lower case, without a leading dot.
    pub domain: String,
    pub host_only: bool,
    pub path: String,
    pub secure: bool,
    /// Unix seconds. `None` is a session cookie.
    pub expires: Option<u64>,
}

pub struct Jar {
    cookies: Mutex<Vec<StoredCookie>>,
    file: PathBuf,
    persist: Mutex<bool>,
    // Hold across an epoch check and cookie I/O, to order old responses with sign-out.
    transition: Mutex<()>,
    /// Bumped whenever the whole cookie set is replaced or cleared. Work
    /// started against an older epoch must not write into the new one.
    epoch: AtomicU64,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn domain_matches(cookie: &StoredCookie, host: &str) -> bool {
    if cookie.host_only { host == cookie.domain } else { host == cookie.domain || host.ends_with(&format!(".{}", cookie.domain)) }
}

fn path_matches(cookie: &StoredCookie, path: &str) -> bool {
    path == cookie.path || (path.starts_with(&cookie.path) && (cookie.path.ends_with('/') || path[cookie.path.len()..].starts_with('/')))
}

/// Only the official portal domains are stored or sent.
fn allowed_domain(domain: &str) -> bool {
    domain == "573.jp" || domain.ends_with(".573.jp")
}

impl Jar {
    pub fn new(file: PathBuf) -> Self {
        Self { cookies: Mutex::new(Vec::new()), file, persist: Mutex::new(false), transition: Mutex::new(()), epoch: AtomicU64::new(0) }
    }

    /// The current generation of the stored cookie set.
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }

    /// True if the cookie set has not been replaced or cleared since `epoch`.
    pub fn same_epoch(&self, epoch: u64) -> bool {
        self.epoch() == epoch
    }

    pub fn load(&self, persist: bool) {
        let _transition = self.transition.lock().unwrap();
        *self.persist.lock().unwrap() = persist;
        if !persist {
            self.remove_saved();
            return;
        }
        let Ok(bytes) = std::fs::read(&self.file) else { return };
        let Some(plain) = crate::platform::unprotect(&bytes) else { return };
        if let Ok(list) = serde_json::from_slice::<Vec<StoredCookie>>(&plain) {
            let now = now();
            *self.cookies.lock().unwrap() =
                list.into_iter().filter(|c| allowed_domain(&c.domain) && c.expires.is_none_or(|e| e > now)).collect();
        }
    }

    pub fn set_persist(&self, persist: bool) {
        let _transition = self.transition.lock().unwrap();
        *self.persist.lock().unwrap() = persist;
        self.save();
    }

    pub fn replace(&self, cookies: Vec<StoredCookie>) {
        let _transition = self.transition.lock().unwrap();
        self.epoch.fetch_add(1, Ordering::AcqRel);
        *self.cookies.lock().unwrap() = cookies.into_iter().filter(|c| allowed_domain(&c.domain)).collect();
        self.save();
    }

    pub fn clear(&self) {
        let _transition = self.transition.lock().unwrap();
        self.epoch.fetch_add(1, Ordering::AcqRel);
        self.cookies.lock().unwrap().clear();
        self.remove_saved();
    }

    fn remove_saved(&self) {
        for path in [&self.file, &self.file.with_extension("tmp")] {
            if let Err(error) = std::fs::remove_file(path)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                crate::log::line("session: could not delete saved cookie data");
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.cookies.lock().unwrap().is_empty()
    }

    fn save(&self) {
        if !*self.persist.lock().unwrap() {
            self.remove_saved();
            return;
        }
        let list: Vec<StoredCookie> = self.cookies.lock().unwrap().iter().filter(|c| c.expires.is_some()).cloned().collect();
        let Ok(plain) = serde_json::to_vec(&list) else { return };
        if let Some(sealed) = crate::platform::protect(&plain) {
            let temporary = self.file.with_extension("tmp");
            if std::fs::write(&temporary, sealed).is_ok() {
                let _ = crate::platform::replace_file(&temporary, &self.file);
            }
        }
    }

    fn upsert(&self, cookie: StoredCookie) {
        let mut cookies = self.cookies.lock().unwrap();
        cookies.retain(|c| !(c.name == cookie.name && c.domain == cookie.domain && c.path == cookie.path));
        if cookie.expires.is_none_or(|e| e > now()) {
            cookies.push(cookie);
        }
    }
}

/// A cookie store bound to one generation of a [`Jar`].
///
/// A request started before a sign-out still finishes and still answers; its
/// `Set-Cookie` headers must not land in the cleared jar. The client built for
/// that request carries the epoch it saw, and the wrapper drops both reads and
/// writes once the jar has moved on.
pub struct Fenced {
    pub jar: std::sync::Arc<Jar>,
    pub epoch: u64,
}

impl reqwest::cookie::CookieStore for Fenced {
    fn set_cookies(&self, headers: &mut dyn Iterator<Item = &HeaderValue>, url: &url::Url) {
        let _transition = self.jar.transition.lock().unwrap();
        if self.jar.same_epoch(self.epoch) {
            self.jar.set_cookies(headers, url);
        }
    }

    fn cookies(&self, url: &url::Url) -> Option<HeaderValue> {
        let _transition = self.jar.transition.lock().unwrap();
        if self.jar.same_epoch(self.epoch) { self.jar.cookies(url) } else { None }
    }
}

impl reqwest::cookie::CookieStore for Jar {
    fn set_cookies(&self, headers: &mut dyn Iterator<Item = &HeaderValue>, url: &url::Url) {
        let Some(host) = url.host_str().map(str::to_ascii_lowercase) else { return };
        let mut changed = false;
        for header in headers {
            let Ok(text) = header.to_str() else { continue };
            let Ok(parsed) = cookie::Cookie::parse(text.to_owned()) else { continue };
            let (domain, host_only) = match parsed.domain() {
                Some(domain) => {
                    let domain = domain.trim_start_matches('.').to_ascii_lowercase();
                    if !(host == domain || host.ends_with(&format!(".{domain}"))) {
                        continue;
                    }
                    (domain, false)
                }
                None => (host.clone(), true),
            };
            if !allowed_domain(&domain) {
                continue;
            }
            let expires = if let Some(age) = parsed.max_age() {
                Some(now().saturating_add(age.whole_seconds().max(0) as u64))
            } else {
                parsed.expires_datetime().map(|at| at.unix_timestamp().max(0) as u64)
            };
            let path = parsed.path().filter(|p| p.starts_with('/')).unwrap_or("/").to_owned();
            self.upsert(StoredCookie {
                name: parsed.name().to_owned(),
                value: parsed.value().to_owned(),
                domain,
                host_only,
                path,
                secure: parsed.secure().unwrap_or(false),
                expires,
            });
            changed = true;
        }
        if changed {
            self.save();
        }
    }

    fn cookies(&self, url: &url::Url) -> Option<HeaderValue> {
        let host = url.host_str()?.to_ascii_lowercase();
        let secure = url.scheme() == "https";
        let now = now();
        let cookies = self.cookies.lock().unwrap();
        let pairs: Vec<String> = cookies
            .iter()
            .filter(|c| domain_matches(c, &host) && path_matches(c, url.path()) && (secure || !c.secure))
            .filter(|c| c.expires.is_none_or(|e| e > now))
            .map(|c| format!("{}={}", c.name, c.value))
            .collect();
        if pairs.is_empty() {
            return None;
        }
        HeaderValue::from_str(&pairs.join("; ")).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::cookie::CookieStore;

    #[test]
    fn domain_and_path_rules() {
        let dir = tempfile::tempdir().unwrap();
        let jar = Jar::new(dir.path().join("c"));
        let url = url::Url::parse("https://p.eagate.573.jp/gate/p/mypage/").unwrap();
        let headers = [
            HeaderValue::from_static("M573SSID=abc; Path=/; Secure; HttpOnly"),
            HeaderValue::from_static("x=1; Domain=.eagate.573.jp; Path=/gate"),
            HeaderValue::from_static("evil=1; Domain=example.com"),
        ];
        jar.set_cookies(&mut headers.iter(), &url);
        let sent = jar.cookies(&url).unwrap();
        let sent = sent.to_str().unwrap();
        assert!(sent.contains("M573SSID=abc") && sent.contains("x=1") && !sent.contains("evil"));
        let other = url::Url::parse("https://p.eagate.573.jp/game/").unwrap();
        assert_eq!(jar.cookies(&other).unwrap().to_str().unwrap(), "M573SSID=abc");
        assert!(jar.cookies(&url::Url::parse("https://example.com/").unwrap()).is_none());
    }

    fn cookie(name: &str) -> StoredCookie {
        StoredCookie {
            name: name.into(),
            value: "v".into(),
            domain: "573.jp".into(),
            host_only: false,
            path: "/".into(),
            secure: true,
            expires: Some(now() + 3600),
        }
    }

    #[test]
    fn replacing_or_clearing_cookies_bumps_the_epoch() {
        let dir = tempfile::tempdir().unwrap();
        let jar = Jar::new(dir.path().join("c"));
        let start = jar.epoch();
        jar.replace(vec![cookie("a")]);
        let after_replace = jar.epoch();
        assert!(after_replace > start);
        assert!(!jar.same_epoch(start), "a read from before the replace is stale");
        assert!(jar.same_epoch(after_replace));
        jar.clear();
        assert!(!jar.same_epoch(after_replace), "a sign-out clear must fence older reads");
        assert!(jar.is_empty());
    }

    #[test]
    fn a_fenced_store_stops_after_sign_out() {
        let dir = tempfile::tempdir().unwrap();
        let jar = std::sync::Arc::new(Jar::new(dir.path().join("c")));
        jar.replace(vec![cookie("a")]);
        let fenced = Fenced { jar: std::sync::Arc::clone(&jar), epoch: jar.epoch() };
        let url = url::Url::parse("https://p.eagate.573.jp/").unwrap();
        assert!(fenced.cookies(&url).is_some());
        // A sign-out, then a new session's cookies.
        jar.clear();
        jar.replace(vec![cookie("b")]);
        // A late Set-Cookie from a request already in flight.
        let headers = [HeaderValue::from_static("late=1; Path=/; Secure")];
        fenced.set_cookies(&mut headers.iter(), &url);
        let stored = jar.cookies(&url).unwrap();
        let stored = stored.to_str().unwrap();
        assert_eq!(stored, "b=v", "a fenced store must not write into the new jar");
        assert!(fenced.cookies(&url).is_none(), "nor read from it");
    }

    #[test]
    fn ordinary_cookie_updates_keep_the_epoch() {
        let dir = tempfile::tempdir().unwrap();
        let jar = Jar::new(dir.path().join("c"));
        jar.replace(vec![cookie("a")]);
        let epoch = jar.epoch();
        assert!(jar.same_epoch(epoch));
        let url = url::Url::parse("https://p.eagate.573.jp/").unwrap();
        let headers = [HeaderValue::from_static("b=2; Path=/; Secure")];
        jar.set_cookies(&mut headers.iter(), &url);
        assert_eq!(jar.epoch(), epoch, "a server cookie does not change the session's identity");
    }
}
