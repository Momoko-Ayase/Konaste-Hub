//! Preparation of each title's official installed settings program.
//!
//! The pin comes from authenticated index metadata, never from an installed
//! `ri.bin` and never from a caller-chosen path. Preparation pins the install
//! directory chain and the title's fixed settings path, then checks that file's
//! size and digest. [`PreparedSettings::start`] shell-opens only that pinned
//! file, with an empty parameter string and the install root as the working
//! directory, and only after the caller's guard returns. There is no argv and
//! no caller-supplied executable. Drop closes the process handle and the file
//! pins. It does not terminate the process.

use std::path::{Path, PathBuf};

use konaste_bootstrap::GameFamily;
#[cfg(all(windows, target_arch = "x86_64"))]
use sha2::{Digest, Sha256};
#[cfg(all(windows, target_arch = "x86_64"))]
use std::fs::File;
#[cfg(all(windows, target_arch = "x86_64"))]
use std::io::Read;

use crate::RuntimeError;

/// Settings executable installed with SDVX's game content. The official
/// launcher's separate `launcher/modules/settings.exe` is a different file and
/// is not a substitute.
pub const SDVX_SETTINGS_PATH: &str = "game/modules/settings.exe";

/// Settings executable published in DDR, pop'n, GITADORA, and NOSTALGIA. Their
/// index has no separate game-content copy: this launcher module is the only
/// official settings program, so there is no game copy to prefer.
pub const LAUNCHER_SETTINGS_PATH: &str = "launcher/modules/settings.exe";

/// Trusted size and digest of one title's settings executable.
///
/// Discovery builds this by decoding the original authenticated index bytes.
/// A caller cannot substitute another path, and this type carries none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingsPin {
    pub bytes: u64,
    pub sha256: [u8; 32],
}

/// Fixed settings path for one family. There is no caller path.
///
/// SDVX ships both copies and the corrected target is the game-content one;
/// the other four titles publish only the launcher copy. Bombergirl ships no
/// settings program, so it stays unsupported.
pub fn settings_path(family: GameFamily) -> Result<&'static str, RuntimeError> {
    match family {
        GameFamily::Sdvx => Ok(SDVX_SETTINGS_PATH),
        GameFamily::Ddr | GameFamily::Popn | GameFamily::Gitadora | GameFamily::Nostalgia => Ok(LAUNCHER_SETTINGS_PATH),
        GameFamily::Bombergirl => Err(RuntimeError::Unsupported),
    }
}

/// Directory chain and settings file held read-only. Not a process and not a
/// grant to open any other executable.
pub struct PreparedSettings {
    root: PathBuf,
    #[cfg_attr(not(all(windows, target_arch = "x86_64")), allow(dead_code))]
    image: PathBuf,
    #[cfg(all(windows, target_arch = "x86_64"))]
    _file: File,
    #[cfg(all(windows, target_arch = "x86_64"))]
    _directories: Vec<File>,
}

/// Retained settings process and the pins that were checked before it existed.
/// Drop closes the handle and the pins. It does not terminate the process.
pub struct SettingsSession {
    _prepared: PreparedSettings,
    #[cfg(all(windows, target_arch = "x86_64"))]
    process: crate::game::GameProcess,
}

impl SettingsSession {
    /// Block until the editor shows a top-level window or the process exits.
    pub fn wait_for_window(&self) -> bool {
        #[cfg(all(windows, target_arch = "x86_64"))]
        {
            self.process.wait_for_window()
        }
        #[cfg(not(all(windows, target_arch = "x86_64")))]
        {
            false
        }
    }

    /// Block until the settings process exits and return that exit code. `259`
    /// is a real exit code here, not `STILL_ACTIVE`, because the wait completed.
    pub fn wait(&mut self) -> Result<u32, RuntimeError> {
        #[cfg(all(windows, target_arch = "x86_64"))]
        {
            self.process.wait()
        }
        #[cfg(not(all(windows, target_arch = "x86_64")))]
        {
            Err(RuntimeError::Unsupported)
        }
    }
}

/// Pin the install chain and the fixed settings file, then verify its size and
/// digest. Does not create a process. `pin` is the discovery pin for `family`;
/// the relative path is fixed and is not taken from the caller.
pub fn prepare_settings(family: GameFamily, root: &Path, pin: &SettingsPin) -> Result<PreparedSettings, RuntimeError> {
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        prepare_with(family, root, pin)
    }
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        let _ = (family, root, pin);
        Err(RuntimeError::Unsupported)
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn prepare_with(family: GameFamily, root: &Path, pin: &SettingsPin) -> Result<PreparedSettings, RuntimeError> {
    let relative = settings_path(family)?;
    if pin.bytes == 0 || pin.bytes > u32::MAX as u64 {
        return Err(RuntimeError::GameSettings);
    }
    if !root.is_dir() {
        return Err(RuntimeError::GameSettings);
    }
    let image = root.join(relative);
    // The chain is held before canonical spelling or the leaf open. An existing
    // reparse point on any ancestor fails inside the pin.
    let directories =
        crate::official::pin_root(image.parent().ok_or(RuntimeError::GameSettings)?).map_err(|_| RuntimeError::GameSettings)?;
    let root = root.canonicalize().map_err(|_| RuntimeError::GameSettings)?;
    let image = root.join(relative);
    let mut file = crate::official::pin_file(&image).map_err(|_| RuntimeError::GameSettings)?;
    let info = crate::official::file_information(&file).map_err(|_| RuntimeError::GameSettings)?;
    let size = (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow);
    if info.nNumberOfLinks != 1 || size != pin.bytes {
        return Err(RuntimeError::GameSettings);
    }
    if hash(&mut file, size)? != pin.sha256 {
        return Err(RuntimeError::GameSettings);
    }
    Ok(PreparedSettings { root, image, _file: file, _directories: directories })
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn hash(file: &mut File, expected_bytes: u64) -> Result<[u8; 32], RuntimeError> {
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let read = file.read(&mut buffer).map_err(|_| RuntimeError::GameSettings)?;
        if read == 0 {
            break;
        }
        total = total.checked_add(read as u64).ok_or(RuntimeError::GameSettings)?;
        if total > expected_bytes {
            return Err(RuntimeError::GameSettings);
        }
        digest.update(&buffer[..read]);
    }
    if total != expected_bytes {
        return Err(RuntimeError::GameSettings);
    }
    Ok(digest.finalize().into())
}

impl PreparedSettings {
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Shell-open the already pinned settings file after `final_guard` succeeds.
    /// The parameter string is empty and the working directory is the install
    /// root. A guard error is returned as itself and creates nothing.
    pub fn start<E: From<RuntimeError>>(self, final_guard: impl FnOnce() -> Result<(), E>) -> Result<SettingsSession, E> {
        #[cfg(all(windows, target_arch = "x86_64"))]
        {
            final_guard()?;
            let image = crate::game::dos_path(&self.image).map_err(E::from)?;
            let root = crate::game::dos_path(&self.root).map_err(E::from)?;
            // One NUL. No token, no graphics prefix, no caller arguments.
            let parameters = [0_u16];
            let process = open_settings(&image, &root, &parameters).map_err(E::from)?;
            Ok(SettingsSession { _prepared: self, process })
        }
        #[cfg(not(all(windows, target_arch = "x86_64")))]
        {
            let _ = (self, final_guard);
            Err(E::from(RuntimeError::Unsupported))
        }
    }
}

/// Production opens through the game shell. The launch test swaps this for a
/// recorder, so that one fixture never becomes a process.
#[cfg(all(windows, target_arch = "x86_64"))]
fn open_settings(image: &[u16], root: &[u16], parameters: &[u16]) -> Result<crate::game::GameProcess, RuntimeError> {
    #[cfg(not(test))]
    {
        crate::game::shell_start(image, root, parameters)
    }
    #[cfg(test)]
    {
        OPEN_SETTINGS.with(|open| open.get()(image, root, parameters))
    }
}

#[cfg(all(test, windows, target_arch = "x86_64"))]
type SettingsOpener = fn(&[u16], &[u16], &[u16]) -> Result<crate::game::GameProcess, RuntimeError>;

#[cfg(all(test, windows, target_arch = "x86_64"))]
std::thread_local! {
    static OPEN_SETTINGS: std::cell::Cell<SettingsOpener> = const { std::cell::Cell::new(crate::game::shell_start) };
}

#[cfg(all(test, windows, target_arch = "x86_64"))]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const BODY: &[u8] = b"synthetic-settings-bytes";

    fn pin_of(body: &[u8]) -> SettingsPin {
        SettingsPin { bytes: body.len() as u64, sha256: Sha256::digest(body).into() }
    }

    fn dos(path: &Path) -> String {
        let text = path.display().to_string();
        text.strip_prefix(r"\\?\").filter(|rest| rest.chars().nth(1) == Some(':')).unwrap_or(&text).to_string()
    }

    fn fixture() -> (tempfile::TempDir, SettingsPin) {
        let directory = tempfile::tempdir().unwrap();
        let image = directory.path().join(SDVX_SETTINGS_PATH);
        std::fs::create_dir_all(image.parent().unwrap()).unwrap();
        std::fs::write(&image, BODY).unwrap();
        (directory, pin_of(BODY))
    }

    fn recorded(image: &[u16], root: &[u16], parameters: &[u16]) -> Result<crate::game::GameProcess, RuntimeError> {
        let image = String::from_utf16(&image[..image.len() - 1]).unwrap();
        let root = String::from_utf16(&root[..root.len() - 1]).unwrap();
        let parameters = String::from_utf16(&parameters[..parameters.len() - 1]).unwrap();
        LAST.lock().unwrap().replace(Recorded { image, root, parameters });
        Err(RuntimeError::GameStart)
    }

    struct Recorded {
        image: String,
        root: String,
        parameters: String,
    }

    static LAST: Mutex<Option<Recorded>> = Mutex::new(None);

    /// The opener override and `LAST` are thread-local and global respectively,
    /// so the two `start` fixtures take turns.
    static START_TURN: Mutex<()> = Mutex::new(());

    fn start_turn() -> std::sync::MutexGuard<'static, ()> {
        START_TURN.lock().unwrap_or_else(|error| error.into_inner())
    }

    #[test]
    fn every_title_with_a_proven_settings_program_has_one_fixed_path() {
        assert_eq!(settings_path(GameFamily::Sdvx).unwrap(), "game/modules/settings.exe");
        assert!(!SDVX_SETTINGS_PATH.contains("launcher/modules"));
        for family in [GameFamily::Ddr, GameFamily::Popn, GameFamily::Gitadora, GameFamily::Nostalgia] {
            assert_eq!(settings_path(family).unwrap(), "launcher/modules/settings.exe");
        }
        // Bombergirl publishes no settings program.
        assert_eq!(settings_path(GameFamily::Bombergirl), Err(RuntimeError::Unsupported));
        for family in GameFamily::ALL {
            let supported = family != GameFamily::Bombergirl;
            assert_eq!(settings_path(family).is_ok(), supported);
        }
    }

    #[test]
    fn settings_paths_stay_inside_the_install_root() {
        for family in GameFamily::ALL {
            let Ok(path) = settings_path(family) else {
                continue;
            };
            assert!(!path.starts_with('/') && !path.contains(['\\', ':']));
            assert!(path.split('/').all(|part| !part.is_empty() && part != "." && part != ".."));
            assert!(path.ends_with("/settings.exe"));
        }
    }

    #[test]
    fn game_content_settings_is_prepared_without_a_launcher_installation() {
        let directory = tempfile::tempdir().unwrap();
        let image = directory.path().join("game/modules/settings.exe");
        std::fs::create_dir_all(image.parent().unwrap()).unwrap();
        std::fs::write(&image, BODY).unwrap();
        assert!(!directory.path().join("launcher").exists());
        let prepared = prepare_settings(GameFamily::Sdvx, directory.path(), &pin_of(BODY))
            .expect("Hub installs the game settings executable, not the official launcher settings copy");
        assert_eq!(prepared.image, image.canonicalize().unwrap());
    }

    #[test]
    fn a_matching_file_is_pinned_against_replacement() {
        let (directory, pin) = fixture();
        let prepared = prepare_with(GameFamily::Sdvx, directory.path(), &pin).unwrap();
        assert_eq!(prepared.image, directory.path().canonicalize().unwrap().join(SDVX_SETTINGS_PATH));
        assert!(std::fs::write(directory.path().join(SDVX_SETTINGS_PATH), b"changed").is_err());
        drop(prepared);
        std::fs::write(directory.path().join(SDVX_SETTINGS_PATH), b"changed").unwrap();
    }

    #[test]
    fn digest_size_link_reparse_and_other_paths_are_refused() {
        let (directory, pin) = fixture();
        let wrong = SettingsPin { bytes: pin.bytes, sha256: [9; 32] };
        assert_eq!(prepare_with(GameFamily::Sdvx, directory.path(), &wrong).map(|_| ()), Err(RuntimeError::GameSettings));
        let wrong = SettingsPin { bytes: pin.bytes + 1, sha256: pin.sha256 };
        assert_eq!(prepare_with(GameFamily::Sdvx, directory.path(), &wrong).map(|_| ()), Err(RuntimeError::GameSettings));
        assert_eq!(
            prepare_with(GameFamily::Sdvx, directory.path(), &SettingsPin { bytes: 0, sha256: pin.sha256 },).map(|_| ()),
            Err(RuntimeError::GameSettings)
        );

        let image = directory.path().join(SDVX_SETTINGS_PATH);
        std::fs::hard_link(&image, directory.path().join("settings-alias")).unwrap();
        assert_eq!(prepare_with(GameFamily::Sdvx, directory.path(), &pin).map(|_| ()), Err(RuntimeError::GameSettings));
        std::fs::remove_file(directory.path().join("settings-alias")).unwrap();

        let elsewhere = tempfile::tempdir().unwrap();
        std::fs::write(elsewhere.path().join("settings.exe"), BODY).unwrap();
        let modules = directory.path().join("game").join("modules");
        std::fs::remove_dir_all(&modules).unwrap();
        let status =
            std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&modules).arg(elsewhere.path()).status().expect("mklink");
        assert!(status.success(), "junction fixture");
        assert_eq!(prepare_with(GameFamily::Sdvx, directory.path(), &pin).map(|_| ()), Err(RuntimeError::GameSettings));

        let wrong_tree = tempfile::tempdir().unwrap();
        let game = wrong_tree.path().join("launcher/modules/settings.exe");
        std::fs::create_dir_all(game.parent().unwrap()).unwrap();
        std::fs::write(&game, BODY).unwrap();
        assert_eq!(prepare_with(GameFamily::Sdvx, wrong_tree.path(), &pin).map(|_| ()), Err(RuntimeError::GameSettings));
        assert_eq!(prepare_with(GameFamily::Bombergirl, directory.path(), &pin).map(|_| ()), Err(RuntimeError::Unsupported));
    }

    fn launcher_fixture() -> (tempfile::TempDir, SettingsPin) {
        let directory = tempfile::tempdir().unwrap();
        let image = directory.path().join(LAUNCHER_SETTINGS_PATH);
        std::fs::create_dir_all(image.parent().unwrap()).unwrap();
        std::fs::write(&image, BODY).unwrap();
        (directory, pin_of(BODY))
    }

    #[test]
    fn launcher_titles_prepare_their_single_published_copy() {
        for family in [GameFamily::Ddr, GameFamily::Popn, GameFamily::Gitadora, GameFamily::Nostalgia] {
            let (directory, pin) = launcher_fixture();
            let prepared = prepare_with(family, directory.path(), &pin).unwrap_or_else(|error| panic!("{family:?}: {error:?}"));
            assert_eq!(prepared.image, directory.path().canonicalize().unwrap().join(LAUNCHER_SETTINGS_PATH));
        }
    }

    #[test]
    fn a_launcher_title_refuses_the_sdvx_game_copy_and_a_pin_mismatch() {
        for family in [GameFamily::Ddr, GameFamily::Popn, GameFamily::Gitadora, GameFamily::Nostalgia] {
            // Only the SDVX game-content copy exists: not this title's target.
            let (directory, pin) = fixture();
            assert_eq!(prepare_with(family, directory.path(), &pin).map(|_| ()), Err(RuntimeError::GameSettings));

            let (directory, pin) = launcher_fixture();
            let wrong = SettingsPin { bytes: pin.bytes, sha256: [9; 32] };
            assert_eq!(prepare_with(family, directory.path(), &wrong).map(|_| ()), Err(RuntimeError::GameSettings));
        }
    }

    #[test]
    fn launcher_settings_start_with_empty_arguments_and_the_install_root() {
        let (directory, pin) = launcher_fixture();
        let _turn = start_turn();
        let prepared = prepare_with(GameFamily::Gitadora, directory.path(), &pin).unwrap();
        let root = prepared.root().to_path_buf();
        let image = prepared.image.clone();
        LAST.lock().unwrap().take();
        OPEN_SETTINGS.with(|open| open.set(recorded));
        let started = prepared.start(|| Ok::<(), RuntimeError>(()));
        OPEN_SETTINGS.with(|open| open.set(crate::game::shell_start));
        assert_eq!(started.err(), Some(RuntimeError::GameStart));
        let recorded = LAST.lock().unwrap().take().unwrap();
        assert_eq!(recorded.parameters, "");
        assert_eq!(recorded.image, dos(&image));
        assert_eq!(recorded.root, dos(&root));
        assert!(recorded.image.replace('\\', "/").ends_with(LAUNCHER_SETTINGS_PATH));
    }

    #[test]
    fn start_records_the_fixed_target_and_runs_nothing() {
        let _turn = start_turn();
        let (directory, pin) = fixture();
        let prepared = prepare_with(GameFamily::Sdvx, directory.path(), &pin).unwrap();
        let root = prepared.root().to_path_buf();
        let image = prepared.image.clone();
        #[derive(Debug, PartialEq, Eq)]
        struct GuardFailed;
        impl From<RuntimeError> for GuardFailed {
            fn from(_: RuntimeError) -> Self {
                Self
            }
        }
        OPEN_SETTINGS.with(|open| open.set(recorded));
        let refused = prepared.start(|| Err(GuardFailed));
        OPEN_SETTINGS.with(|open| open.set(crate::game::shell_start));
        assert_eq!(refused.err(), Some(GuardFailed));
        assert!(LAST.lock().unwrap().is_none());

        let prepared = prepare_with(GameFamily::Sdvx, directory.path(), &pin).unwrap();
        OPEN_SETTINGS.with(|open| open.set(recorded));
        let started = prepared.start(|| Ok::<(), RuntimeError>(()));
        OPEN_SETTINGS.with(|open| open.set(crate::game::shell_start));
        assert_eq!(started.err(), Some(RuntimeError::GameStart));
        let recorded = LAST.lock().unwrap().take().unwrap();
        assert_eq!(recorded.parameters, "");
        assert_eq!(recorded.image, dos(&image));
        assert_eq!(recorded.root, dos(&root));
        let image = recorded.image.replace('\\', "/");
        assert!(image.ends_with(SDVX_SETTINGS_PATH));
        assert!(!image.contains("launcher/modules"));
        assert!(!recorded.parameters.contains("-t"));
    }
}
