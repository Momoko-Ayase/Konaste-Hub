//! The two pieces of official-installer setup a settings program needs.
//!
//! Each title's `libsettings.dll` imports `avs2-core.dll` from its own folder
//! and reads `InstallDir` and `ResourceDir` from `SOFTWARE\KONAMI\<title>`. A Hub
//! install has neither, so `settings.exe` dies with `DllNotFoundException`.
//!
//! The DLL is copied from the runtime pair, which `konaste_bootstrap::inspect`
//! has just re-hashed against its independent pin and Authenticode. A missing
//! pair is fetched once, exactly as a check fetches it, before the copy. An
//! existing file is left alone. Registry hives follow each `libsettings.dll`:
//! NOSTALGIA, GITADORA, and pop'n open HKCU, then HKLM. DDR opens HKLM only,
//! so DDR is written there through one elevated `reg.exe` call. SDVX and
//! Bombergirl need no `avs2-core.dll` copy and are not touched.
//! Values that already exist are never overwritten.
//!
//! Removing a title from the library deletes nothing from the registry. A
//! value that equals what the Hub writes is not evidence that the Hub wrote
//! it: an official installer records the same `InstallDir`/`ResourceDir` for
//! the same root. [`forget`] is therefore deliberately non-destructive.
use crate::{catalog::GameId, state::HubError};
use konaste_bootstrap::{GameFamily, PairStatus};
use std::path::{Path, PathBuf};

/// Hive a title's settings library reads first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

/// What a title needs. `None` for titles whose installer is the only source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Needs {
    pub family: GameFamily,
    /// Folder of `settings.exe` relative to the install root.
    pub modules: &'static str,
    pub hive: Hive,
}

pub fn needs(id: GameId) -> Option<Needs> {
    let (family, hive) = match id {
        GameId::Nostalgia => (GameFamily::Nostalgia, Hive::CurrentUser),
        GameId::Gitadora => (GameFamily::Gitadora, Hive::CurrentUser),
        GameId::Popn => (GameFamily::Popn, Hive::CurrentUser),
        GameId::Ddr => (GameFamily::Ddr, Hive::LocalMachine),
        GameId::Sdvx | GameId::BomberGirl | GameId::Infinitas => return None,
    };
    Some(Needs { family, modules: "launcher/modules", hive })
}

/// Supported title and a local path its official libraries accept. Only the
/// titles whose `libsettings.dll` reads `InstallDir`/`ResourceDir` are limited
/// to an ASCII drive-letter root under the 260-byte cap. SDVX and Bombergirl,
/// which take neither value, are bound only by their verified title. Reusable
/// by callers that must refuse a root before any read, copy, or write.
pub fn title_paths_ok(id: GameId, root: &Path) -> bool {
    id.title().is_some() && (needs(id).is_none() || values(root).is_ok())
}

/// `ResourceDir` beneath this install root, or `None` for a title whose
/// libraries read no official value. Reads no registry: Play creates work
/// folders from this derived path whether or not an official value exists, and
/// never writes one.
pub fn resource_dir(id: GameId, root: &Path) -> Result<Option<PathBuf>, HubError> {
    if id.title().is_none() {
        return Err(HubError { code: "unsupportedTitle" });
    }
    if needs(id).is_none() {
        return Ok(None);
    }
    let (_, resource) = values(root)?;
    Ok(Some(PathBuf::from(resource)))
}

/// The values the official installer writes: both end in a separator, and
/// `ResourceDir` is `Resource\` beneath the install root.
pub fn values(root: &Path) -> Result<(String, String), HubError> {
    let text = root.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    let bytes = text.as_bytes();
    // Drive-letter paths only; the settings library copies at most 260 bytes.
    if bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || bytes[2] != b'\\'
        || text.contains('\0')
        || !text.is_ascii()
    {
        return Err(HubError { code: "gameSettingsFailed" });
    }
    let install = format!(r"{}\", text.trim_end_matches('\\'));
    let resource = format!(r"{install}Resource\");
    if resource.len() + 1 > 260 {
        return Err(HubError { code: "gameSettingsFailed" });
    }
    Ok((install, resource))
}

/// Copy the verified DLL, create `Resource\`, and write the registry values a
/// settings program reads. Blocking; run on a worker thread. Always runs for a
/// title that needs setup, even if [`copy_runtime`] already placed the DLL.
pub fn prepare(id: GameId, root: &Path, cache: &Path) -> Result<(), HubError> {
    let Some(needs) = needs(id) else { return Ok(()) };
    let failed = || HubError { code: "gameSettingsFailed" };
    copy_runtime(id, root, cache)?;
    let (install, resource) = values(root)?;
    std::fs::create_dir_all(root.join("Resource")).map_err(|_| failed())?;
    let key = crate::catalog::descriptor(id).registry.ok_or_else(failed)?;
    match needs.hive {
        Hive::CurrentUser => write_current_user(key, &install, &resource).map_err(|_| failed()),
        Hive::LocalMachine if machine_values_present(key) => Ok(()),
        Hive::LocalMachine => write_local_machine(key, &install, &resource),
    }
}

/// The verified runtime pair, fetched only when the cache has no complete one.
/// The same consent a check already implies; still one pinned, Authenticode-
/// checked package, and no unpinned file is accepted.
pub fn copy_runtime(id: GameId, root: &Path, cache: &Path) -> Result<(), HubError> {
    let Some(needs) = needs(id) else { return Ok(()) };
    let target = root.join(needs.modules).join("avs2-core.dll");
    if target.exists() {
        return Ok(());
    }
    let pair = verified_pair(needs.family, cache).map_err(|_| HubError { code: "componentsMissing" })?;
    copy_new(&pair.avs2_core, &target).map_err(|_| HubError { code: "gameSettingsFailed" })
}

fn verified_pair(family: GameFamily, cache: &Path) -> Result<konaste_bootstrap::RuntimePair, ()> {
    match konaste_bootstrap::inspect(family, cache) {
        Ok(PairStatus::Present(pair)) => Ok(pair),
        Ok(PairStatus::Missing) => konaste_bootstrap::acquire(family, cache, true, None).map_err(|_| ()),
        Ok(PairStatus::Stale { .. }) | Err(_) => Err(()),
    }
}

/// The folders the official launcher creates under `ResourceDir` every time it
/// starts (`launcher.exe` `sub_14001A160`): `work`, then `work\launcher\dev`
/// with `nvram`, `raw`, and `raw\log`. The game writes its data there, and pop'n
/// stops with 5-1500-0110 when `work` is missing. Existing folders are kept;
/// nothing else is created and no file is written.
pub const WORK_FOLDERS: [&str; 6] =
    ["work", r"work\launcher", r"work\launcher\dev", r"work\launcher\dev\nvram", r"work\launcher\dev\raw", r"work\launcher\dev\raw\log"];

/// Create [`WORK_FOLDERS`] under an already-derived `Resource` path. Play derives
/// that path from the install root, so a fresh install with no official values
/// still starts with the folders pop'n requires. The `Resource` folder itself is
/// created first when absent. A title without an official runtime gets none.
/// Existing folders are kept.
pub fn create_work_folders(resource: &Path) -> Result<(), HubError> {
    std::fs::create_dir_all(resource).map_err(|_| HubError { code: "filesystem" })?;
    for folder in WORK_FOLDERS {
        let path = resource.join(folder);
        match std::fs::create_dir(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && path.is_dir() => {}
            Err(_) => return Err(HubError { code: "filesystem" }),
        }
    }
    Ok(())
}

/// Remove from library. Nothing is deleted from the registry.
///
/// The previous implementation deleted HKCU `InstallDir`/`ResourceDir` when
/// they equaled the values the Hub would write. An official installer writes
/// exactly those values for the same root, so that test cannot distinguish
/// Hub-owned data from official data. Removing a title must not destroy an
/// official registration, and the Hub keeps no durable ownership record, so the
/// safe default is to leave the registry alone. HKLM is likewise untouched.
pub fn forget(_id: GameId, _root: &Path) {}

/// Write to a sibling temp name, then move it in without replacing anything.
fn copy_new(from: &Path, to: &Path) -> std::io::Result<()> {
    let temp = to.with_extension("dll.hub-part");
    let _ = std::fs::remove_file(&temp);
    std::fs::copy(from, &temp)?;
    let result = std::fs::hard_link(&temp, to);
    let _ = std::fs::remove_file(&temp);
    match result {
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        other => other,
    }
}

#[cfg(windows)]
fn write_current_user(key: &str, install: &str, resource: &str) -> std::io::Result<()> {
    use winreg::{
        RegKey,
        enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, KEY_WOW64_64KEY},
    };
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey_with_flags(key, KEY_READ | KEY_SET_VALUE | KEY_WOW64_64KEY)?;
    for (name, value) in [("InstallDir", install), ("ResourceDir", resource)] {
        if key.get_raw_value(name).is_err() {
            key.set_value(name, &value)?;
        }
    }
    Ok(())
}
#[cfg(not(windows))]
fn write_current_user(_: &str, _: &str, _: &str) -> std::io::Result<()> {
    Err(std::io::ErrorKind::Unsupported.into())
}

#[cfg(windows)]
fn machine_values_present(key: &str) -> bool {
    use winreg::{
        RegKey,
        enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY},
    };
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(key, KEY_READ | KEY_WOW64_64KEY)
        .is_ok_and(|key| ["InstallDir", "ResourceDir"].iter().all(|name| key.get_value::<String, _>(name).is_ok()))
}
#[cfg(not(windows))]
fn machine_values_present(_: &str) -> bool {
    false
}

/// One UAC prompt. `cmd /c` runs two fixed `reg add` commands without `/f`,
/// so an existing value is kept. The paths come from [`values`], which admits
/// only ASCII drive paths; `"` and `%` are refused before quoting.
#[cfg(windows)]
fn write_local_machine(key: &str, install: &str, resource: &str) -> Result<(), HubError> {
    let failed = || HubError { code: "gameSettingsFailed" };
    if [key, install, resource].iter().any(|text| text.contains(['"', '%', '&', '|', '<', '>', '^'])) {
        return Err(failed());
    }
    let system = std::env::var_os("SystemRoot").map(PathBuf::from).ok_or_else(failed)?.join("System32");
    let reg = system.join("reg.exe");
    let cmd = system.join("cmd.exe");
    // A trailing backslash before a closing quote would escape it; `\\` keeps it literal.
    let quote = |text: &str| format!("\"{}\"", text.strip_suffix('\\').map_or(text.to_string(), |rest| format!(r"{rest}\\")));
    let add =
        |name: &str, value: &str| format!("\"{}\" add \"HKLM\\{key}\" /v {name} /t REG_SZ /d {} /reg:64", reg.display(), quote(value));
    let parameters = format!("/d /c \"{} & {}\"", add("InstallDir", install), add("ResourceDir", resource));
    match elevate(&cmd, &parameters) {
        Ok(()) if machine_values_present(key) => Ok(()),
        Ok(()) => Err(failed()),
        Err(cancelled) if cancelled => Err(HubError { code: "elevationCancelled" }),
        Err(_) => Err(failed()),
    }
}
#[cfg(not(windows))]
fn write_local_machine(_: &str, _: &str, _: &str) -> Result<(), HubError> {
    Err(HubError { code: "windowsOnly" })
}

/// `runas` and wait. `Err(true)` is a declined prompt.
#[cfg(windows)]
fn elevate(file: &Path, parameters: &str) -> Result<(), bool> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_CANCELLED, GetLastError},
        System::Threading::{INFINITE, WaitForSingleObject},
        UI::{
            Shell::{SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
            WindowsAndMessaging::SW_HIDE,
        },
    };
    let wide = |text: &std::ffi::OsStr| text.encode_wide().chain(Some(0)).collect::<Vec<u16>>();
    let verb = wide("runas".as_ref());
    let file = wide(file.as_os_str());
    let parameters = wide(parameters.as_ref());
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: verb.as_ptr(),
        lpFile: file.as_ptr(),
        lpParameters: parameters.as_ptr(),
        nShow: SW_HIDE,
        ..Default::default()
    };
    // SAFETY: every string is NUL-terminated and outlives the call.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        return Err(unsafe { GetLastError() } == ERROR_CANCELLED);
    }
    if info.hProcess.is_null() {
        return Err(false);
    }
    unsafe {
        WaitForSingleObject(info.hProcess, INFINITE);
        CloseHandle(info.hProcess);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_match_the_official_installer_layout() {
        let (install, resource) = values(Path::new(r"\\?\D:\Konaste\NOSTALGIA")).unwrap();
        assert_eq!(install, r"D:\Konaste\NOSTALGIA\");
        assert_eq!(resource, r"D:\Konaste\NOSTALGIA\Resource\");
        assert_eq!(values(Path::new(r"D:\Konaste\GITADORA\")).unwrap().0, r"D:\Konaste\GITADORA\");
        assert!(values(Path::new(r"\\server\share\DDR")).is_err());
        assert!(values(Path::new("relative")).is_err());
        assert!(values(Path::new(r"E:\ゲーム\DDR")).is_err());
    }

    #[test]
    fn hives_follow_each_settings_library() {
        assert_eq!(needs(GameId::Nostalgia).unwrap().hive, Hive::CurrentUser);
        assert_eq!(needs(GameId::Gitadora).unwrap().hive, Hive::CurrentUser);
        assert_eq!(needs(GameId::Popn).unwrap().hive, Hive::CurrentUser);
        assert_eq!(needs(GameId::Ddr).unwrap().hive, Hive::LocalMachine);
        assert!(needs(GameId::Sdvx).is_none());
        assert!(needs(GameId::BomberGirl).is_none());
    }

    #[test]
    fn the_dll_copy_never_replaces_an_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.dll");
        let target = directory.path().join("avs2-core.dll");
        std::fs::write(&source, b"new").unwrap();
        std::fs::write(&target, b"old").unwrap();
        copy_new(&source, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        std::fs::remove_file(&target).unwrap();
        copy_new(&source, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert!(!target.with_extension("dll.hub-part").exists());
    }

    #[test]
    fn supported_paths_require_a_verified_title_and_a_local_drive_root() {
        let root = Path::new(r"D:\Konaste\pop'n music Lively");
        assert!(title_paths_ok(GameId::Popn, root));
        // A verified title with a root the settings library cannot store.
        assert!(!title_paths_ok(GameId::Popn, Path::new(r"\\server\share\popn")));
        assert!(!title_paths_ok(GameId::Popn, Path::new("relative")));
        assert!(!title_paths_ok(GameId::Popn, Path::new(r"E:\ゲーム\popn")));
        // SDVX reads neither registry value, so no drive letter is required.
        assert!(title_paths_ok(GameId::Sdvx, Path::new(r"\\server\share\sdvx")));
        // INFINITAS is not a title and has no supported path.
        assert!(!title_paths_ok(GameId::Infinitas, root));
    }

    #[test]
    fn a_fresh_supported_install_derives_its_resource_without_registry() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("pop'n music Lively");
        std::fs::create_dir_all(&root).unwrap();
        // Play derives the same Resource folder the official installer would,
        // so a fresh install needs no prior Settings run or registry value.
        let resource = resource_dir(GameId::Popn, &root).unwrap().unwrap();
        assert!(resource.is_absolute());
        assert_eq!(resource, root.join("Resource"));
        assert_eq!(resource_dir(GameId::Ddr, &root).unwrap().unwrap(), root.join("Resource"));
        // Titles whose libraries read no official value derive nothing.
        assert_eq!(resource_dir(GameId::Sdvx, &root).unwrap(), None);
        assert_eq!(resource_dir(GameId::BomberGirl, &root).unwrap(), None);
        // The derived path creates the folders pop'n requires.
        create_work_folders(&resource).unwrap();
        for folder in WORK_FOLDERS {
            assert!(resource.join(folder).is_dir(), "{folder}");
        }
        // A canonical root with its own `\\?\` prefix still yields a plain,
        // absolute drive path ending in the `Resource\` the launcher reads.
        let canonical = root.canonicalize().unwrap();
        let from_canonical = resource_dir(GameId::Popn, &canonical).unwrap().unwrap();
        let text = from_canonical.to_string_lossy();
        assert!(from_canonical.is_absolute() && !text.starts_with(r"\\?\"), "{text}");
        assert!(text.ends_with(r"Resource\"), "{text}");
        // INFINITAS is not a title and derives nothing.
        assert!(resource_dir(GameId::Infinitas, &root).is_err());
    }

    #[test]
    fn play_creates_the_launchers_work_folders_and_keeps_existing_ones() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("pop'n music Lively");
        std::fs::create_dir_all(root.join("Resource").join("work")).unwrap();
        std::fs::write(root.join("Resource").join("work").join("keep.txt"), b"x").unwrap();
        let (_, resource) = values(&root).unwrap();
        create_work_folders(Path::new(&resource)).unwrap();
        for folder in super::WORK_FOLDERS {
            assert!(root.join("Resource").join(folder).is_dir(), "{folder}");
        }
        assert!(root.join("Resource").join("work").join("keep.txt").is_file());
        // Running again is a no-op.
        create_work_folders(Path::new(&resource)).unwrap();
        // A title whose libraries read no official value derives nothing, and no
        // work folders are ever created for it.
        let other = dir.path().join("sdvx");
        std::fs::create_dir_all(&other).unwrap();
        assert_eq!(resource_dir(GameId::Sdvx, &other).unwrap(), None);
        assert!(!other.join("Resource").exists());
    }

    #[test]
    fn remove_from_library_is_non_destructive() {
        // `forget` performs no delete and touches nothing on disk.
        let directory = tempfile::tempdir().unwrap();
        let before = std::fs::read_dir(directory.path()).unwrap().count();
        forget(GameId::Popn, directory.path());
        forget(GameId::Ddr, directory.path());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), before);
    }
}
