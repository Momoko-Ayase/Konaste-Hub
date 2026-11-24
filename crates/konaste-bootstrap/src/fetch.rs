//! Pinned HTTPS download.
//!
//! The client follows at most [`crate::MAX_REDIRECTS`] redirects, and only
//! when the next URL is HTTPS on the package's own recorded host. The body is
//! copied in chunks and aborted past [`crate::MAX_PACKAGE_BYTES`]. Nothing
//! here parses the MSI.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use url::Url;

use crate::catalog::OfficialPackage;
use crate::error::BootstrapError;
use crate::{DOWNLOAD_TIMEOUT, MAX_PACKAGE_BYTES, MAX_REDIRECTS};

pub(crate) fn download_pinned(
    package: &OfficialPackage,
    destination: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(), BootstrapError> {
    download_pinned_url(package.host, package.url, MAX_PACKAGE_BYTES, destination, progress)
}

/// Download one literal HTTPS URL that must stay on `pinned_host`.
///
/// `byte_limit` is the abort threshold for this object. It is never larger than
/// [`MAX_PACKAGE_BYTES`]. Redirects use the same host policy as the MSI.
pub(crate) fn download_pinned_url(
    pinned_host: &'static str,
    literal: &str,
    byte_limit: u64,
    destination: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(), BootstrapError> {
    let byte_limit = byte_limit.min(MAX_PACKAGE_BYTES);
    let url = Url::parse(literal).map_err(|err| BootstrapError::fetch(err.to_string()))?;
    check_url(&url, pinned_host)?;

    let client = reqwest::blocking::Client::builder()
        .https_only(true)
        .redirect(redirect_policy(pinned_host))
        .timeout(DOWNLOAD_TIMEOUT)
        .connect_timeout(Duration::from_secs(20))
        .user_agent("konaste-hub/0.1 (bootstrap)")
        .build()
        .map_err(|err| BootstrapError::fetch(err.to_string()))?;

    let response = client
        .get(url)
        .send()
        .map_err(|err| BootstrapError::fetch(err.to_string()))?
        .error_for_status()
        .map_err(|err| BootstrapError::fetch(err.to_string()))?;

    let final_url = response.url().clone();
    check_url(&final_url, pinned_host)?;

    if let Some(length) = response.content_length()
        && length > byte_limit
    {
        return Err(BootstrapError::fetch(format!("content-length {length} exceeds {byte_limit} byte budget")));
    }

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let partial = destination.with_extension("msi.partial");
    let mut file = File::create(&partial)?;
    let mut body = response;
    let mut buf = [0_u8; 64 * 1024];
    let mut received = 0_u64;
    let total = body.content_length();
    loop {
        let read = body.read(&mut buf).map_err(|err| BootstrapError::fetch(err.to_string()))?;
        if read == 0 {
            break;
        }
        received += read as u64;
        if received > byte_limit {
            drop(file);
            let _ = fs::remove_file(&partial);
            return Err(BootstrapError::fetch(format!("download exceeded {byte_limit} byte budget")));
        }
        file.write_all(&buf[..read])?;
        progress(received, total);
    }
    file.flush()?;
    drop(file);
    fs::rename(&partial, destination)?;
    Ok(())
}

fn check_url(url: &Url, pinned_host: &str) -> Result<(), BootstrapError> {
    if url.scheme() != "https" {
        return Err(BootstrapError::fetch(format!("refusing non-https url {url}")));
    }
    match url.host_str() {
        Some(host) if host.eq_ignore_ascii_case(pinned_host) => Ok(()),
        other => Err(BootstrapError::fetch(format!("refusing host {other:?}; pin allows only {}", pinned_host))),
    }
}

fn redirect_policy(pinned_host: &'static str) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() > MAX_REDIRECTS {
            return attempt.error(format!("more than {MAX_REDIRECTS} redirects"));
        }
        let next = attempt.url().clone();
        if next.scheme() != "https" {
            return attempt.error(format!("refusing non-https redirect to {next}"));
        }
        let host = next.host_str().map(str::to_string);
        match host {
            Some(host) if host.eq_ignore_ascii_case(pinned_host) => attempt.follow(),
            other => attempt.error(format!("refusing redirect host {other:?}; pin allows only {pinned_host}")),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{BOMBERGIRL, SDVX};

    #[test]
    fn pin_check_rejects_other_hosts_and_schemes() {
        let other = Url::parse("https://example.com/Installer/BomberGirl_Installer.msi").unwrap();
        assert!(check_url(&other, BOMBERGIRL.host).is_err());
        let http = Url::parse("http://d1q1iflay6y5jj.cloudfront.net/Installer/2021122200_b37bc853/BomberGirl_Installer.msi").unwrap();
        assert!(check_url(&http, BOMBERGIRL.host).is_err());
        let ok = Url::parse(SDVX.url).unwrap();
        assert!(check_url(&ok, SDVX.host).is_ok());
        assert!(check_url(&ok, BOMBERGIRL.host).is_err());
        let update = Url::parse(SDVX.runtime_update.unwrap().url).unwrap();
        assert!(check_url(&update, SDVX.host).is_ok());
        assert!(check_url(&update, BOMBERGIRL.host).is_err());
    }
}
