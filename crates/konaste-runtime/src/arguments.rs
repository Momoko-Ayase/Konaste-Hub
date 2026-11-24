//! Prepared graphics prefix for one title's game parameters.
//!
//! `Token` titles emit nothing before the fixed ` -t ` token. `DdrSettings` reads
//! the fixed `HKLM\SOFTWARE\KONAMI\DanceDanceRevolution` `ResourceDir` value and
//! `config\settings_cf.xml` beneath it, then emits only the proven display, FPS,
//! and window flags. `GitadoraSettings` reads `ResourceDir` under the fixed
//! `SOFTWARE\KONAMI\GITADORA` key: `HKCU` first, and `HKLM` only when that key
//! cannot be opened. An opened `HKCU` key with a missing or malformed value does
//! not fall back. It then reads exactly one `config\*_cf.xml`. DDR never opens
//! `HKCU`. This module never writes the registry, a config file, or an
//! installation. A missing registry value or a missing file is the normal empty
//! map (` --display0 --fps60` for DDR, ` -display0 -window` for GITADORA).
//! pop'n uses its official `InstallDir`/`ResourceDir` only when `InstallDir`
//! names the verified install root. Absent, partial, or stale values (a fresh
//! Hub install, or a moved install) derive both paths from the verified root
//! instead of failing, without writing the registry. An unsafe path, a malformed
//! existing file, more than one settings file, or a
//! format other than the proven text XML is [`RuntimeError::GameSettings`].
//!
//! The AVS reader also accepts a binary property image and a JSON body. No normal
//! writer for those encodings is proven, so an existing non-XML body fails closed.
//! Registry and monitor lookup go through a private seam. Tests supply an owned
//! temporary config and never open the host registry or a user configuration.
//! GITADORA's instrument flag is [`crate::GitadoraMode`], never a config string.

use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::RuntimeError;
use crate::title::ArgumentProfile;

/// Ordinal 245 stores a signed 32-bit result. Values outside that range saturate.
const I32_MIN: i64 = i32::MIN as i64;
const I32_MAX: i64 = i32::MAX as i64;
/// Official `RegQueryValueExA` cap is 260 bytes, which can hold 259 ASCII
/// characters plus a NUL. The wide read uses the same character cap: 260
/// UTF-16 units, including the NUL. That is 520 bytes, not 260 bytes.
const RESOURCE_DIR_UNITS: usize = 260;
/// Ordinal 176 caps each string at 64 bytes. The NUL is included in that cap.
const VALUE_CAP: usize = 63;
/// Chosen body bound, not an observed settings-file length. Larger existing
/// files are rejected rather than truncated.
const SETTINGS_BYTES: usize = 256 * 1024;
const MAX_DEPTH: usize = 8;
const MAX_EVENTS: usize = 4096;
/// `GetSystemMetrics(SM_CMONITORS)`.
const SM_CMONITORS: i32 = 80;

const DISPLAY: &str = "DisplaySettings";
const FPS: &str = "FPSSettings";
const WINDOW: &str = "WindowSettings";
const RESOLUTION: &str = "ResoSettings";
const AUDIO: &str = "AudioPlaySettings";
const DDR_KNOWN: [&str; 3] = [DISPLAY, FPS, WINDOW];
const GITA_KNOWN: [&str; 4] = [DISPLAY, WINDOW, RESOLUTION, AUDIO];

const SETTING_DATAS: &str = "SettingDatas";
const PARAMETERS: &str = "Parameters";
const PAIR: &str = "KeyValuePair";
const KEY: &str = "Key";
const VALUE: &str = "Value";
const STRING: &str = "string";

/// Graphics flags prepared before game creation. The token is not stored here.
pub(crate) struct Arguments {
    prefix: String,
    /// Bare typed flag `-gf` or `-dm`, with no leading space. Empty otherwise.
    suffix: &'static str,
}

impl Arguments {
    /// Load one title profile. `mode` is required only for GITADORA and is
    /// refused for every other profile before any registry or file lookup.
    /// `Token` does not touch the filesystem or registry. DDR with no mode still
    /// reads its fixed registry value. A missing GITADORA mode returns before that.
    /// pop'n reads its `ResourceDir`, falling back to the install root when no
    /// official value is bound, and the installation's `game\dev\setting.bin`.
    pub(crate) fn load(profile: ArgumentProfile, mode: Option<crate::GitadoraMode>, root: &Path) -> Result<Self, RuntimeError> {
        let (prefix, suffix) = match (profile, mode) {
            (ArgumentProfile::Token, None) => (String::new(), ""),
            (ArgumentProfile::PopnBootstrap, None) => (popn_prefix(&production_source(), root)?, ""),
            (ArgumentProfile::DdrSettings, None) => (ddr_prefix(&production_source())?, ""),
            (ArgumentProfile::GitadoraSettings, None) => {
                return Err(RuntimeError::GameModeRequired);
            }
            (ArgumentProfile::GitadoraSettings, Some(mode)) => {
                // `argument()` is the bare flag. The leading space is added here so
                // the parameter join stays ` -t <token> -gf` or ` -dm`.
                let flag = mode.argument();
                if flag != "-gf" && flag != "-dm" {
                    return Err(RuntimeError::Protocol);
                }
                (gitadora_prefix(&production_source())?, flag)
            }
            (ArgumentProfile::Token | ArgumentProfile::DdrSettings | ArgumentProfile::PopnBootstrap, Some(_)) => {
                return Err(RuntimeError::Protocol);
            }
        };
        Ok(Self { prefix, suffix })
    }

    /// Closed graphics text. Empty for a token profile.
    /// The token, instrument flag, and any raw argv stay out of this string.
    pub(crate) fn prefix(&self) -> String {
        self.prefix.clone()
    }

    /// Typed instrument flag (`-gf` or `-dm`), or empty. Never a caller string.
    pub(crate) fn suffix(&self) -> &'static str {
        self.suffix
    }
}

struct Settings {
    display: i32,
    fps: i32,
    window: i32,
}

impl Settings {
    fn missing() -> Self {
        Self { display: 0, fps: 0, window: 0 }
    }
}

/// Where `ResourceDir` and the monitor count come from. Production uses the
/// fixed HKLM value and `GetSystemMetrics`. Tests own both.
trait SettingsSource {
    fn resource_dir(&self, subkey: &str) -> Result<Option<PathBuf>, RuntimeError>;
    fn install_dir(&self, _subkey: &str) -> Result<Option<PathBuf>, RuntimeError> {
        Ok(None)
    }
    fn monitor_count(&self) -> Result<i32, RuntimeError>;
}

struct ProductionSource;

fn production_source() -> ProductionSource {
    ProductionSource
}

impl SettingsSource for ProductionSource {
    fn resource_dir(&self, subkey: &str) -> Result<Option<PathBuf>, RuntimeError> {
        read_resource_dir(subkey)
    }

    fn install_dir(&self, subkey: &str) -> Result<Option<PathBuf>, RuntimeError> {
        lookup_value(&LiveRegistry, resource_order(subkey), subkey, "InstallDir")
    }

    fn monitor_count(&self) -> Result<i32, RuntimeError> {
        live_monitor_count()
    }
}

const DDR_KEY: &str = r"SOFTWARE\KONAMI\DanceDanceRevolution";
const GITA_KEY: &str = r"SOFTWARE\KONAMI\GITADORA";
const POPN_KEY: &str = r"SOFTWARE\KONAMI\pop'n music Lively";

/// pop'n's official reporter (`errorreporter.exe` U39:J:E:A:2026031100,
/// `0x140001690`) does not start the game with the token alone. It builds
///
/// `dev/LocalProp/bootstrap.local.xml debug_localstrap "<I>/x" "<R>/work" [-full ]<cf> -t <token>`
///
/// `<I>` is `InstallDir` and `<R>` is `ResourceDir` (`HKCU`, else `HKLM`), each
/// with every `\` replaced by `/` and joined with no separator, so the stored
/// trailing backslash supplies it. When the official values are absent — a
/// fresh Hub install whose Settings have never run — both are derived from the
/// verified install root (`<root>\` and `<root>\Resource\`) instead. Either way
/// these are read-only: this function never writes the registry. The settings
/// program's four-byte `game\dev\setting.bin` (read relative to the install
/// root) decides the rest: byte 0 nonzero is window mode, otherwise ` -full `
/// is inserted; byte 1 nonzero appends `-cf`. A missing or short file is window
/// mode with no filter. The game (`sub_5A5400`) exits with
/// 5-1590-0000 when it has fewer than five arguments. The token join is
/// unchanged: this prefix, then ` -t <token>`.
/// The work directory and launch arguments must use the same ResourceDir.
pub fn popn_resource_dir(root: &Path) -> Result<PathBuf, RuntimeError> {
    popn_paths(&production_source(), root).map(|(_, resource)| resource)
}

fn popn_paths(source: &impl SettingsSource, root: &Path) -> Result<(PathBuf, PathBuf), RuntimeError> {
    let install = source.install_dir(POPN_KEY)?;
    let resource = source.resource_dir(POPN_KEY)?;
    // Official values are used only when they are bound to this verified root.
    // A stale registration for a moved or different install would otherwise
    // point the game at the wrong directory. `InstallDir` decides the binding;
    // `ResourceDir` then comes from the same official key.
    let (install, resource) = match (install, resource) {
        (Some(install), Some(resource)) if same_install(&install, root) => (install, resource),
        // Absent, partial, or bound to another root: derive from this one. This
        // is the fresh Hub-install case and it writes nothing.
        _ => derived_root_paths(root)?,
    };
    Ok((validated_resource_dir(&install)?, validated_resource_dir(&resource)?))
}

fn popn_prefix(source: &impl SettingsSource, root: &Path) -> Result<String, RuntimeError> {
    let (install, resource) = popn_paths(source, root)?;
    let install = slash_form(&install)?;
    let resource = slash_form(&resource)?;
    let flags = read_popn_flags(root)?;
    let mut prefix = format!("dev/LocalProp/bootstrap.local.xml debug_localstrap \"{install}x\" \"{resource}work\"");
    if !flags.window {
        prefix.push_str(" -full");
    }
    if flags.chattering_filter {
        prefix.push_str(" -cf");
    }
    Ok(prefix)
}

/// Same install root, ignoring a canonical `\\?\` prefix, case, and a trailing
/// separator. Read-only and allocation-bounded by the two path strings.
fn same_install(a: &Path, b: &Path) -> bool {
    fn normal(path: &Path) -> Option<String> {
        let text = path.to_str()?;
        let text = text.strip_prefix(r"\\?\").unwrap_or(text);
        Some(text.trim_end_matches(['\\', '/']).replace('\\', "/").to_ascii_lowercase())
    }
    matches!((normal(a), normal(b)), (Some(a), Some(b)) if a == b)
}

/// `InstallDir` and `ResourceDir` for the verified install root: `<root>\` and
/// `<root>\Resource\`. The canonical `\\?\` spelling is reduced to its plain
/// drive form first. Validation is the same as a registry value.
fn derived_root_paths(root: &Path) -> Result<(PathBuf, PathBuf), RuntimeError> {
    let text = root.to_str().ok_or(RuntimeError::GameSettings)?;
    let plain = PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(text));
    let install = validated_resource_dir(&plain)?;
    let resource = validated_resource_dir(&install.join("Resource"))?;
    Ok((install, resource))
}

/// `E:\games\pop'n music Lively\` to `E:/games/pop'n music Lively/`. A quote or
/// control character cannot be carried inside the reporter's quoting.
fn slash_form(path: &Path) -> Result<String, RuntimeError> {
    let text = path.to_str().ok_or(RuntimeError::GameSettings)?;
    if text.contains('"') || text.chars().any(char::is_control) {
        return Err(RuntimeError::GameSettings);
    }
    let mut text = text.replace('\\', "/");
    if !text.ends_with('/') {
        text.push('/');
    }
    Ok(text)
}

struct PopnFlags {
    window: bool,
    chattering_filter: bool,
}

/// `game\dev\setting.bin` under the installation, as the reporter reads it.
fn read_popn_flags(root: &Path) -> Result<PopnFlags, RuntimeError> {
    // The installation root is canonical (`\\?\E:\...`); read through its plain
    // drive form so the ancestor checks see ordinary components.
    let text = root.to_str().ok_or(RuntimeError::GameSettings)?;
    let plain = PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(text));
    let root = validated_resource_dir(&plain)?;
    let body = read_named_body(&root.join("game").join("dev").join("setting.bin"))?;
    let Some(body) = body else {
        return Ok(PopnFlags { window: true, chattering_filter: false });
    };
    if body.len() > 64 {
        return Err(RuntimeError::GameSettings);
    }
    if body.len() < 4 {
        // The reporter's read fails and it keeps its defaults.
        return Ok(PopnFlags { window: true, chattering_filter: false });
    }
    Ok(PopnFlags { window: body[0] != 0, chattering_filter: body[1] != 0 })
}

fn ddr_prefix(source: &impl SettingsSource) -> Result<String, RuntimeError> {
    let settings = match source.resource_dir(DDR_KEY)? {
        None => Settings::missing(),
        Some(root) => read_fixed_settings(&validated_resource_dir(&root)?)?,
    };
    let monitors = source.monitor_count()?;
    Ok(render_ddr(&settings, monitors))
}

fn gitadora_prefix(source: &impl SettingsSource) -> Result<String, RuntimeError> {
    let settings = match source.resource_dir(GITA_KEY)? {
        None => GitaSettings::missing(),
        Some(root) => read_gitadora_settings(&validated_resource_dir(&root)?)?,
    };
    let monitors = source.monitor_count()?;
    Ok(render_gitadora(&settings, monitors))
}

fn render_ddr(settings: &Settings, monitors: i32) -> String {
    let mut prefix = String::new();
    prefix.push_str(" --display");
    prefix.push_str(&clamped_display(settings.display, monitors).to_string());
    match settings.fps {
        0 => prefix.push_str(" --fps60"),
        1 => prefix.push_str(" --fps120"),
        _ => {}
    }
    match settings.window {
        1 => prefix.push_str(" --fullscreen"),
        2 => prefix.push_str(" --borderless"),
        _ => {}
    }
    prefix
}

/// Negative display becomes 0. A value at or above the monitor count becomes
/// count minus one. Zero monitors is not a usable index.
fn clamped_display(display: i32, monitors: i32) -> i32 {
    if monitors <= 0 {
        return 0;
    }
    if display < 0 {
        0
    } else if display >= monitors {
        monitors - 1
    } else {
        display
    }
}

fn validated_resource_dir(root: &Path) -> Result<PathBuf, RuntimeError> {
    if !root.is_absolute() {
        return Err(RuntimeError::GameSettings);
    }
    let mut normalized = PathBuf::new();
    for part in root.components() {
        match part {
            Component::Prefix(prefix) => {
                if !matches!(prefix.kind(), std::path::Prefix::Disk(_)) {
                    return Err(RuntimeError::GameSettings);
                }
                normalized.push(part.as_os_str());
            }
            Component::RootDir => normalized.push(part.as_os_str()),
            Component::Normal(name) => {
                let text = name.to_string_lossy();
                if text.is_empty() || text == "." || text == ".." || text.contains(':') || text.contains('/') || text.contains('\\') {
                    return Err(RuntimeError::GameSettings);
                }
                normalized.push(name);
            }
            Component::CurDir | Component::ParentDir => return Err(RuntimeError::GameSettings),
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err(RuntimeError::GameSettings);
    }
    let text = normalized.to_string_lossy();
    if text.starts_with(r"\\") || text.contains('\0') {
        return Err(RuntimeError::GameSettings);
    }
    require_fixed_drive(&normalized)?;
    Ok(normalized)
}

/// `GetDriveType` must be a local fixed disk. A mapped network share uses a
/// drive letter and would otherwise look like an absolute path.
fn require_fixed_drive(path: &Path) -> Result<(), RuntimeError> {
    use std::os::windows::ffi::OsStrExt;
    /// `DRIVE_FIXED`.
    const DRIVE_FIXED: u32 = 3;
    let text = path.to_string_lossy();
    let mut chars = text.chars();
    let Some(letter) = chars.next() else {
        return Err(RuntimeError::GameSettings);
    };
    if !letter.is_ascii_alphabetic() || chars.next() != Some(':') {
        return Err(RuntimeError::GameSettings);
    }
    let root = format!(r"{letter}:\");
    let wide: Vec<u16> = std::ffi::OsStr::new(&root).encode_wide().chain(Some(0)).collect();
    // SAFETY: `wide` is a NUL-terminated drive root. The call only classifies it.
    let kind = unsafe { windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(wide.as_ptr()) };
    if kind == DRIVE_FIXED { Ok(()) } else { Err(RuntimeError::GameSettings) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hive {
    CurrentUser,
    LocalMachine,
}

/// DDR opens `HKLM` only. GITADORA opens `HKCU` and tries `HKLM` only when that
/// open fails. The order is chosen once; the loop below does not merge values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HiveOrder {
    LocalMachine,
    CurrentUserThenLocalMachine,
}

fn resource_order(subkey: &str) -> HiveOrder {
    if subkey == GITA_KEY || subkey == POPN_KEY { HiveOrder::CurrentUserThenLocalMachine } else { HiveOrder::LocalMachine }
}

enum KeyOpen<K> {
    Opened(K),
    /// `missing` is the not-found error. Any other open error is also a failure.
    Failed {
        missing: bool,
    },
}

enum ValueQuery {
    Present(String),
    Missing,
    Rejected,
}

trait RegistryLookup {
    type Key;
    fn open_key(&self, hive: Hive, subkey: &str) -> KeyOpen<Self::Key>;
    fn query_value(&self, key: &Self::Key, name: &str) -> ValueQuery;
}

/// One selection loop for both titles. A failed open tries the next hive. An
/// opened key never does: a missing value is the normal empty map and a bad
/// value is [`RuntimeError::GameSettings`].
fn lookup_resource_dir(registry: &impl RegistryLookup, order: HiveOrder, subkey: &str) -> Result<Option<PathBuf>, RuntimeError> {
    lookup_value(registry, order, subkey, "ResourceDir")
}

fn lookup_value(registry: &impl RegistryLookup, order: HiveOrder, subkey: &str, name: &str) -> Result<Option<PathBuf>, RuntimeError> {
    let hives: &[Hive] = match order {
        HiveOrder::LocalMachine => &[Hive::LocalMachine],
        HiveOrder::CurrentUserThenLocalMachine => &[Hive::CurrentUser, Hive::LocalMachine],
    };
    for (index, hive) in hives.iter().copied().enumerate() {
        let last = index + 1 == hives.len();
        match registry.open_key(hive, subkey) {
            KeyOpen::Failed { .. } if !last => {}
            KeyOpen::Failed { missing: true } => return Ok(None),
            KeyOpen::Failed { missing: false } => return Err(RuntimeError::GameSettings),
            KeyOpen::Opened(key) => {
                return match registry.query_value(&key, name) {
                    ValueQuery::Missing => Ok(None),
                    ValueQuery::Rejected => Err(RuntimeError::GameSettings),
                    ValueQuery::Present(text) => Ok(Some(validated_resource_dir(&PathBuf::from(text))?)),
                };
            }
        }
    }
    Ok(None)
}

struct LiveRegistry;

struct LiveKey(windows_sys::Win32::System::Registry::HKEY);

impl Drop for LiveKey {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this value owns the opened query key. Closing it writes nothing.
            unsafe { windows_sys::Win32::System::Registry::RegCloseKey(self.0) };
            self.0 = std::ptr::null_mut();
        }
    }
}

impl RegistryLookup for LiveRegistry {
    type Key = LiveKey;

    fn open_key(&self, hive: Hive, subkey: &str) -> KeyOpen<LiveKey> {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
        use windows_sys::Win32::System::Registry::{
            HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_WOW64_64KEY, RegOpenKeyExW,
        };
        let root = match hive {
            Hive::CurrentUser => HKEY_CURRENT_USER,
            Hive::LocalMachine => HKEY_LOCAL_MACHINE,
        };
        let wide: Vec<u16> = std::ffi::OsStr::new(subkey).encode_wide().chain(Some(0)).collect();
        let mut key = std::ptr::null_mut();
        // SAFETY: `wide` is a fixed NUL-terminated subkey. The mask is query-only
        // on the 64-bit view. No key is created.
        let opened = unsafe { RegOpenKeyExW(root, wide.as_ptr(), 0, KEY_QUERY_VALUE | KEY_WOW64_64KEY, &mut key) };
        if opened == 0 && !key.is_null() {
            KeyOpen::Opened(LiveKey(key))
        } else {
            KeyOpen::Failed { missing: opened == ERROR_FILE_NOT_FOUND }
        }
    }

    fn query_value(&self, key: &LiveKey, value: &str) -> ValueQuery {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
        use windows_sys::Win32::System::Registry::{RRF_RT_REG_SZ, RegGetValueW};
        let name: Vec<u16> = std::ffi::OsStr::new(value).encode_wide().chain(Some(0)).collect();
        let mut bytes = [0_u16; RESOURCE_DIR_UNITS];
        let mut size = (RESOURCE_DIR_UNITS * 2) as u32;
        // SAFETY: `key` is the opened query handle. `RRF_RT_REG_SZ` rejects every
        // type except REG_SZ. The buffer is 260 UTF-16 units and `size` is that
        // capacity in bytes, the wide equivalent of the official 260-byte ANSI cap.
        let read = unsafe {
            RegGetValueW(key.0, std::ptr::null(), name.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), bytes.as_mut_ptr().cast(), &mut size)
        };
        if read == ERROR_FILE_NOT_FOUND {
            return ValueQuery::Missing;
        }
        if read != 0 || size < 2 || size > (RESOURCE_DIR_UNITS * 2) as u32 || !size.is_multiple_of(2) {
            return ValueQuery::Rejected;
        }
        let units = (size as usize) / 2;
        let Some(end) = bytes[..units].iter().position(|unit| *unit == 0) else {
            return ValueQuery::Rejected;
        };
        if end == 0 {
            return ValueQuery::Rejected;
        }
        ValueQuery::Present(std::ffi::OsString::from_wide(&bytes[..end]).to_string_lossy().into_owned())
    }
}

fn read_resource_dir(subkey: &str) -> Result<Option<PathBuf>, RuntimeError> {
    lookup_resource_dir(&LiveRegistry, resource_order(subkey), subkey)
}

/// Read both INFINITAS paths from the same official registry key. Never mix
/// values from a per-user registration with a machine registration.
pub(crate) fn infinitas_paths() -> Result<(PathBuf, PathBuf), RuntimeError> {
    let registry = LiveRegistry;
    let subkey = super::infinitas::REGISTRY_KEY;
    let key = match registry.open_key(Hive::CurrentUser, subkey) {
        KeyOpen::Opened(key) => key,
        KeyOpen::Failed { .. } => match registry.open_key(Hive::LocalMachine, subkey) {
            KeyOpen::Opened(key) => key,
            KeyOpen::Failed { .. } => return Err(RuntimeError::GameSettings),
        },
    };
    let path = |name| match registry.query_value(&key, name) {
        ValueQuery::Present(text) => validated_resource_dir(Path::new(&text)),
        _ => Err(RuntimeError::GameSettings),
    };
    Ok((path("InstallDir")?, path("ResourceDir")?))
}

fn live_monitor_count() -> Result<i32, RuntimeError> {
    // SAFETY: `SM_CMONITORS` is a documented metric index and takes no buffer.
    let count = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetSystemMetrics(SM_CMONITORS) };
    if count < 0 { Ok(0) } else { Ok(count) }
}

struct GitaSettings {
    display: i32,
    window: i32,
    resolution: i32,
    audio: i32,
}

impl GitaSettings {
    fn missing() -> Self {
        Self { display: 0, window: 0, resolution: 0, audio: 0 }
    }
}

/// Single dash. Display is always present. Window is `-window` unless positive.
/// Resolution and audio appear only when positive, in that order.
fn render_gitadora(settings: &GitaSettings, monitors: i32) -> String {
    let mut prefix = String::from(" -display");
    prefix.push_str(&clamped_display(settings.display, monitors).to_string());
    if settings.window > 0 {
        prefix.push_str(" -fullscreen");
    } else {
        prefix.push_str(" -window");
    }
    if settings.resolution > 0 {
        prefix.push_str(" -fhd");
    }
    if settings.audio > 0 {
        prefix.push_str(" -exclusive");
    }
    prefix
}

fn read_fixed_settings(root: &Path) -> Result<Settings, RuntimeError> {
    let body = read_named_body(&root.join("config").join("settings_cf.xml"))?;
    match body {
        None => Ok(Settings::missing()),
        Some(body) => parse_settings(&body, NumberBase::Ddr, DDR_KNOWN.len(), |values| Settings {
            display: values[0],
            fps: values[1],
            window: values[2],
        }),
    }
}

/// Exactly one `*_cf.xml` in `config`. Zero is the normal empty map. More than
/// one is refused until the RID mapping is proven. The name is the directory
/// entry only; nothing outside that directory is opened.
fn read_gitadora_settings(root: &Path) -> Result<GitaSettings, RuntimeError> {
    let config = root.join("config");
    let selected = match select_cf_xml(&config)? {
        None => return Ok(GitaSettings::missing()),
        Some(name) => config.join(name),
    };
    let body = read_named_body(&selected)?;
    match body {
        None => Ok(GitaSettings::missing()),
        Some(body) => parse_settings(&body, NumberBase::Gitadora, GITA_KNOWN.len(), |values| GitaSettings {
            display: values[0],
            window: values[1],
            resolution: values[2],
            audio: values[3],
        }),
    }
}

const MAX_DIR_ENTRIES: usize = 256;

fn select_cf_xml(config: &Path) -> Result<Option<String>, RuntimeError> {
    // Pin every ancestor, including `config`, before the directory is listed.
    // A junction above `config` must fail here rather than be enumerated.
    let ancestors = ancestor_chain(config)?;
    let mut pins = Vec::with_capacity(ancestors.len());
    for directory in &ancestors {
        match open_directory(directory) {
            DirectoryOpen::Missing => return Ok(None),
            DirectoryOpen::Rejected => return Err(RuntimeError::GameSettings),
            DirectoryOpen::Opened(file) => pins.push(file),
        }
    }
    let entries = std::fs::read_dir(config).map_err(|_| RuntimeError::GameSettings)?;
    let mut matches = Vec::new();
    for (seen, entry) in entries.enumerate() {
        if seen >= MAX_DIR_ENTRIES {
            return Err(RuntimeError::GameSettings);
        }
        let entry = entry.map_err(|_| RuntimeError::GameSettings)?;
        let name = entry.file_name();
        let Some(text) = name.to_str() else {
            continue;
        };
        if text.len() > 240 || text.contains(['/', '\\', ':', '\0']) || text == "." || text == ".." {
            return Err(RuntimeError::GameSettings);
        }
        if text.ends_with("_cf.xml") && !text.starts_with('.') {
            matches.push(text.to_string());
        }
        if matches.len() > 1 {
            return Err(RuntimeError::GameSettings);
        }
    }
    drop(pins);
    Ok(matches.pop())
}

/// Missing file: normal defaults. Existing file: no-follow ancestors and leaf,
/// reject a directory, reparse point, or hard link, deny other writers, and
/// require the same file identity before and after the bounded read.
pub(crate) fn read_named_body(config: &Path) -> Result<Option<Vec<u8>>, RuntimeError> {
    use std::os::windows::fs::OpenOptionsExt;

    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
    };

    let Some(parent) = config.parent() else {
        return Err(RuntimeError::GameSettings);
    };
    let ancestors = ancestor_chain(parent)?;
    let mut pins = Vec::with_capacity(ancestors.len());
    for directory in &ancestors {
        match open_directory(directory) {
            DirectoryOpen::Missing => return Ok(None),
            DirectoryOpen::Rejected => return Err(RuntimeError::GameSettings),
            DirectoryOpen::Opened(file) => pins.push(file),
        }
    }
    let opened = std::fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(config);
    let mut file = match opened {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(RuntimeError::GameSettings),
    };
    let before = crate::official::file_information(&file).map_err(|_| RuntimeError::GameSettings)?;
    if before.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || before.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0
        || before.nNumberOfLinks != 1
    {
        return Err(RuntimeError::GameSettings);
    }
    let size = ((before.nFileSizeHigh as u64) << 32) | before.nFileSizeLow as u64;
    if size > SETTINGS_BYTES as u64 {
        return Err(RuntimeError::GameSettings);
    }
    let mut body = vec![0_u8; size as usize];
    file.read_exact(&mut body).map_err(|_| RuntimeError::GameSettings)?;
    let mut extra = [0_u8; 1];
    match file.read(&mut extra) {
        Ok(0) => {}
        _ => return Err(RuntimeError::GameSettings),
    }
    let after = crate::official::file_information(&file).map_err(|_| RuntimeError::GameSettings)?;
    if before.dwVolumeSerialNumber != after.dwVolumeSerialNumber
        || before.nFileIndexHigh != after.nFileIndexHigh
        || before.nFileIndexLow != after.nFileIndexLow
        || before.nFileSizeHigh != after.nFileSizeHigh
        || before.nFileSizeLow != after.nFileSizeLow
        || before.nNumberOfLinks != after.nNumberOfLinks
    {
        return Err(RuntimeError::GameSettings);
    }
    // Ancestor pins stay open through the comparison above.
    drop(pins);
    Ok(Some(body))
}

enum DirectoryOpen {
    Opened(File),
    Missing,
    Rejected,
}

fn open_directory(path: &Path) -> DirectoryOpen {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
    };
    let opened = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path);
    let file = match opened {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return DirectoryOpen::Missing,
        Err(_) => return DirectoryOpen::Rejected,
    };
    let info = match crate::official::file_information(&file) {
        Ok(info) => info,
        Err(_) => return DirectoryOpen::Rejected,
    };
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return DirectoryOpen::Rejected;
    }
    DirectoryOpen::Opened(file)
}

fn ancestor_chain(root: &Path) -> Result<Vec<PathBuf>, RuntimeError> {
    let mut chain = Vec::new();
    let mut cursor = root.to_path_buf();
    loop {
        chain.push(cursor.clone());
        if !cursor.pop() {
            break;
        }
    }
    chain.reverse();
    if chain.is_empty() {
        return Err(RuntimeError::GameSettings);
    }
    Ok(chain)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NumberBase {
    /// DDR ordinal 245 base 0: hex, octal, or decimal.
    Ddr,
    /// GITADORA passes base 10 only.
    Gitadora,
}

/// Proven text XML only: optional UTF-8 BOM, then `SettingDatas/Parameters/KeyValuePair`.
/// A binary property image, JSON, a DTD, or any other existing body fails closed.
fn parse_settings<T>(body: &[u8], base: NumberBase, slots: usize, finish: impl FnOnce(&[i32]) -> T) -> Result<T, RuntimeError> {
    if body.len() > SETTINGS_BYTES {
        return Err(RuntimeError::GameSettings);
    }
    let body = body.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(body);
    if !looks_like_text_xml(body) {
        return Err(RuntimeError::GameSettings);
    }
    let text = std::str::from_utf8(body).map_err(|_| RuntimeError::GameSettings)?;
    let mut reader = Reader::from_str(text);
    reader.config_mut().check_comments = true;
    reader.config_mut().expand_empty_elements = true;
    let known: &[&str] = match base {
        NumberBase::Ddr => &DDR_KNOWN,
        NumberBase::Gitadora => &GITA_KNOWN,
    };
    if slots != known.len() || slots > 4 {
        return Err(RuntimeError::GameSettings);
    }
    let mut values = [0_i32; 4];
    let mut depth = 0_usize;
    let mut events = 0_usize;
    let mut stack: Vec<String> = Vec::new();
    let mut capture: Option<Capture> = None;
    let mut pending_key = None;
    let mut pending_value = None;
    loop {
        events += 1;
        if events > MAX_EVENTS {
            return Err(RuntimeError::GameSettings);
        }
        let event = reader.read_event().map_err(|_| RuntimeError::GameSettings)?;
        match event {
            Event::Decl(_) | Event::Comment(_) => {}
            Event::DocType(_) | Event::PI(_) | Event::CData(_) => {
                return Err(RuntimeError::GameSettings);
            }
            Event::GeneralRef(reference) => {
                append_builtin_ref(&mut capture, &reference)?;
            }
            Event::Start(start) => {
                let name = local_name(start.name())?;
                if !attributes_allowed(&start, stack.is_empty())? {
                    return Err(RuntimeError::GameSettings);
                }
                expect_child(stack.last().map(String::as_str), &name)?;
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(RuntimeError::GameSettings);
                }
                if name == STRING {
                    let parent = stack.last().map(String::as_str);
                    if !matches!(parent, Some(KEY | VALUE)) {
                        return Err(RuntimeError::GameSettings);
                    }
                    capture = Some(Capture { kind: if parent == Some(KEY) { Field::Key } else { Field::Value }, text: String::new() });
                }
                stack.push(name);
            }
            Event::Text(text) => {
                let piece = text.xml10_content();
                if capture.is_some() {
                    append_capped(&mut capture, piece.as_ref())?;
                } else if !piece.trim().is_empty() {
                    return Err(RuntimeError::GameSettings);
                }
            }
            Event::End(_) => {
                let closed = stack.pop().ok_or(RuntimeError::GameSettings)?;
                depth = depth.saturating_sub(1);
                if closed == STRING
                    && let Some(done) = capture.take()
                {
                    store_field(&mut pending_key, &mut pending_value, done)?;
                }
                if closed == PAIR {
                    commit_pair(&mut values, known, base, &mut pending_key, &mut pending_value)?;
                }
            }
            Event::Empty(_) => return Err(RuntimeError::GameSettings),
            Event::Eof => break,
        }
    }
    if !stack.is_empty() || pending_key.is_some() || pending_value.is_some() {
        return Err(RuntimeError::GameSettings);
    }
    if depth != 0 {
        return Err(RuntimeError::GameSettings);
    }
    Ok(finish(&values[..slots]))
}

struct Capture {
    kind: Field,
    text: String,
}

enum Field {
    Key,
    Value,
}

/// The official settings program (.NET `XmlSerializer`) writes the two standard
/// schema namespace declarations on the root element. Those exact pairs are
/// accepted there. Any other attribute, or any attribute on a child, is not.
fn attributes_allowed(start: &quick_xml::events::BytesStart<'_>, root: bool) -> Result<bool, RuntimeError> {
    const NAMESPACES: [(&str, &str); 2] =
        [("xmlns:xsd", "http://www.w3.org/2001/XMLSchema"), ("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance")];
    let mut seen = [false; 2];
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|_| RuntimeError::GameSettings)?;
        let Some(index) = NAMESPACES.iter().position(|(key, value)| attribute.key.as_ref() == *key && attribute.value.as_ref() == *value)
        else {
            return Ok(false);
        };
        if !root || seen[index] {
            return Ok(false);
        }
        seen[index] = true;
    }
    Ok(true)
}

fn local_name(name: quick_xml::name::QName<'_>) -> Result<String, RuntimeError> {
    let raw = name.into_inner();
    if raw.contains(':') || raw.is_empty() {
        return Err(RuntimeError::GameSettings);
    }
    Ok(raw.to_string())
}

fn expect_child(parent: Option<&str>, child: &str) -> Result<(), RuntimeError> {
    let allowed = match parent {
        None => child == SETTING_DATAS,
        Some(SETTING_DATAS) => child == PARAMETERS,
        Some(PARAMETERS) => child == PAIR,
        Some(PAIR) => child == KEY || child == VALUE,
        Some(KEY | VALUE) => child == STRING,
        Some(_) => false,
    };
    if allowed { Ok(()) } else { Err(RuntimeError::GameSettings) }
}

/// Predefined XML entities and numeric character references only. There is no
/// custom resolver, so a name from a DTD cannot expand and nothing is read.
fn append_builtin_ref(capture: &mut Option<Capture>, reference: &quick_xml::events::BytesRef<'_>) -> Result<(), RuntimeError> {
    let raw = reference.as_ref();
    let resolved = if raw.starts_with('#') {
        reference.resolve_char_ref().ok().flatten().filter(|ch| *ch != '\0').map(|ch| ch.to_string()).ok_or(RuntimeError::GameSettings)?
    } else {
        quick_xml::escape::resolve_xml_entity(raw).ok_or(RuntimeError::GameSettings)?.to_string()
    };
    // The replacement is one character and is not scanned again, so `&amp;amp;`
    // stays the text `&amp;` instead of expanding a second time.
    if resolved.chars().count() != 1 {
        return Err(RuntimeError::GameSettings);
    }
    append_capped(capture, &resolved)
}

fn append_capped(capture: &mut Option<Capture>, piece: &str) -> Result<(), RuntimeError> {
    let Some(capture) = capture.as_mut() else {
        return Err(RuntimeError::GameSettings);
    };
    if capture.text.len().saturating_add(piece.len()) > VALUE_CAP {
        return Err(RuntimeError::GameSettings);
    }
    capture.text.push_str(piece);
    Ok(())
}

fn store_field(key: &mut Option<String>, value: &mut Option<String>, done: Capture) -> Result<(), RuntimeError> {
    let slot = match done.kind {
        Field::Key => key,
        Field::Value => value,
    };
    if slot.is_some() {
        return Err(RuntimeError::GameSettings);
    }
    *slot = Some(done.text);
    Ok(())
}

fn commit_pair(
    values: &mut [i32],
    known: &[&str],
    base: NumberBase,
    key: &mut Option<String>,
    value: &mut Option<String>,
) -> Result<(), RuntimeError> {
    let key = key.take().ok_or(RuntimeError::GameSettings)?;
    let value = value.take().ok_or(RuntimeError::GameSettings)?;
    if let Some(index) = known.iter().position(|known| *known == key) {
        values[index] = match base {
            NumberBase::Ddr => parse_i32(&value)?,
            NumberBase::Gitadora => parse_i32_base10(&value)?,
        };
    }
    Ok(())
}

/// C `strtol` base 0 into a saturating `i32`. Leading whitespace is consumed.
/// A leading `0x`/`0X` is hex, a leading `0` is octal, otherwise decimal.
/// `+` and `-` are accepted. Trailing non-numeric text is ignored. No conversion
/// is the missing-key zero, not a settings failure.
fn parse_i32(text: &str) -> Result<i32, RuntimeError> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() && matches!(bytes[index], b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C) {
        index += 1;
    }
    let mut negative = false;
    if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') {
        negative = bytes[index] == b'-';
        index += 1;
    }
    if index >= bytes.len() || !bytes[index].is_ascii_digit() {
        return Ok(0);
    }
    let (base, digits) = if bytes[index] == b'0'
        && index + 1 < bytes.len()
        && (bytes[index + 1] == b'x' || bytes[index + 1] == b'X')
        && index + 2 < bytes.len()
        && bytes[index + 2].is_ascii_hexdigit()
    {
        (16, &bytes[index + 2..])
    } else if bytes[index] == b'0' {
        (8, &bytes[index..])
    } else {
        (10, &bytes[index..])
    };
    let mut magnitude: i64 = 0;
    for byte in digits {
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' if base == 16 => byte - b'a' + 10,
            b'A'..=b'F' if base == 16 => byte - b'A' + 10,
            _ => break,
        };
        if digit as u32 >= base {
            break;
        }
        magnitude = magnitude.saturating_mul(base as i64).saturating_add(digit as i64);
    }
    let signed = if negative { -magnitude } else { magnitude };
    Ok(signed.clamp(I32_MIN, I32_MAX) as i32)
}

/// GITADORA calls `strtol` with base 10. A leading `0x` or `0` is not a base
/// selector. Overflow saturates. No conversion is the missing-key zero.
fn parse_i32_base10(text: &str) -> Result<i32, RuntimeError> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() && matches!(bytes[index], b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C) {
        index += 1;
    }
    let mut negative = false;
    if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') {
        negative = bytes[index] == b'-';
        index += 1;
    }
    if index >= bytes.len() || !bytes[index].is_ascii_digit() {
        return Ok(0);
    }
    let mut magnitude: i64 = 0;
    for byte in &bytes[index..] {
        if !byte.is_ascii_digit() {
            break;
        }
        magnitude = magnitude.saturating_mul(10).saturating_add((byte - b'0') as i64);
    }
    let signed = if negative { -magnitude } else { magnitude };
    Ok(signed.clamp(I32_MIN, I32_MAX) as i32)
}

/// Format 1 is an optional BOM, already stripped, then `<`. Leading whitespace,
/// binary property magic, and a JSON `{`/`[` body are not that format.
fn looks_like_text_xml(body: &[u8]) -> bool {
    body.first() == Some(&b'<')
}

#[cfg(test)]
mod tests {
    use super::*;

    struct OwnedSource {
        root: Option<PathBuf>,
        monitors: i32,
    }

    impl SettingsSource for OwnedSource {
        fn resource_dir(&self, _subkey: &str) -> Result<Option<PathBuf>, RuntimeError> {
            Ok(self.root.clone())
        }

        fn monitor_count(&self) -> Result<i32, RuntimeError> {
            Ok(self.monitors)
        }
    }

    fn scratch_config(body: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        std::fs::create_dir(&config).unwrap();
        std::fs::write(config.join("settings_cf.xml"), body).unwrap();
        let root = dir.path().to_path_buf();
        (dir, root)
    }

    fn prefix_of(root: Option<PathBuf>, monitors: i32) -> Result<String, RuntimeError> {
        ddr_prefix(&OwnedSource { root, monitors })
    }

    fn xml(pairs: &str) -> String {
        format!("<SettingDatas><Parameters>{pairs}</Parameters></SettingDatas>")
    }

    fn pair(key: &str, value: &str) -> String {
        format!("<KeyValuePair><Key><string>{key}</string></Key><Value><string>{value}</string></Value></KeyValuePair>")
    }

    #[test]
    fn token_profile_emits_an_empty_prefix() {
        let args = Arguments::load(ArgumentProfile::Token, None, Path::new(r"C:\")).unwrap();
        assert_eq!(args.prefix(), "");
        assert_eq!(args.suffix(), "");
        assert!(Arguments::load(ArgumentProfile::Token, Some(crate::GitadoraMode::GuitarFreaks), Path::new(r"C:\")).is_err());
    }

    fn popn_root(setting: Option<&[u8]>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let dev = dir.path().join("game").join("dev");
        std::fs::create_dir_all(&dev).unwrap();
        if let Some(bytes) = setting {
            std::fs::write(dev.join("setting.bin"), bytes).unwrap();
        }
        dir
    }

    struct PopnSource {
        install: Option<PathBuf>,
        resource: Option<PathBuf>,
    }

    impl SettingsSource for PopnSource {
        fn resource_dir(&self, subkey: &str) -> Result<Option<PathBuf>, RuntimeError> {
            assert_eq!(subkey, POPN_KEY);
            Ok(self.resource.clone())
        }
        fn install_dir(&self, subkey: &str) -> Result<Option<PathBuf>, RuntimeError> {
            assert_eq!(subkey, POPN_KEY);
            Ok(self.install.clone())
        }
        fn monitor_count(&self) -> Result<i32, RuntimeError> {
            Ok(1)
        }
    }

    /// The canonical `\\?\` root reduced to its plain drive form.
    fn derived_install(root: &Path) -> PathBuf {
        let text = root.to_string_lossy();
        PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
    }

    /// Official values bound to this exact root, as a fresh Settings run leaves
    /// them. This is the "appropriately bound" case.
    fn binder(root: &Path) -> PopnSource {
        let install = derived_install(root);
        let resource = install.join("Resource");
        PopnSource { install: Some(install), resource: Some(resource) }
    }

    /// The official `<I>/x` and `<R>/work` forms for a root.
    fn expected_head(root: &Path) -> String {
        let install = derived_install(root);
        format!(
            "dev/LocalProp/bootstrap.local.xml debug_localstrap \"{}x\" \"{}work\"",
            slash_form(&install).unwrap(),
            slash_form(&install.join("Resource")).unwrap(),
        )
    }

    #[test]
    fn popn_builds_the_reporters_bootstrap_prefix() {
        // No setting.bin: window mode, no filter.
        let root = popn_root(None);
        assert_eq!(popn_prefix(&binder(root.path()), root.path()).unwrap(), expected_head(root.path()));
        // Fullscreen with the chattering filter.
        let root = popn_root(Some(&[0, 1, 0, 0]));
        assert_eq!(popn_prefix(&binder(root.path()), root.path()).unwrap(), format!("{} -full -cf", expected_head(root.path())));
        // Window mode, read through the canonical `\\?\` root the installation
        // check hands over. This install's real file is `01 00 00 00`.
        let root = popn_root(Some(&[1, 0, 0, 0]));
        let canonical = root.path().canonicalize().unwrap();
        assert!(canonical.to_string_lossy().starts_with(r"\\?\"));
        assert_eq!(popn_prefix(&binder(&canonical), &canonical).unwrap(), expected_head(&canonical));
        // A short file is ignored, as the reporter's failed 4-byte read is.
        let root = popn_root(Some(&[0]));
        assert_eq!(popn_prefix(&binder(root.path()), root.path()).unwrap(), expected_head(root.path()));
    }

    #[test]
    fn popn_uses_official_values_only_when_bound_to_this_root() {
        let root = popn_root(Some(&[0, 1, 0, 0]));
        let canonical = root.path().canonicalize().unwrap();
        // Bound to this install: the official values are honored as-is.
        let own = binder(&canonical);
        let expected = format!("{} -full -cf", expected_head(&canonical));
        assert_eq!(popn_prefix(&own, &canonical).unwrap(), expected);
        // A stale official InstallDir for another root is ignored; the verified
        // root wins even though both official values are present. This keeps a
        // moved install from pointing the game at the old directory.
        let stale_install = derived_install(&canonical).join("elsewhere");
        let stale = PopnSource { install: Some(stale_install.clone()), resource: Some(stale_install.join("Resource")) };
        assert!(!same_install(&stale_install, &derived_install(&canonical)));
        assert_eq!(popn_prefix(&stale, &canonical).unwrap(), expected);
    }

    #[test]
    fn popn_derives_its_paths_from_a_fresh_install_without_official_values() {
        // A fresh Hub install has no HKCU/HKLM values. The prefix comes from the
        // verified root, matching the official trailing-separator layout.
        let root = popn_root(Some(&[0, 0, 0, 0]));
        let canonical = root.path().canonicalize().unwrap();
        let expected = format!("{} -full", expected_head(&canonical));
        // Both values absent, or only one present, both derive from the root.
        for source in [
            PopnSource { install: None, resource: None },
            PopnSource { install: Some(PathBuf::from(r"C:\games\pop'n music Lively\")), resource: None },
            PopnSource { install: None, resource: Some(PathBuf::from(r"C:\games\pop'n music Lively\Resource\")) },
        ] {
            assert_eq!(popn_prefix(&source, &canonical).unwrap(), expected);
        }
        // The derived path is validated like a registry value: a relative root
        // is refused rather than guessed.
        assert_eq!(popn_prefix(&PopnSource { install: None, resource: None }, Path::new("relative")), Err(RuntimeError::GameSettings));
    }

    #[test]
    fn popn_command_line_has_the_arguments_the_game_requires() {
        let root = popn_root(Some(&[0, 0, 0, 0]));
        let canonical = root.path().canonicalize().unwrap();
        let install = slash_form(&derived_install(&canonical)).unwrap();
        let resource = slash_form(&derived_install(&canonical).join("Resource")).unwrap();
        let args = Arguments { prefix: popn_prefix(&binder(&canonical), &canonical).unwrap(), suffix: "" };
        let wide = crate::game::parameters(&args, "abc").unwrap();
        let text = String::from_utf16(&wide[..wide.len() - 1]).unwrap();
        assert_eq!(text, format!("dev/LocalProp/bootstrap.local.xml debug_localstrap \"{install}x\" \"{resource}work\" -full -t abc"));
        // Quote-aware split, as the game's CRT does: at least the five it requires.
        let mut count = 0;
        let mut quoted = false;
        let mut in_word = false;
        for ch in text.chars() {
            match ch {
                '"' => {
                    quoted = !quoted;
                    if !in_word {
                        in_word = true;
                        count += 1;
                    }
                }
                ' ' if !quoted => in_word = false,
                _ => {
                    if !in_word {
                        in_word = true;
                        count += 1;
                    }
                }
            }
        }
        assert_eq!(count, 7, "{text}");
    }

    #[test]
    fn missing_resource_and_missing_file_use_normal_defaults() {
        assert_eq!(prefix_of(None, 2).unwrap(), " --display0 --fps60");
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(prefix_of(Some(dir.path().to_path_buf()), 2).unwrap(), " --display0 --fps60");
    }

    #[test]
    fn real_xml_crosses_the_read_parse_and_prefix_seam() {
        let pairs = format!("{}{}{}", pair("DisplaySettings", "1"), pair("FPSSettings", "1"), pair("WindowSettings", "2"));
        let (dir, root) = scratch_config(xml(&pairs).as_bytes());
        let prefix = prefix_of(Some(root), 2).unwrap();
        assert_eq!(prefix, " --display1 --fps120 --borderless");
        drop(dir);
    }

    #[test]
    fn base0_overflow_duplicates_and_trailing_text() {
        assert_eq!(parse_i32("  -010").unwrap(), -8);
        assert_eq!(parse_i32("+0x10").unwrap(), 16);
        assert_eq!(parse_i32("0X00000010").unwrap(), 16);
        assert_eq!(parse_i32("12abc").unwrap(), 12);
        assert_eq!(parse_i32("  ").unwrap(), 0);
        assert_eq!(parse_i32("999999999999999999999").unwrap(), i32::MAX);
        assert_eq!(parse_i32("-999999999999999999999").unwrap(), i32::MIN);
        assert_eq!(parse_i32("08").unwrap(), 0);
        let pairs = format!("{}{}", pair("FPSSettings", "0"), pair("FPSSettings", "1"));
        let (_dir, root) = scratch_config(xml(&pairs).as_bytes());
        assert_eq!(prefix_of(Some(root), 1).unwrap(), " --display0 --fps120");
    }

    #[test]
    fn display_clamps_to_the_monitor_count() {
        let pairs = pair("DisplaySettings", "-4");
        let (_dir, root) = scratch_config(xml(&pairs).as_bytes());
        assert_eq!(prefix_of(Some(root), 3).unwrap(), " --display0 --fps60");
        let pairs = pair("DisplaySettings", "9");
        let (_dir, root) = scratch_config(xml(&pairs).as_bytes());
        assert_eq!(prefix_of(Some(root), 3).unwrap(), " --display2 --fps60");
        let pairs = pair("WindowSettings", "1");
        let (_dir, root) = scratch_config(xml(&pairs).as_bytes());
        assert_eq!(prefix_of(Some(root), 1).unwrap(), " --display0 --fps60 --fullscreen");
        let pairs = pair("WindowSettings", "7");
        let (_dir, root) = scratch_config(xml(&pairs).as_bytes());
        assert_eq!(prefix_of(Some(root), 1).unwrap(), " --display0 --fps60");
    }

    #[test]
    fn invalid_xml_dtd_binary_and_oversize_fail_closed() {
        for body in [
            b"{}".as_slice(),
            b"[1]",
            b"\xA0\x45\x00\x00",
            b"<SettingDatas><Parameters></SettingDatas>",
            b"<?xml version=\"1.0\"?><!DOCTYPE SettingDatas [<!ENTITY x SYSTEM \"file:///secret\">]><SettingDatas><Parameters></Parameters></SettingDatas>",
            &{
                let mut big = xml("").into_bytes();
                big.extend(std::iter::repeat_n(b' ', SETTINGS_BYTES));
                big
            },
        ] {
            let (_dir, root) = scratch_config(body);
            assert_eq!(
                prefix_of(Some(root), 1),
                Err(RuntimeError::GameSettings)
            );
        }
    }

    #[test]
    fn a_hardlink_and_an_unsafe_root_are_rejected() {
        let (dir, root) = scratch_config(xml("").as_bytes());
        let source = root.join("config").join("settings_cf.xml");
        let linked = root.join("config").join("linked.xml");
        std::fs::hard_link(&source, &linked).unwrap();
        assert_eq!(prefix_of(Some(root.clone()), 1), Err(RuntimeError::GameSettings));
        assert_eq!(prefix_of(Some(PathBuf::from(r"\\server\share")), 1), Err(RuntimeError::GameSettings));
        assert_eq!(prefix_of(Some(PathBuf::from(r"C:\games\..\other")), 1), Err(RuntimeError::GameSettings));
        assert_eq!(prefix_of(Some(PathBuf::from(r"relative\config")), 1), Err(RuntimeError::GameSettings));
        let drive = std::env::var_os("SystemDrive").unwrap();
        let fixed = PathBuf::from(format!(r"{}\konaste-argument-not-created", drive.to_string_lossy()));
        assert!(validated_resource_dir(&fixed).is_ok());
        drop(dir);
    }

    #[test]
    fn the_official_settings_program_output_is_read() {
        // As written by DDR's settings program on 2026-11-20: CRLF, a declaration,
        // the two .NET schema namespaces, and a key the Hub does not use.
        let pair = |key: &str, value: &str| {
            format!(
                "    <KeyValuePair>\r\n      <Key>\r\n        <string>{key}</string>\r\n      </Key>\r\n      <Value>\r\n        <string>{value}</string>\r\n      </Value>\r\n    </KeyValuePair>\r\n"
            )
        };
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<SettingDatas xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\r\n  <Parameters>\r\n{}{}{}{}  </Parameters>\r\n</SettingDatas>",
            pair("DisplaySettings", "0"),
            pair("WindowSettings", "0"),
            pair("FPSSettings", "1"),
            pair("AudioDeviceSettings", "0"),
        );
        let (_dir, root) = scratch_config(body.as_bytes());
        assert_eq!(prefix_of(Some(root), 2).unwrap(), " --display0 --fps120");
    }

    #[test]
    fn attributes_other_than_the_root_schema_namespaces_are_refused() {
        for body in [
            "<SettingDatas xmlns:xsd=\"http://example.invalid/\"><Parameters></Parameters></SettingDatas>",
            "<SettingDatas other=\"1\"><Parameters></Parameters></SettingDatas>",
            "<SettingDatas xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\"><Parameters></Parameters></SettingDatas>",
            "<SettingDatas><Parameters xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\"></Parameters></SettingDatas>",
        ] {
            let (_dir, root) = scratch_config(body.as_bytes());
            assert_eq!(prefix_of(Some(root), 2), Err(RuntimeError::GameSettings), "{body}");
        }
    }

    #[test]
    fn an_optional_bom_and_an_unknown_key_do_not_change_known_defaults() {
        let mut body = vec![0xEF, 0xBB, 0xBF];
        body.extend(xml(&pair("Other", "4")).into_bytes());
        let (_dir, root) = scratch_config(&body);
        assert_eq!(prefix_of(Some(root), 2).unwrap(), " --display0 --fps60");
    }

    fn gita_file(dir: &Path, name: &str, body: &[u8]) {
        let config = dir.join("config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join(name), body).unwrap();
    }

    fn gita_prefix(root: Option<PathBuf>, monitors: i32) -> Result<String, RuntimeError> {
        let _ = gitadora_prefix(&OwnedSource { root: root.clone(), monitors })?;
        gitadora_prefix(&OwnedSource { root, monitors })
    }

    #[test]
    fn gitadora_requires_a_typed_mode_and_refuses_one_on_other_titles() {
        assert!(matches!(Arguments::load(ArgumentProfile::GitadoraSettings, None, Path::new(r"C:\")), Err(RuntimeError::GameModeRequired)));
        assert!(matches!(
            Arguments::load(ArgumentProfile::DdrSettings, Some(crate::GitadoraMode::DrumMania), Path::new(r"C:\")),
            Err(RuntimeError::Protocol)
        ));
        assert!(matches!(
            Arguments::load(ArgumentProfile::Token, Some(crate::GitadoraMode::GuitarFreaks), Path::new(r"C:\")),
            Err(RuntimeError::Protocol)
        ));
        assert_eq!(crate::GitadoraMode::GuitarFreaks.argument(), "-gf");
        assert_eq!(crate::GitadoraMode::DrumMania.argument(), "-dm");
    }

    #[test]
    fn gitadora_one_file_sets_flag_order_and_base10_values() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(gita_prefix(Some(dir.path().to_path_buf()), 2).unwrap(), " -display0 -window");
        let pairs = format!(
            "{}{}{}{}",
            pair("DisplaySettings", "-2"),
            pair("WindowSettings", "1"),
            pair("ResoSettings", "0x10"),
            pair("AudioPlaySettings", "1")
        );
        gita_file(dir.path(), "fixture_cf.xml", xml(&pairs).as_bytes());
        // `0x10` is base 10: the `x` stops conversion at 0, so resolution stays off.
        assert_eq!(gita_prefix(Some(dir.path().to_path_buf()), 3).unwrap(), " -display0 -fullscreen -exclusive");
        std::fs::remove_file(dir.path().join("config").join("fixture_cf.xml")).unwrap();
        let pairs = format!(
            "{}{}{}{}",
            pair("DisplaySettings", "4"),
            pair("WindowSettings", "0"),
            pair("ResoSettings", "2"),
            pair("AudioPlaySettings", "-1")
        );
        gita_file(dir.path(), "only_cf.xml", xml(&pairs).as_bytes());
        assert_eq!(gita_prefix(Some(dir.path().to_path_buf()), 2).unwrap(), " -display1 -window -fhd");
        gita_file(dir.path(), "other_cf.xml", xml("").as_bytes());
        assert_eq!(gita_prefix(Some(dir.path().to_path_buf()), 2), Err(RuntimeError::GameSettings));
    }

    #[test]
    fn builtin_and_numeric_references_decode_and_custom_ones_do_not() {
        let pairs = format!("{}{}", pair("AudioPlaySettings", "1&amp;2"), pair("WindowSettings", "&#49;"));
        let (_dir, root) = scratch_config(xml(&pairs).as_bytes());
        // `1&2` is not a number, so audio stays off. `&#49;` is `1`.
        assert_eq!(prefix_of(Some(root), 1).unwrap(), " --display0 --fps60 --fullscreen");
        let pairs = pair("DisplaySettings", "&#x32;");
        let (_dir, root) = scratch_config(xml(&pairs).as_bytes());
        assert_eq!(prefix_of(Some(root), 4).unwrap(), " --display2 --fps60");
        for body in [
            xml(&pair("WindowSettings", "&unknown;")),
            "<SettingDatas><Parameters><KeyValuePair><Key><string>WindowSettings</string></Key><Value><string>1</string></Value></KeyValuePair></Parameters></SettingDatas>"
                .replace("1", "&#0;"),
        ] {
            let (_dir, root) = scratch_config(body.as_bytes());
            assert_eq!(prefix_of(Some(root), 1), Err(RuntimeError::GameSettings));
        }
        // Not expanded twice: the stored text is `&amp;`, which is not a number.
        let (_dir, root) = scratch_config(xml(&pair("WindowSettings", "&amp;amp;")).as_bytes());
        assert_eq!(prefix_of(Some(root), 1).unwrap(), " --display0 --fps60");
    }

    #[test]
    fn a_junction_above_config_is_rejected_before_the_directory_is_listed() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        let linked = dir.path().join("linked");
        std::fs::create_dir(&outside).unwrap();
        std::fs::create_dir(outside.join("config")).unwrap();
        std::fs::write(outside.join("config").join("only_cf.xml"), xml(&pair("WindowSettings", "1"))).unwrap();
        std::fs::create_dir(&linked).unwrap();
        let status = std::process::Command::new(r"C:\Windows\System32\cmd.exe")
            .args(["/D", "/C", "mklink", "/J", &linked.join("parent").to_string_lossy(), &outside.to_string_lossy()])
            .status()
            .unwrap();
        assert!(status.success());
        let root = linked.join("parent");
        assert_eq!(gita_prefix(Some(root), 1), Err(RuntimeError::GameSettings));
        assert!(outside.join("config").join("only_cf.xml").is_file());
    }

    struct FakeRegistry {
        user_open: bool,
        user_open_missing: bool,
        machine_open: bool,
        user_value: Option<String>,
        machine_value: Option<String>,
        user_query_rejected: bool,
        machine_query_rejected: bool,
    }

    impl RegistryLookup for FakeRegistry {
        type Key = Hive;

        fn open_key(&self, hive: Hive, _subkey: &str) -> KeyOpen<Hive> {
            let (open, missing) = match hive {
                Hive::CurrentUser => (self.user_open, self.user_open_missing),
                Hive::LocalMachine => (self.machine_open, true),
            };
            if open { KeyOpen::Opened(hive) } else { KeyOpen::Failed { missing } }
        }

        fn query_value(&self, key: &Hive, _name: &str) -> ValueQuery {
            let (rejected, value) = match key {
                Hive::CurrentUser => (self.user_query_rejected, &self.user_value),
                Hive::LocalMachine => (self.machine_query_rejected, &self.machine_value),
            };
            if rejected { ValueQuery::Rejected } else { value.clone().map_or(ValueQuery::Missing, ValueQuery::Present) }
        }
    }

    fn fake_registry(user: Option<&str>, machine: Option<&str>) -> FakeRegistry {
        FakeRegistry {
            user_open: true,
            user_open_missing: true,
            machine_open: true,
            user_value: user.map(str::to_string),
            machine_value: machine.map(str::to_string),
            user_query_rejected: false,
            machine_query_rejected: false,
        }
    }

    #[test]
    fn gitadora_uses_the_current_user_key_and_ddr_does_not() {
        let drive = std::env::var("SystemDrive").unwrap();
        let user = format!(r"{drive}\user-root");
        let machine = format!(r"{drive}\machine-root");
        let both = fake_registry(Some(&user), Some(&machine));
        assert_eq!(lookup_resource_dir(&both, resource_order(GITA_KEY), GITA_KEY).unwrap().unwrap(), PathBuf::from(&user));
        let opened_without_value = fake_registry(None, Some(&machine));
        assert_eq!(lookup_resource_dir(&opened_without_value, resource_order(GITA_KEY), GITA_KEY).unwrap(), None);
        let mut user_closed = fake_registry(Some(&user), Some(&machine));
        user_closed.user_open = false;
        user_closed.user_open_missing = false;
        assert_eq!(lookup_resource_dir(&user_closed, resource_order(GITA_KEY), GITA_KEY).unwrap().unwrap(), PathBuf::from(&machine));
        let mut bad_user = fake_registry(Some(&user), Some(&machine));
        bad_user.user_query_rejected = true;
        assert_eq!(lookup_resource_dir(&bad_user, resource_order(GITA_KEY), GITA_KEY), Err(RuntimeError::GameSettings));
        assert_eq!(lookup_resource_dir(&both, resource_order(DDR_KEY), DDR_KEY).unwrap().unwrap(), PathBuf::from(&machine));
    }

    #[test]
    fn a_259_character_resource_dir_is_not_cut_by_a_260_byte_wide_buffer() {
        let drive = std::env::var("SystemDrive").unwrap();
        let mut path = format!(r"{drive}\");
        while path.len() < 259 {
            let room = 259 - path.len();
            if room <= 8 {
                path.push_str(&"a".repeat(room));
            } else {
                path.push_str("folder\\");
            }
        }
        assert_eq!(path.len(), 259);
        assert_eq!(path.encode_utf16().count(), 259);
        let decoded = decode_resource_units(&{
            let mut units: Vec<u16> = path.encode_utf16().collect();
            units.push(0);
            assert_eq!(units.len(), RESOURCE_DIR_UNITS);
            units
        });
        assert_eq!(decoded.chars().count(), 259);
        assert!(decoded.ends_with('a'));
        assert!(!decoded.ends_with("folder"));
    }

    fn decode_resource_units(units: &[u16]) -> String {
        assert!(units.len() <= RESOURCE_DIR_UNITS);
        let end = units.iter().position(|unit| *unit == 0).unwrap();
        String::from_utf16(&units[..end]).unwrap()
    }

    #[test]
    fn gitadora_suffix_is_the_typed_instrument_after_the_token() {
        let args = Arguments { prefix: " -display0 -window".into(), suffix: "-gf" };
        let wide = crate::game::parameters(&args, "tok").unwrap();
        let text = String::from_utf16(&wide[..wide.len() - 1]).unwrap();
        assert_eq!(text, " -display0 -window -t tok -gf");
        let args = Arguments { prefix: " -display0 -fullscreen -fhd -exclusive".into(), suffix: "-dm" };
        let wide = crate::game::parameters(&args, "tok").unwrap();
        let text = String::from_utf16(&wide[..wide.len() - 1]).unwrap();
        assert_eq!(text, " -display0 -fullscreen -fhd -exclusive -t tok -dm");
        assert!(!text.contains("--dev") && !text.contains("--trial"));
    }
}
