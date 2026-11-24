//! Authenticode verification through WinVerifyTrust.
//!
//! This is the only native call in the crate. It verifies a file signature.
//! It does not map the file as an image, run it, or talk to the installer.
//!
//! The action is `WINTRUST_ACTION_GENERIC_VERIFY_V2` with `WTD_UI_NONE`,
//! `WTD_REVOKE_NONE`, and `WTD_CACHE_ONLY_URL_RETRIEVAL`, so a check cannot
//! open a revocation-list connection. State is closed with
//! `WTD_STATEACTION_CLOSE` on every path that opened it.

use std::path::Path;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO, WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE,
    WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE, WinVerifyTrust,
};

use crate::error::BootstrapError;

/// Verify `path`.
///
/// Returns `Ok(None)`. `WinVerifyTrust` does not itself return the signer
/// display name, and this crate does not walk `CRYPT_PROVIDER_DATA` to invent
/// one. A trust result other than 0 is a rejection; "unsigned but hash-ok"
/// is not success. The recorded signer for these packages is Konami Amusement
/// Co., Ltd., but that string is not re-checked here.
pub(crate) fn verify_authenticode(path: &Path) -> Result<Option<String>, BootstrapError> {
    let wide = wide_path(path)?;
    let mut file_info = WINTRUST_FILE_INFO {
        cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: wide.as_ptr(),
        hFile: std::ptr::null_mut(),
        pgKnownSubject: std::ptr::null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
        pPolicyCallbackData: std::ptr::null_mut(),
        pSIPClientData: std::ptr::null_mut(),
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file_info },
        dwStateAction: WTD_STATEACTION_VERIFY,
        hWVTStateData: std::ptr::null_mut(),
        pwszURLReference: std::ptr::null_mut(),
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        dwUIContext: 0,
        pSignatureSettings: std::ptr::null_mut(),
    };

    // INVALID_HANDLE_VALUE means "no interactive user" per WinVerifyTrust docs.
    // windows-sys declares the action id as *mut GUID; the provider does not
    // write it. The copy is local so the crate constant is not mutated.
    let hwnd = (-1isize) as HWND;
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let status = unsafe { WinVerifyTrust(hwnd, &mut action, &mut data as *mut WINTRUST_DATA as *mut _) };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    unsafe {
        WinVerifyTrust(hwnd, &mut action, &mut data as *mut WINTRUST_DATA as *mut _);
    }
    if status != 0 {
        return Err(BootstrapError::signature(format!("WinVerifyTrust returned {status:#x} for {}", path.display())));
    }
    Ok(None)
}

fn wide_path(path: &Path) -> Result<Vec<u16>, BootstrapError> {
    use std::os::windows::ffi::OsStrExt;
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.is_empty() || wide.len() > 32_000 {
        return Err(BootstrapError::signature("path is empty or too long to verify"));
    }
    wide.push(0);
    Ok(wide)
}
