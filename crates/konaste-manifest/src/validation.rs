use url::{Host, Url};

use crate::IndexError;

const MAX_PATH_BYTES: usize = 1024;
const MAX_SEGMENT_BYTES: usize = 255;
const MAX_HOSTS: usize = 64;
const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1",
    "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub(crate) fn checked_path(path: &str) -> Result<(), IndexError> {
    if path.is_empty()
        || path.len() > MAX_PATH_BYTES
        || !path.is_ascii()
        || path.bytes().any(|byte| byte.is_ascii_control() || b"<>:\"\\|?*~".contains(&byte))
    {
        return Err(IndexError::UnsafePath);
    }
    for (position, part) in path.split('/').enumerate() {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.len() > MAX_SEGMENT_BYTES
            || part.starts_with(' ')
            || part.ends_with(['.', ' '])
        {
            return Err(IndexError::UnsafePath);
        }
        let stem = part.split('.').next().unwrap_or("").trim_end_matches(' ');
        if RESERVED_NAMES.iter().any(|name| stem.eq_ignore_ascii_case(name)) || (position == 0 && part.eq_ignore_ascii_case(".konaste-hub"))
        {
            return Err(IndexError::UnsafePath);
        }
    }
    Ok(())
}

pub(crate) fn checked_hash(text: &str) -> Result<[u8; 32], IndexError> {
    if text.len() != 64 {
        return Err(IndexError::Schema("file SHA-256 is not 64 lowercase hex characters"));
    }
    fn nibble(byte: u8) -> Result<u8, IndexError> {
        match byte {
            b'0'..=b'9' => Ok(byte - b'0'),
            b'a'..=b'f' => Ok(byte - b'a' + 10),
            _ => Err(IndexError::Schema("file SHA-256 is not 64 lowercase hex characters")),
        }
    }
    let mut hash = [0; 32];
    for (position, pair) in text.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        hash[position] = nibble(pair[0])? * 16 + nibble(pair[1])?;
    }
    Ok(hash)
}

pub(crate) struct AllowedHosts(Vec<String>);

impl AllowedHosts {
    pub(crate) fn new(hosts: &[&str]) -> Result<Self, IndexError> {
        if hosts.is_empty() || hosts.len() > MAX_HOSTS {
            return Err(IndexError::InvalidAllowedHosts);
        }
        let mut checked = Vec::new();
        for host in hosts {
            if host.is_empty()
                || host.len() > 253
                || !host.is_ascii()
                || host.split('.').any(|label| {
                    label.is_empty()
                        || label.len() > 63
                        || label.starts_with('-')
                        || label.ends_with('-')
                        || !label.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                })
            {
                return Err(IndexError::InvalidAllowedHosts);
            }
            let parsed = Url::parse(&format!("https://{host}/")).map_err(|_| IndexError::InvalidAllowedHosts)?;
            if !matches!(parsed.host(), Some(Host::Domain(_))) || !parsed.host_str().is_some_and(|value| value.eq_ignore_ascii_case(host)) {
                return Err(IndexError::InvalidAllowedHosts);
            }
            let normalized = host.to_ascii_lowercase();
            if !checked.contains(&normalized) {
                checked.push(normalized);
            }
        }
        Ok(Self(checked))
    }

    pub(crate) fn url(&self, raw: &str) -> Result<Url, IndexError> {
        if raw.bytes().any(|byte| byte.is_ascii_whitespace() || byte == b'\\') {
            return Err(IndexError::UnsafeUrl);
        }
        let authority = raw.split_once("://").map(|(_, rest)| rest.split('/').next().unwrap_or("")).ok_or(IndexError::UnsafeUrl)?;
        if authority.contains('@') {
            return Err(IndexError::UnsafeUrl);
        }
        let url = Url::parse(raw).map_err(|_| IndexError::UnsafeUrl)?;
        if url.scheme() != "https"
            || url.port_or_known_default() != Some(443)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !matches!(url.host(), Some(Host::Domain(_)))
            || !url.host_str().is_some_and(|host| self.0.iter().any(|allowed| host == allowed))
        {
            return Err(IndexError::UnsafeUrl);
        }
        Ok(url)
    }
}
