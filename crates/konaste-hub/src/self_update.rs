//! Hub self-update from GitHub releases.
//!
//! The Hub asks GitHub for the latest published release (drafts and
//! prereleases are excluded by the API). A newer version is offered in the
//! title bar; nothing is downloaded until the user confirms. The setup
//! executable must match both the SHA-256 GitHub records for the asset and the
//! line in the release's `SHA256SUMS.txt`. The verified setup then runs
//! silently after the Hub exits, replaces the program, and restarts it.
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

const LATEST: &str = "https://api.github.com/repos/Momoko-Ayase/Konaste-Hub/releases/latest";
const SUMS: &str = "SHA256SUMS.txt";
/// The setup is a few MB; anything far larger is not ours.
const MAX_SETUP_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SUMS_BYTES: u64 = 64 * 1024;
const MAX_RELEASE_BYTES: u64 = 1024 * 1024;
pub const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    setup_url: String,
    setup_name: String,
    setup_size: u64,
    /// GitHub's own record of the asset's SHA-256, when present.
    setup_digest: Option<String>,
    sums_url: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(u64, u64, u64);

impl Version {
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.strip_prefix('v').unwrap_or(text).split('.').map(|part| part.parse::<u64>().ok());
        let version = Version(parts.next()??, parts.next()??, parts.next()??);
        parts.next().is_none().then_some(version)
    }

    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).expect("the workspace version is MAJOR.MINOR.PATCH")
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

#[derive(serde::Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    assets: Vec<ApiAsset>,
}

#[derive(serde::Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}

/// The release, if it is published, newer than this build, and carries a
/// setup plus checksum list from the release workflow.
fn parse_release(json: &[u8], current: Version) -> Option<Release> {
    let api: ApiRelease = serde_json::from_slice(json).ok()?;
    if api.draft || api.prerelease {
        return None;
    }
    let version = Version::parse(&api.tag_name).filter(|version| *version > current)?;
    let setup_name = format!("KonasteHub-{version}-setup.exe");
    let setup = api.assets.iter().find(|asset| asset.name == setup_name)?;
    let sums = api.assets.iter().find(|asset| asset.name == SUMS)?;
    if !trusted_url(&setup.browser_download_url) || !trusted_url(&sums.browser_download_url) || setup.size > MAX_SETUP_BYTES {
        return None;
    }
    let setup_digest = match &setup.digest {
        None => None,
        Some(digest) => Some(digest.strip_prefix("sha256:").filter(|hex| is_sha256(hex))?.to_ascii_lowercase()),
    };
    Some(Release {
        version,
        setup_url: setup.browser_download_url.clone(),
        setup_name,
        setup_size: setup.size,
        setup_digest,
        sums_url: sums.browser_download_url.clone(),
    })
}

fn trusted_url(url: &str) -> bool {
    url.starts_with("https://github.com/Momoko-Ayase/Konaste-Hub/releases/download/")
}

fn is_sha256(hex: &str) -> bool {
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// The digest `SHA256SUMS.txt` lists for `name` (`<hex>  <name>` lines).
fn listed_digest(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hex, file) = line.trim().split_once(char::is_whitespace)?;
        (file.trim_start_matches([' ', '*']) == name && is_sha256(hex)).then(|| hex.to_ascii_lowercase())
    })
}

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .https_only(true)
        .user_agent(concat!("KonasteHub/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .expect("update client")
}

/// A newer published release, or `None` when current or unreachable.
pub async fn check(client: &reqwest::Client) -> Option<Release> {
    let response = client
        .get(LATEST)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    parse_release(&read_capped(response, MAX_RELEASE_BYTES).await?, Version::current())
}

async fn read_capped(mut response: reqwest::Response, limit: u64) -> Option<Vec<u8>> {
    if response.content_length().is_some_and(|length| length > limit) {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if (bytes.len() + chunk.len()) as u64 > limit {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    Some(bytes)
}

async fn fetch(client: &reqwest::Client, url: &str, limit: u64) -> Result<Vec<u8>, &'static str> {
    let response = client.get(url).timeout(Duration::from_secs(300)).send().await.map_err(|_| "hubUpdateDownload")?;
    if !response.status().is_success() {
        return Err("hubUpdateDownload");
    }
    read_capped(response, limit).await.ok_or("hubUpdateDownload")
}

/// Download the setup into `directory` and verify it. Returns the verified path.
pub async fn download(client: &reqwest::Client, release: &Release, directory: &Path) -> Result<PathBuf, &'static str> {
    let sums = fetch(client, &release.sums_url, MAX_SUMS_BYTES).await?;
    let listed = std::str::from_utf8(&sums).ok().and_then(|sums| listed_digest(sums, &release.setup_name)).ok_or("hubUpdateVerify")?;
    if release.setup_digest.as_ref().is_some_and(|digest| *digest != listed) {
        return Err("hubUpdateVerify");
    }
    let setup = fetch(client, &release.setup_url, MAX_SETUP_BYTES).await?;
    if setup.len() as u64 != release.setup_size || hex::encode(Sha256::digest(&setup)) != listed {
        return Err("hubUpdateVerify");
    }
    let directory = directory.join("update");
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).map_err(|_| "hubUpdateDownload")?;
    let path = directory.join(&release.setup_name);
    std::fs::write(&path, &setup).map_err(|_| "hubUpdateDownload")?;
    Ok(path)
}

/// Remove a setup left from an earlier update.
pub fn clean(directory: &Path) {
    let _ = std::fs::remove_dir_all(directory.join("update"));
}

/// Start the verified setup. It waits up to 30 s for this process to release
/// the app mutex, installs over the current folder, and restarts the Hub.
/// `/SILENT` keeps its progress window; prerequisite prompts default to No.
#[cfg(windows)]
pub fn run(setup: &Path) -> Result<(), &'static str> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    std::process::Command::new(setup)
        .args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/RESTARTHUB"])
        .creation_flags(DETACHED_PROCESS)
        .spawn()
        .map(drop)
        .map_err(|_| "hubUpdateRun")
}

#[cfg(not(windows))]
pub fn run(_: &Path) -> Result<(), &'static str> {
    Err("windowsOnly")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "1c7a49d70589b68c1549a4c60a8ac74c91dac62571cfbab244ab2ebff689b83e";

    fn release_json(tag: &str, digest: Option<&str>) -> String {
        let base = "https://github.com/Momoko-Ayase/Konaste-Hub/releases/download";
        let digest = digest.map_or("null".to_owned(), |d| format!("\"{d}\""));
        format!(
            r#"{{"tag_name":"{tag}","draft":false,"prerelease":false,"assets":[
                {{"name":"KonasteHub-{v}-setup.exe","browser_download_url":"{base}/{tag}/KonasteHub-{v}-setup.exe","size":7680089,"digest":{digest}}},
                {{"name":"SHA256SUMS.txt","browser_download_url":"{base}/{tag}/SHA256SUMS.txt","size":338,"digest":null}}]}}"#,
            v = tag.trim_start_matches('v'),
        )
    }

    #[test]
    fn versions_compare_numerically_and_reject_extras() {
        assert!(Version::parse("v0.10.0").unwrap() > Version::parse("0.9.9").unwrap());
        assert_eq!(Version::parse("v1.2.3"), Some(Version(1, 2, 3)));
        for bad in ["1.2", "1.2.3.4", "1.2.x", "v", "", "1.2.3-beta"] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
        assert_eq!(Version::current().to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn only_a_newer_published_release_with_both_assets_is_offered() {
        let current = Version(0, 1, 0);
        let release = parse_release(release_json("v0.2.0", Some(&format!("sha256:{DIGEST}"))).as_bytes(), current).unwrap();
        assert_eq!(release.version, Version(0, 2, 0));
        assert_eq!(release.setup_name, "KonasteHub-0.2.0-setup.exe");
        assert_eq!(release.setup_digest.as_deref(), Some(DIGEST));
        assert!(parse_release(release_json("v0.2.0", None).as_bytes(), current).unwrap().setup_digest.is_none());

        assert!(parse_release(release_json("v0.1.0", None).as_bytes(), current).is_none(), "same version");
        assert!(parse_release(release_json("v0.0.9", None).as_bytes(), current).is_none(), "older");
        let prerelease = release_json("v0.2.0", None).replace(r#""prerelease":false"#, r#""prerelease":true"#);
        assert!(parse_release(prerelease.as_bytes(), current).is_none());
        let elsewhere = release_json("v0.2.0", None).replace("github.com/Momoko-Ayase", "github.com/someone-else");
        assert!(parse_release(elsewhere.as_bytes(), current).is_none(), "assets must come from this repository");
        let no_sums = release_json("v0.2.0", None).replace("SHA256SUMS.txt", "notes.txt");
        assert!(parse_release(no_sums.as_bytes(), current).is_none());
        assert!(parse_release(release_json("v0.2.0", Some("md5:abc")).as_bytes(), current).is_none(), "unknown digest kind");
        assert!(parse_release(b"<html>", current).is_none());
    }

    #[test]
    fn checksum_list_lookup_matches_the_exact_file_name() {
        let sums = format!(
            "{}  konaste-hub.exe\n{DIGEST}  KonasteHub-0.2.0-setup.exe\n{}  KonasteHub-0.2.0-setup.exe.bak\n",
            "a".repeat(64),
            "b".repeat(64)
        );
        assert_eq!(listed_digest(&sums, "KonasteHub-0.2.0-setup.exe").as_deref(), Some(DIGEST));
        assert_eq!(listed_digest(&sums.replace("  ", " *"), "KonasteHub-0.2.0-setup.exe").as_deref(), Some(DIGEST));
        assert_eq!(listed_digest(&sums, "KonasteHub-0.3.0-setup.exe"), None);
        assert_eq!(listed_digest("nothex  KonasteHub-0.2.0-setup.exe", "KonasteHub-0.2.0-setup.exe"), None);
    }

    /// Serves `files` over plain HTTP on loopback, one request per connection.
    fn serve(files: Vec<(&'static str, Vec<u8>)>) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut request = [0u8; 2048];
                let read = stream.read(&mut request).unwrap_or(0);
                let line = String::from_utf8_lossy(&request[..read]);
                let path = line.split_whitespace().nth(1).unwrap_or("").trim_start_matches('/').to_owned();
                let response = match files.iter().find(|(name, _)| *name == path) {
                    Some((_, body)) => {
                        let mut head =
                            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
                        head.extend_from_slice(body);
                        head
                    }
                    None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                };
                let _ = stream.write_all(&response);
            }
        });
        base
    }

    fn local_release(base: &str, setup: &[u8], digest: Option<String>) -> Release {
        Release {
            version: Version(0, 2, 0),
            setup_url: format!("{base}/setup"),
            setup_name: "KonasteHub-0.2.0-setup.exe".into(),
            setup_size: setup.len() as u64,
            setup_digest: digest,
            sums_url: format!("{base}/sums"),
        }
    }

    #[tokio::test]
    async fn only_a_setup_matching_both_checksums_is_kept() {
        let setup = b"MZ synthetic setup".to_vec();
        let good = hex::encode(Sha256::digest(&setup));
        let sums = format!("{}  konaste-hub.exe\n{good}  KonasteHub-0.2.0-setup.exe\n", "a".repeat(64)).into_bytes();
        let base = serve(vec![
            ("setup", setup.clone()),
            ("sums", sums),
            ("wrong-sums", format!("{}  KonasteHub-0.2.0-setup.exe\n", "b".repeat(64)).into_bytes()),
        ]);
        let client = reqwest::Client::new();
        let data = tempfile::tempdir().unwrap();

        let path = download(&client, &local_release(&base, &setup, Some(good.clone())), data.path()).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), setup);
        assert_eq!(path, data.path().join("update").join("KonasteHub-0.2.0-setup.exe"));

        let mut wrong_list = local_release(&base, &setup, None);
        wrong_list.sums_url = format!("{base}/wrong-sums");
        assert_eq!(download(&client, &wrong_list, data.path()).await, Err("hubUpdateVerify"));

        let disagreeing = local_release(&base, &setup, Some("c".repeat(64)));
        assert_eq!(download(&client, &disagreeing, data.path()).await, Err("hubUpdateVerify"), "GitHub's digest disagrees with the list");

        let mut short = local_release(&base, &setup, None);
        short.setup_size += 1;
        assert_eq!(download(&client, &short, data.path()).await, Err("hubUpdateVerify"));

        let mut missing = local_release(&base, &setup, None);
        missing.setup_url = format!("{base}/gone");
        assert_eq!(download(&client, &missing, data.path()).await, Err("hubUpdateDownload"));

        clean(data.path());
        assert!(!data.path().join("update").exists());
    }

    #[test]
    fn the_installer_accepts_the_restart_switch() {
        let iss = include_str!("../../../installer/konaste-hub.iss");
        assert!(iss.contains("'/RESTARTHUB'"), "setup must recognise a self-update");
        assert!(iss.contains("Check: IsSelfUpdate"), "setup must restart the Hub after a self-update");
        assert!(iss.contains("Wait := 30"), "setup must wait for the Hub that started it to exit");
    }
}
