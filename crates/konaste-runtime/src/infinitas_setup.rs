//! Run the pinned INFINITAS launcher bootstrap with Windows Installer.
//! This does not launch INFINITAS, perform game-data downloads, or write
//! any KONAMI registry values itself.

use std::path::Path;

use konaste_bootstrap::BootstrapError;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Downloading(u64),
    Wizard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    LauncherInstalled,
    RestartRequired,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Package(#[from] BootstrapError),
    #[error("Windows already has an INFINITAS setup registration")]
    Registered,
    #[error("could not stage or open Windows Installer")]
    Open,
    #[error("Windows Installer returned {0}")]
    Exit(u32),
    #[error("the INFINITAS launcher was not found after setup")]
    MissingLauncher,
}

pub fn is_registered() -> bool {
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        windows::is_registered()
    }
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        false
    }
}

/// Blocking. The caller must obtain explicit consent for downloading and
/// opening KONAMI's installer. The wizard chooses the destination and handles UAC.
pub fn install(data: &Path, consent: bool, progress: impl FnMut(Stage)) -> Result<Outcome, Error> {
    if !consent {
        return Err(BootstrapError::ConsentRequired.into());
    }
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        windows::install(data, progress)
    }
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        let _ = (data, progress);
        Err(BootstrapError::UnsupportedPlatform.into())
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
mod windows {
    use super::*;
    use crate::official::{file_information, pin_file, pin_root};
    use konaste_bootstrap::infinitas as package;
    use std::{
        ffi::OsString,
        os::windows::ffi::OsStringExt,
        path::{Component, PathBuf, Prefix},
        process::Command,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::System::{
        ApplicationInstallationAndServicing::{INSTALLSTATE_UNKNOWN, MsiQueryProductStateW},
        SystemInformation::GetSystemDirectoryW,
    };

    const PRODUCT: &str = "{86BE9AFB-152B-4298-A9C9-D6C7A59C75CA}";

    pub(super) fn is_registered() -> bool {
        let code: Vec<u16> = PRODUCT.encode_utf16().chain(Some(0)).collect();
        // SAFETY: `code` is the fixed product GUID as a NUL-terminated string.
        let state = unsafe { MsiQueryProductStateW(code.as_ptr()) };
        state != INSTALLSTATE_UNKNOWN || super::super::registered_install_dir().is_some()
    }

    pub(super) fn install(data: &Path, mut progress: impl FnMut(Stage)) -> Result<Outcome, Error> {
        if is_registered() {
            return Err(Error::Registered);
        }
        let _data_pins = pin_root(data).map_err(|_| Error::Open)?;
        let scratch = tempfile::Builder::new().prefix("infinitas-msi-").tempdir_in(data).map_err(|_| Error::Open)?;
        let _scratch_pins = pin_root(scratch.path()).map_err(|_| Error::Open)?;
        let msi = scratch.path().join("infinitas_installer_2022060800.msi");
        let mut last = Instant::now();
        package::download(&msi, true, |bytes| {
            if last.elapsed() >= Duration::from_millis(200) || bytes.received == package::MSI_BYTES {
                progress(Stage::Downloading(bytes.received));
                last = Instant::now();
            }
        })?;
        let file_pin = pin_file(&msi).map_err(|_| Error::Open)?;
        if file_information(&file_pin).map_err(|_| Error::Open)?.nNumberOfLinks != 1 {
            return Err(Error::Open);
        }
        package::verify(&msi)?;
        if is_registered() {
            return Err(Error::Registered);
        }
        let msi = msi_argument(&msi)?;
        let system_dir = system_directory()?;
        progress(Stage::Wizard);
        // Keep the MSI and directory pins alive until Windows Installer exits.
        let status = installer_command(&system_dir, &msi).status().map_err(|_| Error::Open)?;
        match status.code().map(|code| code as u32) {
            Some(0) if super::super::registered_install_dir().is_some_and(|root| root.join(super::super::LAUNCHER_IMAGE).is_file()) => {
                Ok(Outcome::LauncherInstalled)
            }
            Some(0) => Err(Error::MissingLauncher),
            Some(1641 | 3010) => Ok(Outcome::RestartRequired),
            Some(code) => Err(Error::Exit(code)),
            None => Err(Error::Open),
        }
    }

    fn system_directory() -> Result<PathBuf, Error> {
        let mut buffer = [0u16; 260];
        // SAFETY: `buffer` is writable and the length is in UTF-16 units.
        let len = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
        if len == 0 || len >= buffer.len() {
            return Err(Error::Open);
        }
        Ok(PathBuf::from(OsString::from_wide(&buffer[..len])))
    }

    fn msi_argument(path: &Path) -> Result<OsString, Error> {
        let wide = crate::game::dos_path(path).map_err(|_| Error::Open)?;
        let argument = OsString::from_wide(&wide[..wide.len() - 1]);
        let path = Path::new(&argument);
        if !matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_))) {
            return Err(Error::Open);
        }
        Ok(argument)
    }

    fn installer_command(system: &Path, msi: &std::ffi::OsStr) -> Command {
        let mut command = Command::new(system.join("msiexec.exe"));
        command.current_dir(system).args([std::ffi::OsStr::new("/i"), msi]);
        command
    }

    #[test]
    fn invokes_absolute_system_installer_with_full_ui_and_no_properties() {
        let system = Path::new(r"C:\Windows\System32");
        let msi = msi_argument(Path::new(r"\\?\D:\scratch pad\infinitas.msi")).unwrap();
        let command = installer_command(system, &msi);
        assert_eq!(command.get_program(), system.join("msiexec.exe").as_os_str());
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["/i", r"D:\scratch pad\infinitas.msi"]);
        assert!(msi_argument(Path::new(r"\\host\share\package.msi")).is_err());
    }
}
