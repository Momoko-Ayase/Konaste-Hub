use crate::{catalog::GameDescriptor, state::HubError};

use std::path::{Path, PathBuf};

/// A registry hive a detected installation may live in. The official launchers
/// read the current user's hive first and fall back to the machine hive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

/// Read one title's `InstallDir` from the official key, HKCU first, then HKLM,
/// in the 64-bit view. Read-only; the hub never writes the registry.
pub fn detect_root(game: &GameDescriptor) -> Option<PathBuf> {
    detect_root_with(game, &read_install_dir)
}

/// Test seam: `read(hive, key)` returns the raw `InstallDir` string.
pub fn detect_root_with(game: &GameDescriptor, read: &dyn Fn(Hive, &str) -> Option<String>) -> Option<PathBuf> {
    let key = game.registry?;
    let usable = |hive: Hive| read(hive, key).filter(|raw| !raw.trim().is_empty() && !raw.contains('\0'));
    // HKCU wins; a blank or unreadable HKCU value falls back to HKLM.
    usable(Hive::CurrentUser).or_else(|| usable(Hive::LocalMachine)).map(PathBuf::from)
}

#[cfg(windows)]
fn read_install_dir(hive: Hive, subkey: &str) -> Option<String> {
    use winreg::{
        RegKey,
        enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY},
    };
    let root = match hive {
        Hive::CurrentUser => RegKey::predef(HKEY_CURRENT_USER),
        Hive::LocalMachine => RegKey::predef(HKEY_LOCAL_MACHINE),
    };
    let key = root.open_subkey_with_flags(subkey, KEY_READ | KEY_WOW64_64KEY).ok()?;
    key.get_value("InstallDir").ok()
}
#[cfg(not(windows))]
fn read_install_dir(_: Hive, _: &str) -> Option<String> {
    None
}

#[cfg(windows)]
fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
pub fn product_version(path: &Path) -> Option<String> {
    use windows_sys::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
    let filename = wide(path.as_os_str());
    let length = unsafe { GetFileVersionInfoSizeW(filename.as_ptr(), std::ptr::null_mut()) };
    if length == 0 || length > 1024 * 1024 {
        return None;
    }
    let mut buffer = vec![0_u8; length as usize];
    if unsafe { GetFileVersionInfoW(filename.as_ptr(), 0, length, buffer.as_mut_ptr().cast()) } == 0 {
        return None;
    }
    for language in ["041104b0", "040904b0"] {
        let key = format!("\\StringFileInfo\\{language}\\ProductVersion");
        let key = wide(std::ffi::OsStr::new(&key));
        let mut value = std::ptr::null_mut();
        let mut count = 0_u32;
        if unsafe { VerQueryValueW(buffer.as_ptr().cast(), key.as_ptr(), &mut value, &mut count) } != 0
            && !value.is_null()
            && count > 0
            && count <= 256
        {
            let text = unsafe { std::slice::from_raw_parts(value.cast::<u16>(), count as usize) };
            return Some(String::from_utf16_lossy(text).trim_end_matches('\0').to_owned());
        }
    }
    None
}
#[cfg(not(windows))]
pub fn product_version(_: &Path) -> Option<String> {
    None
}

#[cfg(windows)]
pub fn replace_file(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW};
    let from = wide(from.as_os_str());
    let to = wide(to.as_os_str());
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
#[cfg(not(windows))]
pub fn replace_file(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    std::fs::rename(from, to)
}

/// Open a URL or folder with its default handler.
///
/// Runs on its own thread. ShellExecuteW can pump window messages while it
/// works; called from a gpui click handler, those messages re-enter gpui's
/// window procedure mid-dispatch and gpui aborts the process. The thread also
/// initializes COM, which ShellExecuteW expects of its caller.
#[cfg(windows)]
fn shell_open(target: &std::ffi::OsStr, code: &'static str) -> Result<(), HubError> {
    use windows_sys::Win32::{
        System::Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize},
        UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    };
    let target = wide(target);
    std::thread::Builder::new()
        .name("shell-open".into())
        .spawn(move || {
            let operation = wide(std::ffi::OsStr::new("open"));
            let com = unsafe { CoInitializeEx(std::ptr::null(), (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32) };
            let result = unsafe {
                ShellExecuteW(std::ptr::null_mut(), operation.as_ptr(), target.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL)
            };
            if com >= 0 {
                unsafe { CoUninitialize() };
            }
            if result as usize <= 32 {
                crate::log::line(format!("shell open failed: {code} ({})", result as usize));
            }
        })
        .map(|_| ())
        .map_err(|_| HubError { code })
}

#[cfg(windows)]
pub fn open_folder(root: &Path) -> Result<(), HubError> {
    if !root.is_dir() {
        return Err(HubError { code: "notAnInstallation" });
    }
    shell_open(root.as_os_str(), "filesystem")
}

/// Open an official https page in the default browser.
#[cfg(windows)]
pub fn open_url(url: &str) -> Result<(), HubError> {
    let parsed = url::Url::parse(url).map_err(|_| HubError { code: "browserUnavailable" })?;
    if parsed.scheme() != "https" {
        return Err(HubError { code: "browserUnavailable" });
    }
    shell_open(std::ffi::OsStr::new(parsed.as_str()), "browserUnavailable")
}
#[cfg(not(windows))]
pub fn open_url(_: &str) -> Result<(), HubError> {
    Err(HubError { code: "windowsOnly" })
}

#[cfg(windows)]
pub fn free_space(path: &Path) -> Option<u64> {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let path = wide(path.as_os_str());
    let mut free = 0_u64;
    let ok = unsafe { GetDiskFreeSpaceExW(path.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(free)
}
#[cfg(not(windows))]
pub fn free_space(_: &Path) -> Option<u64> {
    None
}

/// Encrypt for the current Windows user (DPAPI).
#[cfg(windows)]
pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
    dpapi(data, true)
}
#[cfg(windows)]
pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
    dpapi(data, false)
}
#[cfg(not(windows))]
pub fn protect(_: &[u8]) -> Option<Vec<u8>> {
    None
}
#[cfg(not(windows))]
pub fn unprotect(_: &[u8]) -> Option<Vec<u8>> {
    None
}

#[cfg(windows)]
fn dpapi(data: &[u8], encrypt: bool) -> Option<Vec<u8>> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData},
    };
    let input = CRYPT_INTEGER_BLOB { cbData: u32::try_from(data.len()).ok()?, pbData: data.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    if ok == 0 || output.pbData.is_null() {
        return None;
    }
    let result = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(output.pbData.cast());
    }
    Some(result)
}

/// Default folder for new installs.
pub fn default_games_folder() -> PathBuf {
    std::env::var_os("SystemDrive")
        .map(|drive| PathBuf::from(format!("{}\\", drive.to_string_lossy())))
        .unwrap_or_else(|| PathBuf::from("C:\\"))
        .join("Games")
}

/// Must match `HubMutex` in `installer/konaste-hub.iss`.
pub const APP_MUTEX: &str = "KonasteHub";

/// Hold a named mutex for the life of every Hub process, including headless
/// runs and runtime workers, so Setup and the uninstaller can tell the Hub is
/// running. Windows closes the handle when the process exits.
#[cfg(windows)]
pub fn hold_app_mutex() {
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let name = wide(std::ffi::OsStr::new(APP_MUTEX));
    // Intentionally leaked: the handle must outlive `main`.
    let _ = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
}

#[cfg(not(windows))]
pub fn hold_app_mutex() {}

pub fn data_directory() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(std::env::temp_dir)
        .join("konaste-hub")
}

#[cfg(not(windows))]
pub fn open_folder(_: &Path) -> Result<(), HubError> {
    Err(HubError { code: "windowsOnly" })
}

#[cfg(windows)]
pub fn game_running(executable: &Path) -> Result<bool, HubError> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS},
            Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW},
        },
    };
    let expected_name = executable.file_name().ok_or(HubError { code: "notAnInstallation" })?.to_string_lossy().to_lowercase();
    let expected_path = executable.to_string_lossy().trim_start_matches(r"\\?\").to_lowercase();
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(HubError { code: "processCheck" });
    }
    let result = (|| {
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
        while found {
            let count = entry.szExeFile.iter().position(|ch| *ch == 0).unwrap_or(entry.szExeFile.len());
            // No threads: the process has exited and only an open handle (for
            // example Windows Error Reporting after a crash) keeps it listed.
            if entry.cntThreads > 0 && String::from_utf16_lossy(&entry.szExeFile[..count]).to_lowercase() == expected_name {
                let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID) };
                let actual = if handle.is_null() {
                    // Access denied still allows the kernel image name, in NT form.
                    let nt = nt_image_path(entry.th32ProcessID).ok_or(HubError { code: "processCheck" })?;
                    if nt.to_lowercase() == nt_form(&expected_path).ok_or(HubError { code: "processCheck" })?.to_lowercase() {
                        return Ok(true);
                    }
                    found = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
                    continue;
                } else {
                    let mut path = vec![0_u16; 32768];
                    let mut length = path.len() as u32;
                    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &mut length) } != 0;
                    unsafe {
                        CloseHandle(handle);
                    }
                    if !ok {
                        return Err(HubError { code: "processCheck" });
                    }
                    String::from_utf16_lossy(&path[..length as usize])
                };
                if actual.trim_start_matches(r"\\?\").to_lowercase() == expected_path {
                    return Ok(true);
                }
            }
            found = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
        }
        Ok(false)
    })();
    unsafe {
        CloseHandle(snapshot);
    }
    result
}

/// Image path of `pid` as `\Device\HarddiskVolumeN\...`. Needs no process handle.
#[cfg(windows)]
fn nt_image_path(pid: u32) -> Option<String> {
    use windows_sys::{Wdk::System::SystemInformation::NtQuerySystemInformation, Win32::Foundation::UNICODE_STRING};
    #[repr(C)]
    struct ProcessIdInformation {
        process_id: usize,
        image_name: UNICODE_STRING,
    }
    const SYSTEM_PROCESS_ID_INFORMATION: i32 = 88;
    let mut buffer = vec![0_u16; 32768];
    let mut info = ProcessIdInformation {
        process_id: pid as usize,
        image_name: UNICODE_STRING { Length: 0, MaximumLength: (buffer.len() * 2 - 2) as u16, Buffer: buffer.as_mut_ptr() },
    };
    let status = unsafe {
        NtQuerySystemInformation(
            SYSTEM_PROCESS_ID_INFORMATION,
            (&raw mut info).cast(),
            size_of::<ProcessIdInformation>() as u32,
            std::ptr::null_mut(),
        )
    };
    if status != 0 {
        return None;
    }
    let length = usize::from(info.image_name.Length / 2).min(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..length]))
}

/// `E:\dir\file` as `\Device\HarddiskVolumeN\dir\file`, through the drive's DOS device link.
#[cfg(windows)]
fn nt_form(path: &str) -> Option<String> {
    use windows_sys::Win32::Storage::FileSystem::QueryDosDeviceW;
    let bytes = path.as_bytes();
    if bytes.len() < 3 || bytes[1] != b':' || bytes[2] != b'\\' || !bytes[0].is_ascii_alphabetic() {
        return None;
    }
    let drive = wide(std::ffi::OsStr::new(&path[..2]));
    let mut target = vec![0_u16; 1024];
    let written = unsafe { QueryDosDeviceW(drive.as_ptr(), target.as_mut_ptr(), target.len() as u32) };
    if written == 0 {
        return None;
    }
    let end = target.iter().position(|ch| *ch == 0)?;
    Some(format!("{}{}", String::from_utf16_lossy(&target[..end]), &path[2..]))
}
#[cfg(not(windows))]
pub fn game_running(_: &Path) -> Result<bool, HubError> {
    Err(HubError { code: "windowsOnly" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn sdvx() -> &'static GameDescriptor {
        crate::catalog::descriptor(crate::catalog::GameId::Sdvx)
    }

    type RegistryFixture = Box<dyn Fn(Hive, &str) -> Option<String>>;

    fn fixture(entries: &[(Hive, &str, &str)]) -> RegistryFixture {
        let mut map = BTreeMap::new();
        for (hive, key, value) in entries {
            map.insert((*hive, key.to_string()), value.to_string());
        }
        Box::new(move |hive, key| map.get(&(hive, key.to_string())).cloned())
    }

    #[test]
    #[cfg(windows)]
    fn the_app_mutex_is_visible_to_setup() {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE},
        };
        hold_app_mutex();
        let name = wide(std::ffi::OsStr::new(APP_MUTEX));
        let handle = unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, name.as_ptr()) };
        assert!(!handle.is_null(), "Setup's CheckForMutexes would miss a running Hub");
        unsafe { CloseHandle(handle) };
        let iss = include_str!("../../../installer/konaste-hub.iss");
        assert!(iss.contains(&format!("HubMutex = '{APP_MUTEX}';")) && iss.contains("CheckForMutexes(HubMutex)"));
    }

    #[test]
    fn the_running_check_finds_its_own_process_and_ignores_other_copies() {
        let own = std::env::current_exe().unwrap();
        assert!(game_running(&own).unwrap());
        let elsewhere = std::env::temp_dir().join(own.file_name().unwrap());
        assert!(!game_running(&elsewhere).unwrap());
        let nt = nt_image_path(std::process::id()).unwrap();
        let dos = own.to_string_lossy().trim_start_matches(r"\\?\").to_string();
        assert_eq!(nt.to_lowercase(), nt_form(&dos).unwrap().to_lowercase());
    }

    #[test]
    fn detection_reads_the_current_user_hive_before_the_machine_hive() {
        let read = fixture(&[
            (Hive::CurrentUser, sdvx().registry.unwrap(), r"D:\PerUser\SDVX"),
            (Hive::LocalMachine, sdvx().registry.unwrap(), r"D:\Konaste\SDVX"),
        ]);
        assert_eq!(detect_root_with(sdvx(), &read), Some(PathBuf::from(r"D:\PerUser\SDVX")));
    }

    #[test]
    fn detection_falls_back_to_the_machine_hive_and_rejects_empty_values() {
        let machine_only = fixture(&[(Hive::LocalMachine, sdvx().registry.unwrap(), r"D:\Konaste\SDVX")]);
        assert_eq!(detect_root_with(sdvx(), &machine_only), Some(PathBuf::from(r"D:\Konaste\SDVX")));
        let empty = fixture(&[
            (Hive::CurrentUser, sdvx().registry.unwrap(), "  "),
            (Hive::LocalMachine, sdvx().registry.unwrap(), r"D:\Konaste\SDVX"),
        ]);
        assert_eq!(detect_root_with(sdvx(), &empty), Some(PathBuf::from(r"D:\Konaste\SDVX")));
        assert_eq!(detect_root_with(sdvx(), &fixture(&[])), None);
    }
}
