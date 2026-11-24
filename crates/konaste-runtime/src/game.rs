//! Direct creation of the verified title's pinned game module.
//!
//! This replaces the reporter's observed create-and-wait role. It does not launch
//! `errorreporter.exe`, upload a GameLog, or place the game in the SDK job. The
//! parameter string is the prepared graphics prefix, then the fixed ` -t ` prefix
//! plus the fresh callback token. There is no `--dev`, `--trial`, environment
//! token, or caller-supplied argv. Graphics arguments are prepared before this
//! call; `start` does not read the filesystem.
//! Drop closes the process handle and does not terminate the process.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, GetProcessId, INFINITE, WaitForSingleObject};
use windows_sys::Win32::UI::Shell::{SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, IsWindowVisible, SW_SHOWNORMAL};
use zeroize::Zeroize;

use crate::arguments::Arguments;
use crate::{InstalledGame, RuntimeError};

const VERB: &[u16] = &[b'o' as u16, b'p' as u16, b'e' as u16, b'n' as u16, 0];
/// `SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI`.
const MASK: u32 = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;

/// Owned process handle. The handle is not inheritable and is not a COM pointer.
/// Drop closes the handle and does not terminate the process.
pub(crate) struct GameProcess {
    handle: HANDLE,
}

// SAFETY: `handle` is a kernel process handle. It is used only by the owner,
// and no COM or window pointer is stored beside it.
unsafe impl Send for GameProcess {}

impl GameProcess {
    /// Completed wait, then the exit code. `259` is returned as a real exit
    /// code because the wait has already completed; it is not treated as
    /// `STILL_ACTIVE`.
    pub(crate) fn wait(&self) -> Result<u32, RuntimeError> {
        // SAFETY: `handle` is the process handle this value owns.
        let waited = unsafe { WaitForSingleObject(self.handle, INFINITE) };
        if waited == WAIT_FAILED || waited != WAIT_OBJECT_0 {
            return Err(RuntimeError::GameWait);
        }
        let mut code = 0_u32;
        // SAFETY: the wait completed, so the exit code is no longer the running
        // sentinel. The output pointer is a local `u32`.
        if unsafe { GetExitCodeProcess(self.handle, &mut code) } == 0 {
            return Err(RuntimeError::GameWait);
        }
        Ok(code)
    }

    pub(crate) fn wait_for_window(&self) -> bool {
        // SAFETY: the owned handle stays live throughout both waits and keeps
        // the process identity valid while we look for its top-level window.
        let pid = unsafe { GetProcessId(self.handle) };
        if pid == 0 {
            return false;
        }
        let mut status = unsafe { WaitForSingleObject(self.handle, 0) };
        while status == WAIT_TIMEOUT {
            if has_visible_window(pid) {
                return true;
            }
            status = unsafe { WaitForSingleObject(self.handle, 100) };
        }
        false
    }
}

fn has_visible_window(pid: u32) -> bool {
    unsafe extern "system" fn visit(window: HWND, data: LPARAM) -> i32 {
        // SAFETY: EnumWindows calls synchronously with the live stack context.
        let (pid, found) = unsafe { &mut *(data as *mut (u32, bool)) };
        let mut owner = 0;
        unsafe { GetWindowThreadProcessId(window, &mut owner) };
        if *pid == owner && unsafe { IsWindowVisible(window) } != 0 {
            *found = true;
            return 0;
        }
        1
    }
    let mut context = (pid, false);
    // SAFETY: the callback only accesses this context during EnumWindows.
    unsafe { EnumWindows(Some(visit), (&raw mut context) as LPARAM) };
    context.1
}

impl Drop for GameProcess {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: this value owns the handle. Closing it does not terminate
            // the process; no `TerminateProcess` call exists on this path.
            unsafe {
                CloseHandle(self.handle);
            }
            self.handle = std::ptr::null_mut();
        }
    }
}

/// Create the pinned game. `token` is the already validated callback token.
/// `args` was prepared before the host's final guard; this call does not read
/// settings, the registry, or any other file. The COM apartment exists only for
/// this call, on this thread.
pub(crate) fn start(game: &InstalledGame, token: &str, args: &Arguments) -> Result<GameProcess, RuntimeError> {
    let title = crate::title(game.family())?;
    let launch = title.launch()?;
    let mut parameters = parameters(args, token)?;
    let root = dos_path(&game.root().join(launch.working_directory))?;
    let image = dos_path(&game.root().join(title.module_path))?;
    let process = shell_start(&image, &root, &parameters);
    parameters.zeroize();
    process
}

pub(crate) fn shell_start(image: &[u16], root: &[u16], parameters: &[u16]) -> Result<GameProcess, RuntimeError> {
    if [image, root, parameters].iter().any(|value| value.last() != Some(&0) || value[..value.len() - 1].contains(&0)) {
        return Err(RuntimeError::GameStart);
    }
    let _apartment = Apartment::enter()?;
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: MASK,
        hwnd: std::ptr::null_mut(),
        lpVerb: VERB.as_ptr(),
        lpFile: image.as_ptr(),
        lpParameters: parameters.as_ptr(),
        lpDirectory: root.as_ptr(),
        nShow: SW_SHOWNORMAL,
        ..Default::default()
    };
    // SAFETY: every string is NUL-terminated and owned for the call. The verb is
    // `open`. The mask keeps the process handle and does not request elevation.
    // `NOASYNC` makes the call return only after creation has completed or failed.
    let created = unsafe { ShellExecuteExW(&mut info) };
    if created == 0 || info.hProcess.is_null() {
        if !info.hProcess.is_null() {
            unsafe { CloseHandle(info.hProcess) };
        }
        return Err(RuntimeError::GameStart);
    }
    // Shell's returned handle is not marked inheritable by this mask. Clear the
    // bit anyway so a later spawn cannot pass the game handle onward.
    unsafe {
        windows_sys::Win32::Foundation::SetHandleInformation(info.hProcess, windows_sys::Win32::Foundation::HANDLE_FLAG_INHERIT, 0);
    }
    Ok(GameProcess { handle: info.hProcess })
}

/// Prepared graphics prefix, then ` -t ` plus the token, then the typed
/// instrument suffix. The prefix and suffix are closed whitelists. No other
/// argument is appended and the token is not taken from argv. The wide buffer
/// is reserved to its exact final length before any token unit is written.
pub(crate) fn parameters(args: &Arguments, token: &str) -> Result<zeroize::Zeroizing<Vec<u16>>, RuntimeError> {
    if token.is_empty()
        || token.len() > 64
        || !token.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'+' | b'/' | b'='))
    {
        return Err(RuntimeError::Protocol);
    }
    let prefix = args.prefix();
    let suffix = args.suffix();
    // Both pieces are the closed ASCII whitelist. Their UTF-16 length is the
    // byte length, so the buffer is reserved before any token unit is written.
    if !prefix.is_ascii() || !suffix.is_ascii() {
        return Err(RuntimeError::GameStart);
    }
    let spaced = !suffix.is_empty();
    let mut wide = zeroize::Zeroizing::new(Vec::new());
    wide.reserve_exact(prefix.len() + 4 + token.len() + usize::from(spaced) + suffix.len() + 1);
    wide.extend(prefix.encode_utf16());
    drop(prefix);
    wide.extend_from_slice(&[b' ' as u16, b'-' as u16, b't' as u16, b' ' as u16]);
    wide.extend(token.encode_utf16());
    if spaced {
        wide.push(b' ' as u16);
        wide.extend(suffix.encode_utf16());
    }
    if wide.contains(&0) {
        return Err(RuntimeError::GameStart);
    }
    wide.push(0);
    Ok(wide)
}

/// Absolute path without a `\\?\` prefix. ShellExecute rejects that prefix on
/// the file and directory arguments. A path that does not fit the normal DOS
/// form is rejected rather than truncated.
pub(crate) fn dos_path(path: &Path) -> Result<Vec<u16>, RuntimeError> {
    if !path.is_absolute() {
        return Err(RuntimeError::GameStart);
    }
    let text = path.as_os_str();
    let mut wide: Vec<u16> = text.encode_wide().collect();
    if wide.first().copied() == Some(b'\\' as u16)
        && wide.get(1).copied() == Some(b'\\' as u16)
        && wide.get(2).copied() == Some(b'?' as u16)
        && wide.get(3).copied() == Some(b'\\' as u16)
    {
        // `\\?\C:\...` becomes `C:\...`. `\\?\UNC\` is not a drive path.
        if wide.len() > 6 && wide[5] == b':' as u16 {
            wide.drain(..4);
        } else {
            return Err(RuntimeError::GameStart);
        }
    }
    if wide.contains(&0) || wide.len() > 259 {
        return Err(RuntimeError::GameStart);
    }
    wide.push(0);
    Ok(wide)
}

/// Apartment for this thread only. `RPC_E_CHANGED_MODE` means the thread already
/// has one, so this call must not uninitialize it.
struct Apartment {
    owned: bool,
}

impl Apartment {
    fn enter() -> Result<Self, RuntimeError> {
        // SAFETY: reserved is null. The apartment is apartment-threaded, matching
        // a Shell call made on the thread that entered it.
        let result = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
        if result == windows_sys::Win32::Foundation::S_OK || result == windows_sys::Win32::Foundation::S_FALSE {
            Ok(Self { owned: true })
        } else if result == windows_sys::Win32::Foundation::RPC_E_CHANGED_MODE {
            Ok(Self { owned: false })
        } else {
            Err(RuntimeError::GameStart)
        }
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: paired with the `CoInitializeEx` that returned `S_OK` or
            // `S_FALSE` on this same thread.
            unsafe { CoUninitialize() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameters_are_the_fixed_prefix_and_nothing_else() {
        let token_only = Arguments::load(crate::title::ArgumentProfile::Token, None, std::path::Path::new(r"C:\")).unwrap();
        let wide = parameters(&token_only, "ab-CD_12").unwrap();
        let text = String::from_utf16(&wide[..wide.len() - 1]).unwrap();
        assert_eq!(text, " -t ab-CD_12");
        assert!(token_only.prefix().is_empty());
        assert!(wide.ends_with(&[0]));
        assert!(!text.contains("--dev") && !text.contains("--trial"));
        assert!(parameters(&token_only, "").is_err());
        assert!(parameters(&token_only, &"a".repeat(65)).is_err());
        assert!(parameters(&token_only, "has space").is_err());
        assert!(parameters(&token_only, "bad%value").is_err());
    }

    #[test]
    fn dos_paths_drop_only_the_drive_prefix_and_reject_truncation() {
        let wide = dos_path(Path::new(r"\\?\C:\games\bomber")).unwrap();
        assert_eq!(String::from_utf16(&wide[..wide.len() - 1]).unwrap(), r"C:\games\bomber");
        assert!(dos_path(Path::new(r"\\?\UNC\server\share")).is_err());
        assert!(dos_path(Path::new(r"relative\modules\bomber.exe")).is_err());
        let long = format!(r"C:\{}", "a".repeat(300));
        assert!(dos_path(Path::new(&long)).is_err());
    }

    #[test]
    fn only_a_visible_window_for_the_target_process_counts() {
        use windows_sys::Win32::System::Threading::GetCurrentProcessId;
        use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, SW_SHOWNOACTIVATE, ShowWindow, WS_POPUP};
        let class: Vec<u16> = "STATIC".encode_utf16().chain(Some(0)).collect();
        use std::ptr::{null, null_mut};
        // Keep the synthetic window off-screen and do not activate it.
        let window = unsafe {
            CreateWindowExW(0, class.as_ptr(), null(), WS_POPUP, -32000, -32000, 32, 32, null_mut(), null_mut(), null_mut(), null())
        };
        assert!(!window.is_null());
        let pid = unsafe { GetCurrentProcessId() };
        let hidden = has_visible_window(pid);
        unsafe { ShowWindow(window, SW_SHOWNOACTIVATE) };
        let shown = has_visible_window(pid);
        let wrong_process = has_visible_window(u32::MAX);
        unsafe { DestroyWindow(window) };
        assert!(!hidden);
        assert!(shown);
        assert!(!wrong_process);
    }

    #[test]
    fn a_completed_wait_preserves_the_real_259_exit_code() {
        use std::os::windows::io::AsRawHandle;
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        let system = std::env::var_os("SystemRoot").unwrap();
        let command = std::path::Path::new(&system).join("System32").join("cmd.exe");
        let mut child = Command::new(command)
            .args(["/D", "/C", "ping -n 2 127.0.0.1 >nul & exit 259"])
            .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
            .spawn()
            .unwrap();
        let handle = child.as_raw_handle() as HANDLE;
        // SAFETY: the handle is duplicated so dropping the session closes only
        // the duplicate. The original child is reaped separately below.
        let mut duplicate = std::ptr::null_mut();
        let opened = unsafe {
            windows_sys::Win32::Foundation::DuplicateHandle(
                windows_sys::Win32::System::Threading::GetCurrentProcess(),
                handle,
                windows_sys::Win32::System::Threading::GetCurrentProcess(),
                &mut duplicate,
                0,
                0,
                windows_sys::Win32::Foundation::DUPLICATE_SAME_ACCESS,
            )
        };
        assert_ne!(opened, 0);
        let session = GameProcess { handle: duplicate };
        assert!(!session.wait_for_window(), "the headless process exited without showing a window");
        assert_eq!(session.wait().unwrap(), 259);
        drop(session);
        let status = child.wait().unwrap();
        assert_eq!(status.code(), Some(259));
    }

    #[test]
    fn dropping_the_game_handle_does_not_kill_a_running_process() {
        use std::os::windows::{io::AsRawHandle, process::CommandExt};
        let system = std::env::var_os("SystemRoot").unwrap();
        let scratch = tempfile::tempdir().unwrap();
        std::fs::write(scratch.path().join("spin.cmd"), b"@echo off\r\n:again\r\ngoto again\r\n").unwrap();
        let mut child = std::process::Command::new(Path::new(&system).join("System32").join("cmd.exe"))
            .current_dir(scratch.path())
            .args(["/D", "/C", r".\spin.cmd"])
            .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
            .spawn()
            .unwrap();
        let mut duplicate = std::ptr::null_mut();
        // SAFETY: duplicate the owned child handle to exercise the production Drop.
        let copied = unsafe {
            windows_sys::Win32::Foundation::DuplicateHandle(
                windows_sys::Win32::System::Threading::GetCurrentProcess(),
                child.as_raw_handle().cast(),
                windows_sys::Win32::System::Threading::GetCurrentProcess(),
                &mut duplicate,
                0,
                0,
                windows_sys::Win32::Foundation::DUPLICATE_SAME_ACCESS,
            )
        };
        if copied != 0 {
            drop(GameProcess { handle: duplicate });
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        let still_running = child.try_wait().unwrap().is_none();
        // Only the synthetic fixture is terminated, after observing Drop behavior.
        let _ = child.kill();
        child.wait().unwrap();
        assert_ne!(copied, 0);
        assert!(still_running);
    }

    #[test]
    fn the_production_shell_creator_starts_only_a_synthetic_fixture() {
        let system = std::env::var_os("SystemRoot").unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let image = dos_path(&Path::new(&system).join("System32").join("cmd.exe")).unwrap();
        let root = dos_path(scratch.path()).unwrap();
        let parameters: Vec<u16> = " /D /C exit 259".encode_utf16().chain(Some(0)).collect();
        let process = shell_start(&image, &root, &parameters).unwrap();
        assert_eq!(process.wait().unwrap(), 259);
        assert!(matches!(shell_start(&[], &root, &parameters), Err(RuntimeError::GameStart)));
    }

    #[test]
    fn the_mask_keeps_the_handle_and_does_not_force_elevation() {
        assert_eq!(MASK & SEE_MASK_NOCLOSEPROCESS, SEE_MASK_NOCLOSEPROCESS);
        assert_eq!(MASK & SEE_MASK_NOASYNC, SEE_MASK_NOASYNC);
        assert_eq!(MASK & SEE_MASK_FLAG_NO_UI, SEE_MASK_FLAG_NO_UI);
        assert_eq!(SW_SHOWNORMAL, 1);
        let verb = String::from_utf16(&VERB[..VERB.len() - 1]).unwrap();
        assert_eq!(verb, "open");
    }
}
