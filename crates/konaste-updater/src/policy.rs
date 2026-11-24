//! Where bytes may be fetched from.
//!
//! Official mode is HTTPS on an explicit host list (the two known CDN hosts,
//! not a wildcard). Loopback mode exists only so tests can speak HTTP to a
//! local server. Content redirects are refused, not followed.

use url::Url;

use crate::error::UpdateError;

#[derive(Debug, Clone)]
pub enum DownloadPolicy {
    /// HTTPS, port 443, host equal to one entry (ASCII case-insensitive).
    Official { hosts: Vec<String> },
    /// HTTP or HTTPS to a loopback host, any port. Not for production.
    LoopbackTest,
}

impl DownloadPolicy {
    pub fn official(hosts: impl IntoIterator<Item = impl AsRef<str>>) -> Result<Self, UpdateError> {
        let mut cleaned = Vec::new();
        for host in hosts {
            let host = host.as_ref().trim().trim_end_matches('.').to_ascii_lowercase();
            if host.is_empty()
                || host.len() > 253
                || host.contains('*')
                || host.contains('/')
                || host.contains('\\')
                || host.contains(':')
                || host.parse::<std::net::IpAddr>().is_ok()
            {
                return Err(UpdateError::policy(&host, "official allowlist entries must be DNS names without wildcards"));
            }
            if !cleaned.iter().any(|existing: &String| existing == &host) {
                cleaned.push(host);
            }
        }
        if cleaned.is_empty() {
            return Err(UpdateError::policy("", "official allowlist is empty"));
        }
        Ok(Self::Official { hosts: cleaned })
    }

    pub fn loopback_test() -> Self {
        Self::LoopbackTest
    }

    pub(crate) fn check_url(&self, raw: &str) -> Result<Url, UpdateError> {
        let url = Url::parse(raw).map_err(|err| UpdateError::policy(raw, err.to_string()))?;
        self.check_parsed(&url)?;
        Ok(url)
    }

    pub(crate) fn check_parsed(&self, url: &Url) -> Result<(), UpdateError> {
        if url.username() != "" || url.password().is_some() {
            return Err(UpdateError::policy(url.as_str(), "credentials in URLs are not allowed"));
        }
        match self {
            Self::Official { hosts } => {
                if url.scheme() != "https" {
                    return Err(UpdateError::policy(url.as_str(), "official downloads require https"));
                }
                if url.port_or_known_default() != Some(443) {
                    return Err(UpdateError::policy(url.as_str(), "official downloads use port 443"));
                }
                let host = url
                    .host_str()
                    .ok_or_else(|| UpdateError::policy(url.as_str(), "missing host"))?
                    .trim_end_matches('.')
                    .to_ascii_lowercase();
                if !hosts.iter().any(|allowed| allowed == &host) {
                    return Err(UpdateError::policy(url.as_str(), "host is not on the official allowlist"));
                }
            }
            Self::LoopbackTest => {
                if url.scheme() != "http" && url.scheme() != "https" {
                    return Err(UpdateError::policy(url.as_str(), "only http and https are allowed"));
                }
                if !is_loopback(url) {
                    return Err(UpdateError::policy(url.as_str(), "loopback test policy rejects non-loopback hosts"));
                }
            }
        }
        Ok(())
    }
}

fn is_loopback(url: &Url) -> bool {
    match url.host_str() {
        Some("localhost") => true,
        Some(host) => match host.trim_start_matches('[').trim_end_matches(']').parse::<std::net::IpAddr>() {
            Ok(ip) => ip.is_loopback(),
            Err(_) => false,
        },
        None => false,
    }
}
