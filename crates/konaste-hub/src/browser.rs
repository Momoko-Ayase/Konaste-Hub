//! The hidden official sign-in page.
//!
//! KONAMI's sign-in API sits behind a bot check that only accepts requests
//! from its own page with its scripts running. So the page is loaded in a
//! WebView2 that is never shown, and the Hub's native form (see `signin.rs`
//! and the sign-in dialog) sends each step's request from inside that page.
//! The user never sees or types into the page.
//!
//! Navigation is limited to the official HTTPS hosts. When the page reaches
//! the e-amusement portal signed in, its cookies go to the native session.
use crate::jar::StoredCookie;
use std::{cell::RefCell, collections::VecDeque, path::PathBuf};

pub enum Event {
    /// A page finished loading.
    Page(String),
    /// The answer to a request sent with [`send`].
    Reply {
        id: u64,
        status: u16,
        body: String,
    },
    SignedIn(Vec<StoredCookie>),
}

pub(crate) fn allowed(url: &str) -> bool {
    let Ok(url) = url::Url::parse(url) else { return url == "about:blank" };
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && matches!(url.host_str(), Some("p.eagate.573.jp" | "my.konami.net" | "account.konami.net"))
}

fn is_portal(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|u| {
        u.host_str() == Some("p.eagate.573.jp") && !u.path().starts_with("/gate/p/login") && !u.path().starts_with("/gate/k/login")
    })
}

/// The script that sends one request from the page and posts the answer back.
fn call_script(nonce: u64, call: &crate::signin::Call) -> zeroize::Zeroizing<String> {
    let path = serde_json::to_string(&call.path).unwrap_or_default();
    let body = match &call.body {
        Some(body) => zeroize::Zeroizing::new(serde_json::to_string(body.as_str()).unwrap_or_default()),
        None => zeroize::Zeroizing::new("null".to_owned()),
    };
    zeroize::Zeroizing::new(format!(
        r#"(function(){{var n="{nonce}",id={id};function done(s,b){{window.ipc.postMessage(JSON.stringify({{n:n,id:id,status:s,body:b}}))}}
if(location.origin!=="https://my.konami.net"){{done(0,"");return}}
var b={body},o={{method:"{method}",redirect:"manual",credentials:"same-origin"}};
if(b!==null){{o.body=b;o.headers={{"Content-Type":"application/json"}}}}
fetch({path},o).then(function(r){{
if({binary}){{return r.arrayBuffer().then(function(a){{var u=new Uint8Array(a),s="";if(u.length>262144){{done(r.status,"");return}}for(var i=0;i<u.length;i+=32768)s+=String.fromCharCode.apply(null,u.subarray(i,i+32768));done(r.status,btoa(s))}})}}
return r.text().then(function(t){{done(r.status,t.length>262144?"":t)}})
}}).catch(function(){{done(0,"")}})}})()"#,
        id = call.id,
        method = call.method,
        binary = call.binary,
        body = body.as_str(),
    ))
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    };

    struct Raw(HWND);
    impl raw_window_handle::HasWindowHandle for Raw {
        fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
            let mut handle = raw_window_handle::Win32WindowHandle::new(
                std::num::NonZeroIsize::new(self.0 as isize).ok_or(raw_window_handle::HandleError::Unavailable)?,
            );
            handle.hinstance = None;
            Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(raw_window_handle::RawWindowHandle::Win32(handle)) })
        }
    }

    struct Active {
        hwnd: HWND,
        webview: wry::WebView,
        _context: wry::WebContext,
        nonce: u64,
    }

    thread_local! {
        static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) };
        /// Filled from WebView2 callbacks; drained by `poll`.
        static EVENTS: RefCell<VecDeque<Event>> = const { RefCell::new(VecDeque::new()) };
        /// Signed-in check on the portal: next probe time, and an unanswered probe.
        static PROBE: RefCell<Option<std::time::Instant>> = const { RefCell::new(None) };
        static ASKED: RefCell<Option<std::time::Instant>> = const { RefCell::new(None) };
        static READY: RefCell<bool> = const { RefCell::new(false) };
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    fn push(event: Event) {
        EVENTS.with(|events| events.borrow_mut().push_back(event));
    }

    unsafe extern "system" fn proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // Never shown; a stray close request is ignored.
        if message == WM_CLOSE {
            return 0;
        }
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    fn nonce() -> u64 {
        use std::hash::{BuildHasher, Hasher};
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
        hasher.finish()
    }

    pub fn is_open() -> bool {
        ACTIVE.with(|active| active.try_borrow().map(|a| a.is_some()).unwrap_or(true))
    }

    /// Load the official sign-in page in the hidden browser.
    ///
    /// Each attempt uses a fresh user-data folder, never the previous login.
    /// With remembering disabled, WebView2 also runs in InPrivate mode.
    /// Only the native session may be remembered across Hub restarts.
    pub fn open(data: PathBuf, remember: bool) -> Result<(), ()> {
        close();
        let class = wide("KonasteHubSignInHost");
        let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
        let mut wc: WNDCLASSW = unsafe { std::mem::zeroed() };
        wc.lpfnWndProc = Some(proc);
        wc.hInstance = instance;
        wc.lpszClassName = class.as_ptr();
        unsafe { RegisterClassW(&wc) };
        // A tool window that is never shown: no taskbar button, no activation.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class.as_ptr(),
                class.as_ptr(),
                WS_POPUP,
                0,
                0,
                480,
                760,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            return Err(());
        }
        let nonce = nonce();
        let expected = nonce.to_string();
        let profile = data.with_file_name(format!("sign-in-run-{nonce:016x}"));
        // Keep WebView2's environment directory under the Hub data directory
        // even in InPrivate mode, rather than beside the installed executable.
        let mut context = wry::WebContext::new(Some(profile.clone()));
        let webview = wry::WebViewBuilder::new_with_web_context(&mut context)
            .with_incognito(!remember)
            .with_url(crate::signin::START)
            .with_bounds(wry::Rect {
                position: wry::dpi::PhysicalPosition::new(0, 0).into(),
                size: wry::dpi::PhysicalSize::new(480u32, 760u32).into(),
            })
            .with_navigation_handler(|url| allowed(&url))
            .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
            .with_download_started_handler(|_, _| false)
            .with_devtools(false)
            .with_ipc_handler(move |request| {
                // Answers come only from the official sign-in page and carry this session's nonce.
                if request.uri().host() != Some("my.konami.net") {
                    return;
                }
                let Ok(message) = serde_json::from_str::<serde_json::Value>(request.body()) else { return };
                if message.get("n").and_then(|v| v.as_str()) != Some(expected.as_str()) {
                    return;
                }
                let (Some(id), Some(status)) = (message.get("id").and_then(|v| v.as_u64()), message.get("status").and_then(|v| v.as_u64()))
                else {
                    return;
                };
                let body = message.get("body").and_then(|v| v.as_str()).unwrap_or("").to_owned();
                push(Event::Reply { id, status: status.min(999) as u16, body });
            })
            .with_on_page_load_handler(move |event, url| {
                if !matches!(event, wry::PageLoadEvent::Finished) {
                    return;
                }
                if is_portal(&url) {
                    PROBE.with(|p| *p.borrow_mut() = Some(std::time::Instant::now()));
                }
                push(Event::Page(url));
            })
            .build_as_child(&Raw(hwnd))
            .map_err(|_| {
                unsafe { DestroyWindow(hwnd) };
                retire(profile);
            })?;
        ACTIVE.with(|active| *active.borrow_mut() = Some(Active { hwnd, webview, _context: context, nonce }));
        EVENTS.with(|events| events.borrow_mut().clear());
        ASKED.with(|a| *a.borrow_mut() = None);
        READY.with(|r| *r.borrow_mut() = false);
        PROBE.with(|p| *p.borrow_mut() = None);
        Ok(())
    }

    pub fn close() {
        let taken = ACTIVE.with(|active| active.try_borrow_mut().ok().and_then(|mut a| a.take()));
        let mut profile = None;
        if let Some(active) = taken {
            let hwnd = active.hwnd;
            profile = active._context.data_directory().map(std::path::Path::to_path_buf);
            drop(active);
            unsafe { DestroyWindow(hwnd) };
        }
        EVENTS.with(|events| events.borrow_mut().clear());
        if let Some(profile) = profile {
            retire(profile);
        }
    }

    /// Run at startup before any WebView opens, to remove profiles left when
    /// Windows or the Hub exited mid-sign-in.
    pub fn retire_stale(directory: &std::path::Path) {
        if let Ok(children) = std::fs::read_dir(directory) {
            for child in children.flatten() {
                let name = child.file_name();
                if name
                    .to_str()
                    .and_then(|name| name.strip_prefix("sign-in-run-"))
                    .is_some_and(|id| id.len() == 16 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
                {
                    retire(child.path());
                }
            }
        }
    }

    /// Delete a browser profile once WebView2 has let go of it. The window is
    /// closed first, but its processes can outlive it briefly, so removal is
    /// retried off the UI thread. No later sign-in will reuse this directory.
    fn retire(data: PathBuf) {
        std::thread::spawn(move || {
            // Bounded: WebView2 may hold the directory open until it exits.
            for attempt in 0..20 {
                if !data.exists() || std::fs::remove_dir_all(&data).is_ok() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(50 * (attempt + 1)));
            }
            let _ = std::fs::remove_dir_all(data);
        });
    }

    pub fn send(call: &crate::signin::Call) {
        ACTIVE.with(|active| {
            if let Ok(active) = active.try_borrow()
                && let Some(active) = active.as_ref()
            {
                let script = call_script(active.nonce, call);
                if active.webview.evaluate_script(&script).is_err() {
                    push(Event::Reply { id: call.id, status: 0, body: String::new() });
                }
            }
        });
    }

    pub fn navigate(url: &str) {
        if !allowed(url) {
            return;
        }
        ACTIVE.with(|active| {
            if let Ok(active) = active.try_borrow()
                && let Some(active) = active.as_ref()
            {
                let _ = active.webview.load_url(url);
            }
        });
    }

    /// Everything that happened since the last call. Called from the UI loop.
    pub fn poll() -> Vec<Event> {
        probe();
        if READY.with(|r| std::mem::take(&mut *r.borrow_mut()))
            && let Some(cookies) = collect()
        {
            push(Event::SignedIn(cookies));
        }
        EVENTS.with(|events| events.borrow_mut().drain(..).collect())
    }

    const PROBE_EVERY: std::time::Duration = std::time::Duration::from_millis(1500);

    /// Signed in when the page is on the portal host and its inline template
    /// says so. A page whose template is missing or not filled in yet asks My
    /// Page in the background (same origin, same cookies, no navigation) and
    /// answers from that on a later probe.
    const SIGNED_IN_SCRIPT: &str = r#"(function(){try{
        if (location.hostname !== 'p.eagate.573.jp' || location.pathname.indexOf('/gate/p/login') === 0 || location.pathname.indexOf('/gate/k/login') === 0) return 'no';
        var t = window.ea_common_template && window.ea_common_template.userstatus && window.ea_common_template.userstatus.state;
        if (t && t.login === true) return 'yes';
        var probe = window.__konasteHubProbe;
        if (probe === 'yes' || probe === 'no') { window.__konasteHubProbe = undefined; return probe; }
        if (probe !== 'pending') {
            window.__konasteHubProbe = 'pending';
            fetch('/gate/p/mypage/index.html', {credentials: 'same-origin', redirect: 'follow'})
                .then(function(r){ return r.url.indexOf('/gate/p/login') < 0 ? r.text() : ''; })
                .then(function(s){ window.__konasteHubProbe = /"login"\s*:\s*true/.test(s) ? 'yes' : 'no'; })
                .catch(function(){ window.__konasteHubProbe = 'no'; });
        }
        return 'wait';
    }catch(e){return 'no'}})()"#;

    fn probe() {
        let now = std::time::Instant::now();
        if ASKED.with(|a| a.borrow().is_some_and(|at| now.duration_since(at) < std::time::Duration::from_secs(4))) {
            return;
        }
        // Probing starts once a portal page has loaded.
        if !PROBE.with(|p| p.borrow().is_some_and(|at| now >= at)) {
            return;
        }
        ACTIVE.with(|active| {
            let Ok(active) = active.try_borrow() else { return };
            let Some(active) = active.as_ref() else { return };
            PROBE.with(|p| *p.borrow_mut() = Some(now + PROBE_EVERY));
            if !active.webview.url().is_ok_and(|url| is_portal(&url)) {
                return;
            }
            ASKED.with(|a| *a.borrow_mut() = Some(now));
            let _ = active.webview.evaluate_script_with_callback(SIGNED_IN_SCRIPT, |result| {
                ASKED.with(|a| *a.borrow_mut() = None);
                // Deferred: the callback runs inside WebView2's handler.
                if result.contains("yes") {
                    READY.with(|r| *r.borrow_mut() = true);
                    PROBE.with(|p| *p.borrow_mut() = None);
                } else if result.contains("wait") {
                    PROBE.with(|p| *p.borrow_mut() = Some(std::time::Instant::now() + std::time::Duration::from_millis(400)));
                }
            });
        });
    }

    /// Every cookie the profile holds for the portal domains.
    fn collect() -> Option<Vec<StoredCookie>> {
        let cookies = ACTIVE.with(|active| {
            let active = active.try_borrow().ok()?;
            let active = active.as_ref()?;
            let mut all = Vec::new();
            // Not only a few paths: session cookies can sit on `.573.jp` or a deeper path.
            match active.webview.cookies() {
                Ok(list) => all.extend(list.into_iter().filter(|c| {
                    c.domain().is_some_and(|d| {
                        let d = d.trim_start_matches('.').to_ascii_lowercase();
                        d == "573.jp" || d.ends_with(".573.jp")
                    })
                })),
                Err(_) => {
                    for url in ["https://p.eagate.573.jp/", "https://p.eagate.573.jp/gate/", "https://p.eagate.573.jp/game/"] {
                        if let Ok(list) = active.webview.cookies_for_url(url) {
                            all.extend(list);
                        }
                    }
                }
            }
            Some(all)
        })?;
        let mut stored: Vec<StoredCookie> = Vec::new();
        for cookie in cookies {
            let domain = cookie.domain().unwrap_or("p.eagate.573.jp").trim_start_matches('.').to_ascii_lowercase();
            let host_only = cookie.domain().is_none_or(|d| !d.starts_with('.'));
            let expires = cookie.expires_datetime().map(|at| at.unix_timestamp().max(0) as u64);
            let path = cookie.path().unwrap_or("/").to_owned();
            if stored.iter().any(|c| c.name == cookie.name() && c.domain == domain && c.path == path) {
                continue;
            }
            stored.push(StoredCookie {
                name: cookie.name().to_owned(),
                value: cookie.value().to_owned(),
                domain,
                host_only,
                path,
                secure: cookie.secure().unwrap_or(false),
                expires,
            });
        }
        Some(stored)
    }
}

#[cfg(windows)]
pub use imp::{close, is_open, navigate, open, poll, retire_stale, send};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_is_limited_to_official_https_hosts() {
        assert!(allowed("https://my.konami.net/ja/signin"));
        assert!(allowed("https://p.eagate.573.jp/gate/p/mypage/"));
        assert!(!allowed("http://p.eagate.573.jp/"));
        assert!(!allowed("https://p.eagate.573.jp.evil.test/"));
        assert!(!allowed("https://p.eagate.573.jp:8443/"));
        assert!(is_portal("https://p.eagate.573.jp/"));
        assert!(!is_portal("https://p.eagate.573.jp/gate/k/login_exec.html?code=x"));
        assert!(!is_portal("https://my.konami.net/ja/signin"));
        assert!(!allowed("konaste.ddr://login?tk=x"));
    }

    #[test]
    fn request_script_escapes_its_values() {
        let call = crate::signin::Call {
            id: 7,
            method: "POST",
            path: "/api/logins".into(),
            body: Some(zeroize::Zeroizing::new(r#"{"password":"a\"</script>"}"#.into())),
            binary: false,
        };
        let script = call_script(42, &call);
        assert!(script.contains(r#"var n="42",id=7;"#));
        assert!(script.contains(r#"var b="{\"password\":\"a\\\"</script>\"}""#));
        assert!(script.contains(r#"fetch("/api/logins",o)"#));
        assert!(script.contains(r#"location.origin!=="https://my.konami.net""#));
    }
}
