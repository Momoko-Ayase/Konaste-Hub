//! Read-only current title-bound discovery through a pinned runtime worker.
//!
//! Each check starts a fresh process, lets official mode 2 complete normally with
//! the fixed non-account placeholder, obtains the public resource URL/digest,
//! then independently fetches and decodes the bounded HTTPS index. No cached
//! result, game token, installation root, updater plan, or launch permission is
//! accepted or produced. A bootstrap package alone does not enable a title:
//! discovery also requires its verified native contract and HTTPS namespace.
//!
//! The caller supplies its own executable, whose entry point must dispatch
//! [`konaste_runtime::WORKER_ARGUMENT`] to [`konaste_runtime::worker_main`] before
//! starting its GUI/runtime. Only that exact native executable is spawned; no
//! shell or official executable is run. The worker is job-limited, not a general
//! filesystem sandbox. Its verified native contract mounts only fresh scratch
//! storage, never an installation.

use std::{io::Read, path::Path, time::Duration};

use konaste_bootstrap::GameFamily;
use konaste_manifest::{MAX_INDEX_BYTES, ResourceIndex};
use konaste_runtime::{ResourceLocation, RuntimeError};
use thiserror::Error;
use url::Url;

#[cfg(test)]
const HOST: &str = "d1q1iflay6y5jj.cloudfront.net";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DiscoveryError {
    #[error("current discovery is not verified for this title")]
    Unsupported,
    #[error("a pinned official runtime pair is missing")]
    ComponentsMissing,
    #[error("official runtime validation or initialization failed")]
    Runtime,
    #[error("the official metadata worker failed")]
    Worker,
    #[error("the official metadata worker timed out")]
    Timeout,
    #[error("invalid metadata worker response")]
    Protocol,
    #[error("could not retrieve the official resource index")]
    Network,
    #[error("the official resource index could not be verified")]
    InvalidMetadata,
}

/// A freshly retrieved, digest-checked index, not an executable update plan.
///
/// `bytes` are the exact body accepted by the independently supplied digest and
/// the strict decoder. They are not an updater plan and this type is not `Clone`.
#[derive(Debug)]
pub struct CurrentMetadata {
    pub index: ResourceIndex,
    pub sha256: [u8; 32],
    family: GameFamily,
    location: Url,
    bytes: Vec<u8>,
}

impl CurrentMetadata {
    pub fn family(&self) -> GameFamily {
        self.family
    }

    /// Checked public index location. It never has credentials, query, or fragment.
    pub fn index_url(&self) -> &Url {
        &self.location
    }

    /// Bind local evidence to this title and the exact authenticated body, not
    /// the caller-mutable decoded projection. This does not grant entitlement.
    pub fn installation_pin(&self) -> Result<konaste_runtime::InstallationPin, DiscoveryError> {
        self.installation_pins().map(|(installation, _)| installation)
    }

    /// Derive both pins from one decode of the same authenticated index. A
    /// missing settings record does not make the game installation invalid.
    pub fn installation_pins(&self) -> Result<(konaste_runtime::InstallationPin, Option<konaste_runtime::SettingsPin>), DiscoveryError> {
        let title = konaste_runtime::title(self.family).map_err(map_runtime)?;
        let index = konaste_manifest::decode(&self.bytes, self.sha256, &[title.host]).map_err(|_| DiscoveryError::InvalidMetadata)?;
        let module = index
            .files
            .iter()
            .find(|file| file.path == title.manifest_module && file.stored_size > 0)
            .ok_or(DiscoveryError::InvalidMetadata)?;
        let pin = konaste_runtime::InstallationPin {
            index_sha256: self.sha256,
            module_sha256: module.stored_sha256,
            module_bytes: module.stored_size,
        };
        let settings = konaste_runtime::settings_path(self.family).ok().and_then(|path| settings_pin_in(&index, path).ok());
        Ok((pin, settings))
    }

    /// Exact authenticated index body. Callers may publish these bytes; this
    /// crate does not translate them into an installation plan.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Per-title settings size and digest from the original authenticated
    /// bytes. The public index projection is not consulted. The title's fixed
    /// [`konaste_runtime::settings_path`] is looked up, so the launcher copy is
    /// only accepted for the four titles that publish no game copy. Bombergirl,
    /// a missing record, and an empty record are refused. This is not a path
    /// and not permission to start the program.
    pub fn settings_pin(&self) -> Result<konaste_runtime::SettingsPin, DiscoveryError> {
        let path = konaste_runtime::settings_path(self.family).map_err(map_runtime)?;
        let title = konaste_runtime::title(self.family).map_err(map_runtime)?;
        let index = konaste_manifest::decode(&self.bytes, self.sha256, &[title.host]).map_err(|_| DiscoveryError::InvalidMetadata)?;
        settings_pin_in(&index, path)
    }
}

fn settings_pin_in(index: &ResourceIndex, path: &str) -> Result<konaste_runtime::SettingsPin, DiscoveryError> {
    let file = index.files.iter().find(|file| file.path == path && file.stored_size > 0).ok_or(DiscoveryError::InvalidMetadata)?;
    Ok(konaste_runtime::SettingsPin { bytes: file.stored_size, sha256: file.stored_sha256 })
}

/// Blocking; call outside the GUI thread. Never downloads bootstrap packages.
///
/// A fresh worker owns a fresh scratch directory and pinned DLL copies. The
/// parent enforces a 45-second worker lifetime, then a separate 30-second index
/// transfer deadline. A failed check cannot return a retained successful result.
pub fn check(family: GameFamily, cache_root: &Path, worker_executable: &Path) -> Result<CurrentMetadata, DiscoveryError> {
    require_supported(family)?;
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        let location = konaste_runtime::discover(family, cache_root, worker_executable).map_err(map_runtime)?;
        resolve(location, family, fetch_index)
    }
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        let _ = (cache_root, worker_executable);
        Err(DiscoveryError::Unsupported)
    }
}

fn require_supported(family: GameFamily) -> Result<(), DiscoveryError> {
    konaste_runtime::discovery_profile(family).map(|_| ()).map_err(map_runtime)
}

fn map_runtime(error: RuntimeError) -> DiscoveryError {
    match error {
        RuntimeError::Unsupported => DiscoveryError::Unsupported,
        RuntimeError::ComponentsMissing => DiscoveryError::ComponentsMissing,
        RuntimeError::Runtime => DiscoveryError::Runtime,
        RuntimeError::Worker => DiscoveryError::Worker,
        RuntimeError::Timeout => DiscoveryError::Timeout,
        RuntimeError::Protocol => DiscoveryError::Protocol,
        RuntimeError::VerificationDenied
        | RuntimeError::ModuleMismatch
        | RuntimeError::GameModeRequired
        | RuntimeError::GameSettings
        | RuntimeError::GameStart
        | RuntimeError::GameWait => DiscoveryError::Protocol,
    }
}

fn checked_location(location: &ResourceLocation, family: GameFamily) -> Result<(Url, [u8; 32]), DiscoveryError> {
    require_supported(family)?;
    if location.family != family
        || location.url.len() >= 0x800
        || !location.url.is_ascii()
        || location.url.bytes().any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        || location.sha256.len() != 64
        || !location.sha256.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(DiscoveryError::Protocol);
    }
    let url =
        konaste_runtime::discovery_profile(family).map_err(map_runtime)?.resource_url(&location.url).ok_or(DiscoveryError::Protocol)?;
    let mut digest = [0; 32];
    for (i, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&location.sha256[2 * i..2 * i + 2], 16).map_err(|_| DiscoveryError::Protocol)?;
    }
    Ok((url, digest))
}

fn resolve(
    location: ResourceLocation,
    family: GameFamily,
    fetch: impl FnOnce(&Url) -> Result<Vec<u8>, DiscoveryError>,
) -> Result<CurrentMetadata, DiscoveryError> {
    let (url, digest) = checked_location(&location, family)?;
    let bytes = fetch(&url)?;
    let profile = konaste_runtime::discovery_profile(family).map_err(map_runtime)?;
    let index = konaste_manifest::decode(&bytes, digest, &[profile.host]).map_err(|_| DiscoveryError::InvalidMetadata)?;
    Ok(CurrentMetadata { index, sha256: digest, family, location: url, bytes })
}

fn fetch_index(url: &Url) -> Result<Vec<u8>, DiscoveryError> {
    let client = reqwest::blocking::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .user_agent("Konaste Hub/0.1.0")
        .build()
        .map_err(|_| DiscoveryError::Network)?;
    fetch_with(&client, url)
}

fn fetch_with(client: &reqwest::blocking::Client, url: &Url) -> Result<Vec<u8>, DiscoveryError> {
    let response = client
        .get(url.clone())
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .header(reqwest::header::CACHE_CONTROL, "no-cache, no-store")
        .send()
        .map_err(|_| DiscoveryError::Network)?;
    if response.status() != reqwest::StatusCode::OK
        || response.headers().contains_key(reqwest::header::CONTENT_RANGE)
        || response.headers().get(reqwest::header::CONTENT_ENCODING).is_some_and(|value| value.as_bytes() != b"identity")
        || response.content_length().is_some_and(|length| length > MAX_INDEX_BYTES as u64)
    {
        return Err(DiscoveryError::Network);
    }
    let mut bytes = Vec::new();
    response.take(MAX_INDEX_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|_| DiscoveryError::Network)?;
    if bytes.len() > MAX_INDEX_BYTES {
        return Err(DiscoveryError::InvalidMetadata);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location() -> ResourceLocation {
        ResourceLocation {
            family: GameFamily::Bombergirl,
            url: format!("https://{HOST}/ResourceInfo/synthetic/test.xml"),
            sha256: "a0".repeat(32),
        }
    }

    #[test]
    fn receipts_bind_protocol_title_digest_and_exact_https_source() {
        let value = location();
        let (url, hash) = checked_location(&value, GameFamily::Bombergirl).unwrap();
        assert_eq!(url.host_str(), Some(HOST));
        assert_eq!(hash, [0xa0; 32]);
        for bad in [
            format!("http://{HOST}/ResourceInfo/test.xml"),
            format!("https://{HOST}:444/ResourceInfo/test.xml"),
            format!("https://user@{HOST}/ResourceInfo/test.xml"),
            format!("https://{HOST}.example.test/ResourceInfo/test.xml"),
            format!("https://{HOST}/Installer/test.xml"),
            format!("https://{HOST}/ResourceInfo/test.xml?tk=not-a-token"),
            format!("https://{HOST}/ResourceInfo/test.xml#fragment"),
            format!(" https://{HOST}/ResourceInfo/test.xml"),
        ] {
            let mut value = location();
            value.url = bad;
            assert_eq!(checked_location(&value, GameFamily::Bombergirl).map(|_| ()), Err(DiscoveryError::Protocol));
        }
        let mut value = location();
        value.family = GameFamily::Sdvx;
        assert!(checked_location(&value, GameFamily::Bombergirl).is_err());
        value = location();
        value.sha256 = "AA".repeat(32);
        assert!(checked_location(&value, GameFamily::Bombergirl).is_err());
        value.sha256 = "aa".repeat(33);
        assert!(checked_location(&value, GameFamily::Bombergirl).is_err());
    }

    #[test]
    fn discovery_profiles_do_not_mix_metadata_hosts() {
        let mut value = location();
        value.family = GameFamily::Sdvx;
        assert_eq!(checked_location(&value, GameFamily::Sdvx), Err(DiscoveryError::Protocol));
        let profile = konaste_runtime::discovery_profile(GameFamily::Sdvx).unwrap();
        value.url = format!("https://{}/vi/downloads/resource_info/synthetic.xml", profile.host);
        assert_eq!(checked_location(&value, GameFamily::Sdvx).unwrap().1, [0xa0; 32]);
    }

    const NAME_ALPHABET: &[u8] = b"0123456789:ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuvwxyz";

    fn packed_name(text: &str) -> Vec<u8> {
        let mut packed = vec![0; (text.len() * 6).div_ceil(8)];
        for (position, character) in text.bytes().enumerate() {
            let index = NAME_ALPHABET.iter().position(|value| *value == character).unwrap();
            for bit in 0..6 {
                if index & (1 << (5 - bit)) != 0 {
                    let output = position * 6 + bit;
                    packed[output / 8] |= 1 << (7 - output % 8);
                }
            }
        }
        let mut encoded = vec![u8::try_from(text.len()).unwrap()];
        encoded.extend(packed);
        encoded
    }

    fn align(bytes: &mut Vec<u8>) {
        bytes.resize(bytes.len().next_multiple_of(4), 0);
    }

    fn open_node(nodes: &mut Vec<u8>, kind: u8, name: &str) {
        nodes.push(kind);
        nodes.extend(packed_name(name));
    }

    fn close_node(nodes: &mut Vec<u8>) {
        nodes.push(0xfe);
    }

    fn text_node(nodes: &mut Vec<u8>, data: &mut Vec<u8>, name: &str, value: &str) {
        open_node(nodes, 0x0b, name);
        align(data);
        data.extend(u32::try_from(value.len() + 1).unwrap().to_be_bytes());
        data.extend(value.as_bytes());
        data.push(0);
        align(data);
        close_node(nodes);
    }

    fn number_node(nodes: &mut Vec<u8>, data: &mut Vec<u8>, name: &str, value: u32) {
        open_node(nodes, 0x06, name);
        align(data);
        data.extend(value.to_be_bytes());
        close_node(nodes);
    }

    /// One fictional empty file. The encoder matches the observed wire subset;
    /// the bytes are generated here and are not an official index.
    fn synthetic_index() -> Vec<u8> {
        synthetic_index_for(GameFamily::Bombergirl, "data/empty.bin", 0)
    }

    fn synthetic_index_for(family: GameFamily, path: &str, size: u32) -> Vec<u8> {
        synthetic_index_with_files(family, &[(path, size)])
    }

    fn synthetic_index_with_files(family: GameFamily, files: &[(&str, u32)]) -> Vec<u8> {
        let title = konaste_runtime::title(family).unwrap();
        let sum = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let revision = 2026010100_u32;
        let mut nodes = Vec::new();
        let mut data = Vec::new();
        open_node(&mut nodes, 0x01, "resource_info");
        for &(path, size) in files {
            let url = format!("https://{}{}synthetic/{path}", title.host, title.content_prefix);
            open_node(&mut nodes, 0x01, "file");
            text_node(&mut nodes, &mut data, "path", path);
            number_node(&mut nodes, &mut data, "version", revision);
            number_node(&mut nodes, &mut data, "size", size);
            text_node(&mut nodes, &mut data, "sum", sum);
            text_node(&mut nodes, &mut data, "url", &url);
            close_node(&mut nodes);
        }
        number_node(&mut nodes, &mut data, "ri_ver", revision);
        close_node(&mut nodes);
        nodes.push(0xff);
        align(&mut nodes);
        let mut body = vec![0xa0, 0x42, 0x00, 0xff];
        body.extend(u32::try_from(nodes.len()).unwrap().to_be_bytes());
        body.extend(&nodes);
        body.extend(u32::try_from(data.len()).unwrap().to_be_bytes());
        body.extend(&data);
        body
    }

    #[test]
    fn resolve_retains_the_exact_body_accepted_by_decode() {
        let body = synthetic_index();
        let digest = {
            use sha2::{Digest, Sha256};
            let value: [u8; 32] = Sha256::digest(&body).into();
            value
        };
        let mut location = location();
        location.sha256 = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        let expected = body.clone();
        let metadata = resolve(location, GameFamily::Bombergirl, |_| Ok(body)).unwrap();
        assert_eq!(metadata.bytes(), expected.as_slice());
        assert_eq!(metadata.sha256, digest);
        assert_eq!(metadata.index.revision, 2026010100);
        assert_eq!(metadata.index.files.len(), 1);
        assert_eq!(metadata.index.files[0].path, "data/empty.bin");
        assert_eq!(metadata.index.files[0].stored_size, 0);
        assert_eq!(metadata.index.files[0].url.host_str(), Some(HOST));
    }

    #[test]
    fn installation_pin_uses_the_exact_body_not_a_mutable_projection() {
        use sha2::{Digest, Sha256};
        let body = synthetic_index_for(GameFamily::Bombergirl, "modules/bomber.exe", 12);
        let digest: [u8; 32] = Sha256::digest(&body).into();
        let mut location = location();
        location.sha256 = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        let mut metadata = resolve(location, GameFamily::Bombergirl, |_| Ok(body)).unwrap();
        assert_eq!(metadata.family(), GameFamily::Bombergirl);
        metadata.index.files[0].path = "modules/untrusted.exe".into();
        metadata.index.files[0].stored_size = 0;
        metadata.index.files[0].stored_sha256 = [42; 32];
        let pin = metadata.installation_pin().unwrap();
        assert_eq!(pin.index_sha256, digest);
        assert_eq!(pin.module_bytes, 12);
        let empty_digest: [u8; 32] = Sha256::digest(b"").into();
        assert_eq!(pin.module_sha256, empty_digest);
        metadata.sha256 = [0; 32];
        assert_eq!(metadata.installation_pin(), Err(DiscoveryError::InvalidMetadata));
    }

    #[test]
    fn settings_pin_reads_only_the_sdvx_game_record_from_the_body() {
        use sha2::{Digest, Sha256};
        let body = synthetic_index_for(GameFamily::Sdvx, konaste_runtime::SDVX_SETTINGS_PATH, 24);
        let digest: [u8; 32] = Sha256::digest(&body).into();
        let profile = konaste_runtime::discovery_profile(GameFamily::Sdvx).unwrap();
        let location = ResourceLocation {
            family: GameFamily::Sdvx,
            url: format!("https://{}/vi/downloads/resource_info/synthetic.xml", profile.host),
            sha256: digest.iter().map(|byte| format!("{byte:02x}")).collect(),
        };
        let mut metadata = resolve(location, GameFamily::Sdvx, |_| Ok(body)).unwrap();
        metadata.index.files[0].path = "launcher/modules/settings.exe".into();
        metadata.index.files[0].stored_size = 99;
        metadata.index.files[0].stored_sha256 = [7; 32];
        let pin = metadata.settings_pin().unwrap();
        assert_eq!(pin.bytes, 24);
        let empty: [u8; 32] = Sha256::digest(b"").into();
        assert_eq!(pin.sha256, empty);
        assert_ne!(pin.sha256, [7; 32]);

        metadata.sha256 = [0; 32];
        assert_eq!(metadata.settings_pin(), Err(DiscoveryError::InvalidMetadata));
    }

    /// Synthetic index whose single record is `path`. The location uses the
    /// title's own resource prefix so it passes the receipt check.
    fn metadata_for(family: GameFamily, path: &str, size: u32) -> CurrentMetadata {
        metadata_with_files(family, &[(path, size)])
    }

    fn metadata_with_files(family: GameFamily, files: &[(&str, u32)]) -> CurrentMetadata {
        use sha2::{Digest, Sha256};
        let body = synthetic_index_with_files(family, files);
        let profile = konaste_runtime::discovery_profile(family).unwrap();
        let location = ResourceLocation {
            family,
            url: match family {
                GameFamily::Sdvx => format!("https://{}/vi/downloads/resource_info/synthetic.xml", profile.host),
                GameFamily::Bombergirl | GameFamily::Popn => format!("https://{}/ResourceInfo/synthetic.xml", profile.host),
                GameFamily::Ddr => format!("https://{}/list/synthetic.xml", profile.host),
                GameFamily::Gitadora | GameFamily::Nostalgia => format!("https://{}/data/list/synthetic.xml", profile.host),
            },
            sha256: hex::encode(Sha256::digest(&body)),
        };
        resolve(location, family, |_| Ok(body)).unwrap()
    }

    #[test]
    fn installation_pins_include_only_the_expected_settings_record() {
        let family = GameFamily::Sdvx;
        let module = konaste_runtime::title(family).unwrap().manifest_module;
        let files = [(module, 12), (konaste_runtime::LAUNCHER_SETTINGS_PATH, 99)];
        let metadata = metadata_with_files(family, &files);
        assert!(metadata.installation_pins().unwrap().1.is_none());

        let mut metadata = metadata_with_files(family, &[files[0], files[1], (konaste_runtime::SDVX_SETTINGS_PATH, 24)]);
        metadata.index.files.clear();
        let (installation, settings) = metadata.installation_pins().unwrap();
        assert_eq!(installation.module_bytes, 12);
        assert_eq!(settings.unwrap().bytes, 24);
        assert_eq!(settings, Some(metadata.settings_pin().unwrap()));
    }

    #[test]
    fn launcher_titles_take_the_launcher_record_and_reject_the_game_copy() {
        for family in [GameFamily::Ddr, GameFamily::Popn, GameFamily::Gitadora, GameFamily::Nostalgia] {
            let metadata = metadata_for(family, konaste_runtime::LAUNCHER_SETTINGS_PATH, 40);
            let pin = metadata.settings_pin().unwrap();
            assert_eq!(pin.bytes, 40);

            // A launcher-title index that only has an SDVX-style game copy fails.
            let metadata = metadata_for(family, konaste_runtime::SDVX_SETTINGS_PATH, 40);
            assert_eq!(metadata.settings_pin(), Err(DiscoveryError::InvalidMetadata));
        }
    }

    #[test]
    fn settings_pin_refuses_other_titles_and_the_wrong_module() {
        for (family, path) in [
            (GameFamily::Sdvx, "launcher/modules/settings.exe"),
            (GameFamily::Sdvx, konaste_runtime::SDVX_SETTINGS_PATH),
            (GameFamily::Bombergirl, konaste_runtime::LAUNCHER_SETTINGS_PATH),
            (GameFamily::Bombergirl, "modules/bomber.exe"),
        ] {
            let size = if path == konaste_runtime::SDVX_SETTINGS_PATH { 0 } else { 16 };
            let metadata = metadata_for(family, path, size);
            assert_eq!(
                metadata.settings_pin(),
                Err(if family == GameFamily::Sdvx { DiscoveryError::InvalidMetadata } else { DiscoveryError::Unsupported })
            );
        }
    }

    #[test]
    fn installation_pin_refuses_a_missing_or_empty_title_module() {
        use sha2::{Digest, Sha256};
        for path in ["modules/bomber.exe", "data/empty.bin"] {
            let body = synthetic_index_for(GameFamily::Bombergirl, path, 0);
            let mut location = location();
            location.sha256 = hex::encode(Sha256::digest(&body));
            let metadata = resolve(location, GameFamily::Bombergirl, |_| Ok(body)).unwrap();
            assert_eq!(metadata.installation_pin(), Err(DiscoveryError::InvalidMetadata));
        }
    }

    #[test]
    fn metadata_digest_is_not_derived_from_the_retrieved_body() {
        let result = resolve(location(), GameFamily::Bombergirl, |_| Ok(b"untrusted body".to_vec()));
        assert!(matches!(result, Err(DiscoveryError::InvalidMetadata)));
        let mut value = location();
        value.url = "https://example.test/manifest.xml".into();
        let result = resolve(value, GameFamily::Bombergirl, |_| panic!("invalid receipt must not fetch"));
        assert!(matches!(result, Err(DiscoveryError::Protocol)));
    }

    #[test]
    fn transport_is_anonymous_bounded_identity_and_does_not_follow_redirects() {
        use std::{io::Write, net::TcpListener, thread};
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        for (status, headers, body, accepted) in [
            (200, "", "abc", true),
            (200, "Content-Encoding: gzip\r\n", "abc", false),
            (206, "", "abc", false),
            (302, "Location: http://127.0.0.1:1/never-follow\r\n", "", false),
            (503, "", "", false),
            (200, "Content-Range: bytes 0-2/3\r\n", "abc", false),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let url = Url::parse(&format!("http://{}/index", listener.local_addr().unwrap())).unwrap();
            let server = thread::spawn(move || {
                let started = std::time::Instant::now();
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && started.elapsed() < Duration::from_secs(3) => {
                            thread::sleep(Duration::from_millis(5))
                        }
                        Err(error) => panic!("loopback accept: {error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                let mut request = [0; 4096];
                let mut length = 0;
                while !request[..length].windows(4).any(|part| part == b"\r\n\r\n") {
                    assert!(length < request.len());
                    let count = stream.read(&mut request[length..]).unwrap();
                    assert!(count > 0);
                    length += count;
                }
                let request = std::str::from_utf8(&request[..length]).unwrap().to_ascii_lowercase();
                assert!(request.starts_with("get /index http/1.1\r\n"));
                assert!(request.contains("accept-encoding: identity"));
                assert!(request.contains("cache-control: no-cache, no-store"));
                assert!(!request.contains("\r\ncookie:"));
                assert!(!request.contains("\r\nauthorization:"));
                write!(stream, "HTTP/1.1 {status} Test\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                    .unwrap();
            });
            assert_eq!(fetch_with(&client, &url).is_ok(), accepted);
            server.join().unwrap();
        }
    }
}
