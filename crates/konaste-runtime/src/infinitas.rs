//! Local INFINITAS settings. This module does not use the installed index,
//! contact KONAMI, execute vendor code, or confer update or launch permission.

use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    path::{Path, PathBuf},
};

use roxmltree::{Document, Node, ParsingOptions};
use thiserror::Error;

pub const REGISTRY_KEY: &str = r"SOFTWARE\KONAMI\beatmania IIDX INFINITAS";
pub const GAME_IMAGE: &str = "game/app/bm2dx.exe";
pub const LAUNCHER_IMAGE: &str = "launcher/modules/bm2dx_launcher.exe";
pub const SETTINGS_IMAGE: &str = "launcher/modules/bm2dx_settings.exe";
pub const UPDATER_IMAGE: &str = "launcher/modules/bm2dx_updater.exe";

/// Registry presence is not evidence that a game is installed or up to date.
pub fn registered_install_dir() -> Option<PathBuf> {
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        crate::arguments::infinitas_paths().ok().map(|(install, _)| install)
    }
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        None
    }
}

#[path = "infinitas_setup.rs"]
pub mod setup;

const MAX_BYTES: usize = 256 * 1024;
const EMPTY: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n",
    "<SettingDatas xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" ",
    "xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\">\r\n",
    "  <Parameters>\r\n  </Parameters>\r\n</SettingDatas>\r\n",
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Setting {
    Audio,
    FrameRate,
    Led,
}
impl Setting {
    pub const ALL: [Self; 3] = [Self::Audio, Self::FrameRate, Self::Led];

    /// Choices exposed by the official settings UI, in their persisted form.
    pub fn accepts(self, value: i32) -> bool {
        match self {
            Self::Audio | Self::FrameRate => matches!(value, 0 | 1),
            Self::Led => matches!(value, 0..=5 | 999),
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Audio => "AudioPlaySettings",
            Self::FrameRate => "FPSSettings",
            Self::Led => "LEDSettings",
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SettingsError {
    #[error("the registered installation or its current-user settings path is unavailable")]
    Unavailable,
    #[error("invalid settings XML")]
    Invalid,
    #[error("the settings file or the registered paths changed since they were read")]
    Changed,
    #[error("could not save the settings file")]
    Write,
}

pub struct Settings {
    path: PathBuf,
    body: Option<Vec<u8>>,
    values: BTreeMap<Setting, i32>,
}
impl Settings {
    /// An absent key uses the official default. A malformed value is not
    /// displayed as the default, and is left untouched unless changed explicitly.
    pub fn value(&self, key: Setting) -> Option<i32> {
        self.values.get(&key).copied()
    }

    pub fn load(root: &Path) -> Result<Self, SettingsError> {
        #[cfg(all(windows, target_arch = "x86_64"))]
        {
            local::load(root)
        }
        #[cfg(not(all(windows, target_arch = "x86_64")))]
        {
            let _ = root;
            Err(SettingsError::Unavailable)
        }
    }

    pub fn save(&self, root: &Path, changes: &BTreeMap<Setting, i32>) -> Result<(), SettingsError> {
        #[cfg(all(windows, target_arch = "x86_64"))]
        {
            local::save(self, root, changes)
        }
        #[cfg(not(all(windows, target_arch = "x86_64")))]
        {
            let _ = (root, changes);
            Err(SettingsError::Unavailable)
        }
    }
}

#[cfg(any(test, all(windows, target_arch = "x86_64")))]
fn settings_path(resource: &Path, rid: u32) -> PathBuf {
    resource.join("config").join(format!("{rid}_cf.xml"))
}

fn element(node: Node<'_, '_>, name: &str) -> bool {
    node.has_tag_name(name) && node.tag_name().namespace().is_none() && node.attributes().len() == 0
}

/// The proven .NET XmlSerializer shape. Unknown rows are left in their exact
/// original representation, including text escaping, ordering and whitespace.
struct Xml<'a> {
    src: &'a str,
    parameters: Range<usize>,
    values: BTreeMap<Setting, (Option<i32>, Range<usize>)>,
}
impl<'a> Xml<'a> {
    fn parse(bytes: Option<&'a [u8]>) -> Result<Self, SettingsError> {
        use SettingsError::Invalid;
        let src = match bytes {
            Some(bytes) if bytes.len() <= MAX_BYTES => std::str::from_utf8(bytes).map_err(|_| Invalid)?,
            Some(_) => return Err(Invalid),
            None => EMPTY,
        };
        let doc = Document::parse_with_options(src, ParsingOptions { nodes_limit: 16_384, ..Default::default() }).map_err(|_| Invalid)?;
        let root = doc.root_element();
        if !element(root, "SettingDatas") {
            return Err(Invalid);
        }
        let [parameters] = element_children(root)?.try_into().map_err(|_| Invalid)?;
        if !element(parameters, "Parameters") {
            return Err(Invalid);
        }
        let mut values = BTreeMap::new();
        let mut seen_keys = BTreeSet::new();
        for pair in element_children(parameters)? {
            if !element(pair, "KeyValuePair") {
                return Err(Invalid);
            }
            let [key, value] = element_children(pair)?.try_into().map_err(|_| Invalid)?;
            let key = string_node(key, "Key")?;
            let value = string_node(value, "Value")?;
            let text = key.text().unwrap_or_default();
            if !seen_keys.insert(text) {
                return Err(Invalid);
            }
            if let Some(setting) = Setting::ALL.into_iter().find(|setting| setting.key() == text) {
                values.insert(setting, (value.text().unwrap_or_default().trim().parse().ok(), value.range()));
            }
        }
        Ok(Self { src, parameters: parameters.range(), values })
    }

    fn value(&self, setting: Setting) -> Option<i32> {
        self.values.get(&setting).map_or(Some(0), |(value, _)| *value)
    }

    fn patched(&self, changes: &BTreeMap<Setting, i32>) -> Result<String, SettingsError> {
        use std::fmt::Write as _;
        let mut edits = Vec::new();
        let mut added = String::new();
        for (&setting, &value) in changes {
            if !setting.accepts(value) {
                return Err(SettingsError::Invalid);
            }
            if let Some((_, range)) = self.values.get(&setting) {
                edits.push((range.clone(), format!("<string>{value}</string>")));
            } else {
                let key = setting.key();
                let _ = write!(
                    added,
                    "\r\n    <KeyValuePair><Key><string>{key}</string></Key><Value><string>{value}</string></Value></KeyValuePair>"
                );
            }
        }
        if !added.is_empty() {
            if let Some(end) = self.src[self.parameters.clone()].rfind("</Parameters") {
                let end = self.parameters.start + end;
                edits.push((end..end, format!("{added}\r\n  ")));
            } else {
                edits.push((self.parameters.clone(), format!("<Parameters>{added}\r\n  </Parameters>")));
            }
        }
        edits.sort_unstable_by_key(|(range, _)| range.start);
        let mut result = self.src.to_owned();
        for (range, replacement) in edits.into_iter().rev() {
            result.replace_range(range, &replacement);
        }
        if result.len() > MAX_BYTES {
            return Err(SettingsError::Invalid);
        }
        Ok(result)
    }
}

fn element_children<'a, 'input>(parent: Node<'a, 'input>) -> Result<Vec<Node<'a, 'input>>, SettingsError> {
    let mut elements = Vec::new();
    for child in parent.children() {
        if child.is_element() {
            elements.push(child);
        } else if !child.is_comment() && !child.text().is_some_and(|text| text.trim().is_empty()) {
            return Err(SettingsError::Invalid);
        }
    }
    Ok(elements)
}

fn string_node<'a, 'input>(parent: Node<'a, 'input>, name: &str) -> Result<Node<'a, 'input>, SettingsError> {
    let [text] = element_children(parent)?.try_into().map_err(|_| SettingsError::Invalid)?;
    if !element(parent, name) || !element(text, "string") || !text.children().all(|child| child.is_text()) {
        return Err(SettingsError::Invalid);
    }
    Ok(text)
}

#[cfg(all(windows, target_arch = "x86_64"))]
mod local {
    use super::*;
    use crate::{
        arguments::{infinitas_paths, read_named_body},
        official::pin_root,
    };
    use std::{
        fs,
        io::Write,
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    };
    use windows_sys::Win32::{
        Security::{GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, IsValidSid, TOKEN_QUERY, TOKEN_USER, TokenUser},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    fn current_rid() -> Result<u32, SettingsError> {
        let mut token = std::ptr::null_mut();
        // SAFETY: valid pseudo-handle and writable storage for the new token.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(SettingsError::Unavailable);
        }
        // SAFETY: OpenProcessToken returned an owned kernel handle.
        let token = unsafe { OwnedHandle::from_raw_handle(token) };
        let mut size = 0;
        // SAFETY: a zero-size query obtains the required buffer length.
        unsafe { GetTokenInformation(token.as_raw_handle(), TokenUser, std::ptr::null_mut(), 0, &mut size) };
        if !(std::mem::size_of::<TOKEN_USER>() as u32..=4096).contains(&size) {
            return Err(SettingsError::Unavailable);
        }
        let mut buffer = vec![0_usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
        // SAFETY: buffer is aligned for TOKEN_USER and has at least size writable bytes.
        unsafe {
            if GetTokenInformation(token.as_raw_handle(), TokenUser, buffer.as_mut_ptr().cast(), size, &mut size) == 0 {
                return Err(SettingsError::Unavailable);
            }
            let sid = (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid;
            if sid.is_null() || IsValidSid(sid) == 0 {
                return Err(SettingsError::Unavailable);
            }
            let count = *GetSidSubAuthorityCount(sid);
            let index = count.checked_sub(1).ok_or(SettingsError::Unavailable)?;
            Ok(*GetSidSubAuthority(sid, u32::from(index)))
        }
    }

    fn location(root: &Path) -> Result<(PathBuf, PathBuf), SettingsError> {
        let (install, resource) = infinitas_paths().map_err(|_| SettingsError::Unavailable)?;
        let _install_pins = pin_root(&install).map_err(|_| SettingsError::Unavailable)?;
        if install.canonicalize().map_err(|_| SettingsError::Unavailable)? != root || !install.join(GAME_IMAGE).is_file() {
            return Err(SettingsError::Unavailable);
        }
        Ok((settings_path(&resource, current_rid()?), resource))
    }

    pub(super) fn load(root: &Path) -> Result<Settings, SettingsError> {
        let (path, resource) = location(root)?;
        let _resource_pins = pin_root(&resource).map_err(|_| SettingsError::Unavailable)?;
        let body = read_named_body(&path).map_err(|_| SettingsError::Unavailable)?;
        let xml = Xml::parse(body.as_deref())?;
        let values = Setting::ALL.into_iter().filter_map(|key| Some((key, xml.value(key)?))).collect();
        Ok(Settings { path, body, values })
    }

    pub(super) fn save(settings: &Settings, root: &Path, changes: &BTreeMap<Setting, i32>) -> Result<(), SettingsError> {
        if changes.is_empty() {
            return Ok(());
        }
        let (path, resource) = location(root)?;
        if path != settings.path {
            return Err(SettingsError::Changed);
        }
        write_settings(settings, &resource, changes)
    }

    fn write_settings(settings: &Settings, resource: &Path, changes: &BTreeMap<Setting, i32>) -> Result<(), SettingsError> {
        let path = &settings.path;
        let bytes = Xml::parse(settings.body.as_deref())?.patched(changes)?;
        let _resource_pins = pin_root(resource).map_err(|_| SettingsError::Unavailable)?;
        let config = path.parent().ok_or(SettingsError::Unavailable)?;
        if settings.body.is_none() {
            match fs::create_dir(config) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err(SettingsError::Write),
            }
        }
        let _config_pins = pin_root(config).map_err(|_| SettingsError::Unavailable)?;
        let mut temporary = tempfile::NamedTempFile::new_in(config).map_err(|_| SettingsError::Write)?;
        temporary.write_all(bytes.as_bytes()).and_then(|()| temporary.as_file().sync_all()).map_err(|_| SettingsError::Write)?;
        let current = read_named_body(path).map_err(|_| SettingsError::Unavailable)?;
        if current != settings.body {
            return Err(SettingsError::Changed);
        }
        if current.is_some() {
            temporary.persist(path).map_err(|_| SettingsError::Write)?;
        } else {
            temporary.persist_noclobber(path).map_err(|error| {
                if error.error.kind() == std::io::ErrorKind::AlreadyExists { SettingsError::Changed } else { SettingsError::Write }
            })?;
        }
        Ok(())
    }

    #[test]
    fn save_does_not_replace_a_hardlinked_settings_file() {
        let fixture = tempfile::tempdir().unwrap();
        let resource = fixture.path();
        let path = settings_path(resource, 1001);
        fs::create_dir(path.parent().unwrap()).unwrap();
        let other = resource.join("other.xml");
        fs::write(&other, EMPTY).unwrap();
        fs::hard_link(&other, &path).unwrap();
        let body = fs::read(&path).unwrap();
        let opened = Settings { path: path.clone(), body: Some(body.clone()), values: BTreeMap::new() };
        let edit = BTreeMap::from([(Setting::Audio, 1)]);
        assert_eq!(write_settings(&opened, resource, &edit), Err(SettingsError::Unavailable));
        assert_eq!(fs::read(&path).unwrap(), body);
        assert_eq!(fs::read(&other).unwrap(), body);
    }

    #[test]
    fn save_creates_only_current_rid_file_and_keeps_newer_bytes() {
        let fixture = tempfile::tempdir().unwrap();
        let resource = fixture.path();
        let path = settings_path(resource, 1001);
        let other = settings_path(resource, 1002);
        let edit = BTreeMap::from([(Setting::Audio, 1)]);
        let initial = Settings { path: path.clone(), body: None, values: BTreeMap::new() };
        write_settings(&initial, resource, &edit).unwrap();
        assert!(!other.exists());
        let saved = fs::read(&path).unwrap();
        assert_eq!(Xml::parse(Some(&saved)).unwrap().value(Setting::Audio), Some(1));
        assert_eq!(write_settings(&initial, resource, &edit), Err(SettingsError::Changed));
        let opened = Settings { body: Some(saved), ..initial };
        fs::write(&path, EMPTY).unwrap();
        assert_eq!(write_settings(&opened, resource, &edit), Err(SettingsError::Changed));
        assert_eq!(fs::read(&path).unwrap(), EMPTY.as_bytes());
        let latest = Settings { body: Some(EMPTY.as_bytes().to_vec()), ..opened };
        write_settings(&latest, resource, &edit).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert_eq!(Xml::parse(Some(&bytes)).unwrap().value(Setting::Audio), Some(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FUTURE: &str = "<KeyValuePair><Key><string>FutureKey</string></Key><Value><string>x &amp; y</string></Value></KeyValuePair>";
    const BINDING: &str = "<KeyValuePair><Key><string>KeyConfig1P_1</string></Key><Value><string>57</string></Value></KeyValuePair>";

    #[test]
    fn editing_one_key_preserves_bindings_and_unknown_rows() {
        let body = format!("\u{feff}{}", EMPTY.replace("</Parameters>", &format!("{FUTURE}{BINDING}</Parameters>")));
        let xml = Xml::parse(Some(body.as_bytes())).unwrap();
        assert_eq!(xml.value(Setting::FrameRate), Some(0));
        let changed = xml.patched(&BTreeMap::from([(Setting::FrameRate, 1)])).unwrap();
        assert!(changed.starts_with('\u{feff}'));
        assert!(changed.contains(FUTURE) && changed.contains(BINDING));
        let xml = Xml::parse(Some(changed.as_bytes())).unwrap();
        assert_eq!(xml.value(Setting::FrameRate), Some(1));
        let changed_again = xml.patched(&BTreeMap::from([(Setting::FrameRate, 0)])).unwrap();
        assert_eq!(changed_again, changed.replace("<string>1</string>", "<string>0</string>"));
    }

    #[test]
    fn empty_read_is_default_and_a_self_closing_array_can_be_extended() {
        let empty = b"<SettingDatas><Parameters /></SettingDatas>";
        for body in [None, Some(empty.as_slice())] {
            let xml = Xml::parse(body).unwrap();
            assert_eq!(xml.value(Setting::Audio), Some(0));
            let changed = xml.patched(&BTreeMap::from([(Setting::Audio, 1)])).unwrap();
            let xml = Xml::parse(Some(changed.as_bytes())).unwrap();
            assert_eq!(xml.value(Setting::Audio), Some(1));
        }
    }

    #[test]
    fn dtd_invalid_shapes_and_duplicate_owned_keys_are_rejected() {
        let key = "<KeyValuePair><Key><string>FPSSettings</string></Key><Value><string>0</string></Value></KeyValuePair>";
        let duplicates = EMPTY.replace("</Parameters>", &format!("{key}{key}</Parameters>"));
        for text in [
            "<!DOCTYPE SettingDatas [<!ENTITY x SYSTEM 'file:///sample'>]><SettingDatas><Parameters/></SettingDatas>",
            "<SettingDatas><Parameters/><Parameters/></SettingDatas>",
            "<SettingDatas><Parameters><Invalid/></Parameters></SettingDatas>",
            &duplicates,
        ] {
            assert!(matches!(Xml::parse(Some(text.as_bytes())), Err(SettingsError::Invalid)));
        }
    }

    #[test]
    fn the_rid_selects_only_the_current_windows_users_file() {
        let resource = tempfile::tempdir().unwrap();
        let other = settings_path(resource.path(), 1000);
        std::fs::create_dir(other.parent().unwrap()).unwrap();
        std::fs::write(&other, EMPTY).unwrap();
        let current = settings_path(resource.path(), 1001);
        assert_eq!(current.file_name().unwrap(), "1001_cf.xml");
        assert!(!current.exists());
    }
}
