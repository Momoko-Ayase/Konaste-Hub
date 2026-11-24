//! Windows x64 pinned AVS worker for one admitted title.
//!
//! Discovery and one-attempt launch verification share this loader. Both load the
//! two pinned launcher DLLs already copied into a fresh scratch directory and boot
//! AVS with fixed relative raw/nvram mounts under that working directory. Neither
//! mounts an installation, redirects CRT stdio, downloads, or creates a game.
//! The admitted title supplies the game id and the updater and launcher versions.
//! Caller input does not. An unadmitted family fails before scratch or package use.
//!
//! Discovery is mode 2. It uses the fixed non-account placeholder and asks
//! `libeacnet` for the public resource URL and hash. Mode 2 skips the version-check
//! cases and goes to the normal non-zero-mode terminal. It does not call a module,
//! game-start, account, or subscription export, and it does not read private image
//! offsets.
//!
//! Verification is the normal mode-1 launcher sequence: version check, heartbeat,
//! then the non-zero-mode terminal. It then asks for the installed game module hash
//! and `checkGameStart`. It does not call download, account-list, or subscription
//! exports, and it does not create a game or error-reporter process. A successful
//! return is `Ok(())` for that one attempt, not a token and not a reusable grant.
//! Synthetic adapter tests do not establish a live authorization.
//!
//! Official boot success is worker-finished (`OA043711000001 == 2`), error 0, and
//! `OA043711000009 == 1`. Result 2 is a boot error or heartbeat exit and is not
//! accepted. Resource, module, and start polls return 2 after the request has
//! already been released, on both success and failure, so each is called once per
//! observation and never again after 2. Error 0 is required after that 2.
//! `OA043711000040` is not a zero-enqueue export: this pinned library returns 1 on
//! its paths, including request failure, and the official caller treats any
//! nonzero value as sent. Error 0 is still required before the start poll.
//!
//! The 128 MiB AVS buffer, both module handles, and the pinning file handles are
//! intentionally left alive until process exit. Official threads may still hold
//! them after shutdown. `eac_shutdown` runs once on every path that initialized
//! the network library.

use std::ffi::CStr;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Component, Path};
use std::time::{Duration, Instant};

use zeroize::{Zeroize, Zeroizing};

use sha2::{Digest, Sha256};
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
    FILE_SHARE_WRITE, GetFileInformationByHandle,
};
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows_sys::Win32::System::Memory::{MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc};

use konaste_bootstrap::{GameFamily, package_for};

use crate::{PROTOCOL, Receipt, RuntimeError};

/// `updater.exe` `sub_140017F49`: descriptor size passed to property create.
const PROPERTY_DESCRIPTOR_BYTES: usize = 0xD18;
/// Ordinal 298's third argument. The buffer must be this size and real.
const AVS_BUFFER_BYTES: usize = 0x0800_0000;
/// Shared prefix of both boot layouts. Legacy records are exactly this long.
const LEGACY_RECORD_BYTES: usize = 0xA0;
/// Game id at 0x04. The cap includes the terminating NUL.
const GAME_ID_AT: usize = 0x04;
const GAME_ID_CAP: usize = 0x41;
/// Client version at 0x45. The cap includes the terminating NUL.
const VERSION_AT: usize = 0x45;
const VERSION_CAP: usize = 0x15;
/// Placeholder or launch token at 0x5A. The cap includes the terminating NUL.
const TOKEN_AT: usize = 0x5A;
const TOKEN_CAP: usize = 0x41;
/// Stays clear on both boot modes. It is not part of the token field.
const FLAG_AT: usize = 0x9B;
/// Optional services C string. Empty selects the built-in URL. Never a caller URL.
/// The current caller record is exactly `0x9C + 0x800`, with no alignment padding.
const SERVICES_AT: usize = 0x9C;
const SERVICES_CAP: usize = 0x800;
const SERVICES_RECORD_BYTES: usize = SERVICES_AT + SERVICES_CAP;
const URL_CAP: i32 = 0x800;
const HASH_CAP: i32 = 0x41;
const MODULE_HASH_CAP: i32 = 0x41;
const TOKEN_COPY_BYTES: usize = 256;

const ORDINAL_PROPERTY_CREATE: u16 = 145;
const ORDINAL_PROPERTY_DESTROY: u16 = 146;
const ORDINAL_PROPERTY_QUERY: u16 = 159;
const ORDINAL_PROPERTY_SEARCH: u16 = 162;
const ORDINAL_NODE_CREATE: u16 = 163;
const ORDINAL_NODE_REFER: u16 = 171;
const ORDINAL_AVS_BOOT: u16 = 298;

const NODE_DIRECTORY: i32 = 1;
const NODE_STRING: i32 = 0x0B;
const NODE_U32: i32 = 5;
const NODE_BOOL: i32 = 0x34;
const NODE_ATTRIBUTE: i32 = 0x2E;

const EXPORT_BOOT: &[u8] = b"OA043711000000\0";
const EXPORT_STATE: &[u8] = b"OA043711000001\0";
const EXPORT_SHUTDOWN: &[u8] = b"OA043711000002\0";
const EXPORT_ERROR: &[u8] = b"OA043711000003\0";
/// Secondary error code, the second number of the official `(003-004)` status.
const EXPORT_ERROR_DETAIL: &[u8] = b"OA043711000004\0";
const EXPORT_BOOT_RESULT: &[u8] = b"OA043711000009\0";
const EXPORT_RESOURCE_SEND: &[u8] = b"OA043711000026\0";
const EXPORT_RESOURCE_POLL: &[u8] = b"OA043711000027\0";
const EXPORT_RESOURCE_COPY: &[u8] = b"OA043711000028\0";
const EXPORT_MODULE_SEND: &[u8] = b"OA043711000030\0";
const EXPORT_MODULE_POLL: &[u8] = b"OA043711000031\0";
const EXPORT_MODULE_COPY: &[u8] = b"OA043711000032\0";
const EXPORT_START_SEND: &[u8] = b"OA043711000040\0";
const EXPORT_START_POLL: &[u8] = b"OA043711000041\0";

/// Fixed non-account placeholder. Not a session and not read from the user.
const PLACEHOLDER: &[u8] = b"public-metadata";

const MODE_METADATA: u32 = 2;
const MODE_LAUNCH: u32 = 1;
const WORKER_FINISHED: i32 = 2;
const NORMAL_TERMINAL: i32 = 1;
const NO_ERROR: i32 = 0;
const REQUEST_ENQUEUED: i32 = 0;
const REQUEST_RELEASED: i32 = 2;
const COPY_OK: i32 = 0;
const AVS_BOOTED: i32 = 1;
const PROPERTY_QUERY_OK: i32 = 0;

const BOOT_DEADLINE: Duration = Duration::from_secs(25);
const RESOURCE_DEADLINE: Duration = Duration::from_secs(10);
const STAGE_DEADLINE: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

const AVS_NAME: &str = "avs2-core.dll";
const EAC_NAME: &str = "libeacnet.dll";

/// Mount sources are fixed ASCII, relative to the scratch working directory.
/// An absolute scratch path is not converted: a non-ASCII Windows directory
/// would not fit in the C string AVS reads.
const RAW_SOURCE: &[u8] = b"work\\updater\\dev\\raw\0";
const NVRAM_SOURCE: &[u8] = b"work\\updater\\dev\\nvram\0";

/// Microsoft x64. `updater.exe` passes the fifth node argument at `[rsp+20h]`.
type PropertyCreate = unsafe extern "system" fn(i32, *mut u8, i32) -> *mut u8;
type PropertyDestroy = unsafe extern "system" fn(*mut u8);
type NodeCreate = unsafe extern "system" fn(*mut u8, *mut u8, i32, *const u8, usize) -> *mut u8;
type NodeRefer = unsafe extern "system" fn(*mut u8);
type PropertyQuery = unsafe extern "system" fn(*mut u8) -> i32;
type PropertySearch = unsafe extern "system" fn(*mut u8, *mut u8, *const u8) -> *mut u8;
type AvsBoot = unsafe extern "system" fn(*mut u8, *mut u8, u32, u32, u64, u64) -> i32;
type EacBoot = unsafe extern "system" fn(*mut u8) -> i32;
type EacVoid = unsafe extern "system" fn() -> i32;
type EacCopy = unsafe extern "system" fn(*mut u8, i32, *mut u8, i32) -> i32;
type EacText = unsafe extern "system" fn(*const u8) -> i32;
type EacFill = unsafe extern "system" fn(*mut u8, i32) -> i32;

struct Avs {
    property_create: PropertyCreate,
    property_destroy: PropertyDestroy,
    node_create: NodeCreate,
    node_refer: NodeRefer,
    property_query: PropertyQuery,
    property_search: PropertySearch,
    boot: AvsBoot,
}

struct Eac {
    boot: EacBoot,
    state: EacVoid,
    shutdown: EacVoid,
    error: EacVoid,
    error_detail: EacVoid,
    boot_result: EacVoid,
    resource_send: EacVoid,
    resource_poll: EacVoid,
    resource_copy: EacCopy,
    module_send: EacText,
    module_poll: EacVoid,
    module_copy: EacFill,
    start_send: EacText,
    start_poll: EacVoid,
}

/// The four post-boot calls. The real library and the synthetic test adapter both
/// implement this, so verification tests drive the same state machine.
trait EacStage {
    fn error(&self) -> i32;
    fn boot_result(&self) -> i32;
    fn module_send(&self, version: *const u8) -> i32;
    fn module_poll(&self) -> i32;
    fn module_copy(&self, out: *mut u8, capacity: i32) -> i32;
    fn start_send(&self, token: *const u8) -> i32;
    fn start_poll(&self) -> i32;
}

impl EacStage for Eac {
    fn error(&self) -> i32 {
        // SAFETY: no arguments. Reads the one live boot's error getter.
        unsafe { (self.error)() }
    }

    fn boot_result(&self) -> i32 {
        // SAFETY: no arguments. Reads the one live boot's result getter.
        unsafe { (self.boot_result)() }
    }

    fn module_send(&self, version: *const u8) -> i32 {
        // SAFETY: `version` is a NUL-terminated QBG ProductVersion with no `%`.
        // The export only enqueues getHash; it does not take a path or a hash.
        unsafe { (self.module_send)(version) }
    }

    fn module_poll(&self) -> i32 {
        // SAFETY: no arguments. 2 consumes the one module request.
        unsafe { (self.module_poll)() }
    }

    fn module_copy(&self, out: *mut u8, capacity: i32) -> i32 {
        // SAFETY: `out` is writable for `capacity` bytes. Copy reads the imported
        // response; it does not enqueue another request.
        unsafe { (self.module_copy)(out, capacity) }
    }

    fn start_send(&self, token: *const u8) -> i32 {
        // SAFETY: `token` is a NUL-terminated callback token inside a 256-byte
        // zeroed buffer. This export returns 1 on its paths, including failure.
        unsafe { (self.start_send)(token) }
    }

    fn start_poll(&self) -> i32 {
        // SAFETY: no arguments. 2 consumes the one start request.
        unsafe { (self.start_poll)() }
    }
}

/// Boot the pinned pair in `scratch` and return the public resource receipt.
///
/// `scratch` is the worker's fresh directory and contains only the two DLL
/// copies the caller already placed there. This function creates `raw` and
/// `nvram` under it, pins those paths, and does not mount anything else.
/// `family` selects the discovery profile and its package pins. It is not taken
/// from the receipt the library returns. Discovery does not require a full title:
/// a metadata-only family has no game module or product code yet.
pub(crate) fn discover(family: GameFamily, scratch: &Path) -> Result<Receipt, RuntimeError> {
    let profile = crate::discovery_profile(family)?;
    let mut pinned = pin_scratch(family, scratch)?;
    // DLL pins already deny renaming scratch. Create the relative mount tree
    // before either library is mapped.
    pinned.extend(prepare_mounts(scratch)?);
    // Native threads may retain the mapped libraries on every initialized exit.
    // These handles deliberately remain owned by the worker process until exit.
    let _pins = std::mem::ManuallyDrop::new(pinned);
    let avs_path = scratch.join(AVS_NAME);
    let eac_path = scratch.join(EAC_NAME);
    let (avs_module, eac_module) = load_pair(&avs_path, &eac_path)?;
    // SAFETY: both handles came from LoadLibraryExW on the pinned paths above
    // and are non-null. GetProcAddress only looks up the compile-time names and
    // ordinals listed here. A null result is rejected before any call.
    let avs = unsafe { bind_avs(avs_module) }.map_err(|_| RuntimeError::Runtime)?;
    let eac = unsafe { bind_eac(eac_module) }.map_err(|_| RuntimeError::Runtime)?;

    let mut session = Prepared::open(avs, eac)?;
    let mut record = boot_record(profile, BootMode::Metadata)?;
    // SAFETY: record is the profile's exact boot blob. The first fields are the
    // fixed strings and the services slot, when the layout has one, stays zero.
    // It stays live for this call. A zero return means the worker was created.
    let started = unsafe { (session.eac.boot)(record.as_mut_ptr()) };
    session.shutdown.armed = true;
    if started != 0 {
        return Err(RuntimeError::Runtime);
    }
    wait_until_finished(&session.eac)?;

    // SAFETY: no arguments. Send only enqueues resource-info request type 4.
    let sent = unsafe { (session.eac.resource_send)() };
    if sent != REQUEST_ENQUEUED {
        return Err(RuntimeError::Runtime);
    }
    wait_until_released(&session.eac)?;

    let mut url = [0_u8; URL_CAP as usize];
    let mut hash = [0_u8; HASH_CAP as usize];
    // SAFETY: both buffers are writable for the capacities passed. Copy reads
    // the already-imported response and writes two C strings; it does not
    // enqueue another request. Caps are the updater's 0x800 and 0x41.
    let copied = unsafe { (session.eac.resource_copy)(url.as_mut_ptr(), URL_CAP, hash.as_mut_ptr(), HASH_CAP) };
    if copied != COPY_OK {
        return Err(RuntimeError::Runtime);
    }
    let url = c_string(&url)?;
    let sha256 = c_string(&hash)?;
    // Stop before returning. ManuallyDrop pins and unmanaged native allocations
    // remain alive until process exit, including on an error after initialization.
    session.shutdown.disarm();
    Ok(Receipt { protocol: PROTOCOL, family, url, sha256 })
}

/// One mode-1 module-hash and game-start check. Returns `Ok(())` only when both
/// succeed. Does not retry codes 1 or 2005, and does not create a game process.
/// Shutdown runs before this returns.
pub(crate) fn verify(family: GameFamily, scratch: &Path, input: &crate::VerificationInput) -> Result<(), crate::RuntimeError> {
    launch(family, scratch, input, |session| {
        session.healthy()?;
        session.shutdown.disarm();
        Ok(())
    })
}

/// The same mode-1 pipeline. `hold` runs only after `verify_stages` succeeds and
/// before [`ShutdownGuard`] disarms. A denial or mismatch never calls it.
pub(crate) fn prepare(
    family: GameFamily,
    scratch: &Path,
    input: &crate::VerificationInput,
    hold: impl FnOnce(&mut LiveSession) -> Result<(), RuntimeError>,
) -> Result<(), RuntimeError> {
    launch(family, scratch, input, hold)
}

fn launch(
    family: GameFamily,
    scratch: &Path,
    input: &crate::VerificationInput,
    after: impl FnOnce(&mut LiveSession) -> Result<(), RuntimeError>,
) -> Result<(), RuntimeError> {
    let title = crate::title(family)?;
    let launch = title.launch()?;
    if !crate::installation::valid_game_version(family, &input.product_version) {
        return Err(RuntimeError::Protocol);
    }
    let token = token_bytes(input.token.as_bytes())?;
    crate::diag::stage("pin");
    let mut pinned = pin_scratch(family, scratch)?;
    pinned.extend(prepare_mounts(scratch)?);
    let _pins = std::mem::ManuallyDrop::new(pinned);
    let avs_path = scratch.join(AVS_NAME);
    let eac_path = scratch.join(EAC_NAME);
    crate::diag::stage("load");
    let (avs_module, eac_module) = load_pair(&avs_path, &eac_path)?;
    // SAFETY: both handles came from LoadLibraryExW on the pinned paths above
    // and are non-null. A null export is rejected before any call.
    crate::diag::stage("bind");
    let avs = unsafe { bind_avs(avs_module) }.map_err(|_| RuntimeError::Runtime)?;
    let eac = unsafe { bind_eac(eac_module) }.map_err(|_| RuntimeError::Runtime)?;
    crate::diag::stage("avs-boot");
    let mut session = Prepared::open(avs, eac)?;
    crate::diag::stage("eac-boot");
    let mut record = boot_record(launch.profile, BootMode::Launch)?;
    write_token(record.as_mut_slice(), token.as_slice())?;
    // SAFETY: record is the profile's exact mode-1 blob. The token field is
    // NUL-terminated and was not truncated. The services slot stays zero.
    // A zero return means the worker was created.
    let started = unsafe { (session.eac.boot)(record.as_mut_ptr()) };
    record.zeroize();
    session.shutdown.armed = true;
    if started != 0 {
        crate::diag::detail(&[("ret", started as i64)]);
        return Err(RuntimeError::Runtime);
    }
    crate::diag::stage("boot-wait");
    let booted = wait_until_finished(&session.eac);
    if booted.is_err() {
        note_codes(&session.eac);
    }
    booted?;
    let verified = verify_stages(&session.eac, &input.product_version, token.as_slice(), &input.module_sha256, STAGE_DEADLINE);
    if verified.is_err() {
        note_codes(&session.eac);
    }
    verified?;
    crate::diag::stage("hold");
    let mut live = LiveSession { stage: &session.eac, shutdown: &mut session.shutdown };
    let held = after(&mut live);
    // Verify disarms inside `after`. Prepare disarms here, after the hold, so a
    // dropped or failed callback still shuts the library down exactly once.
    live.shutdown.disarm();
    held
}

/// Add the SDK's state, result, and both error getters to the failure note.
fn note_codes(eac: &Eac) {
    // SAFETY: no-argument getters of the one live boot.
    let (state, result, error, detail) = unsafe { ((eac.state)(), (eac.boot_result)(), (eac.error)(), (eac.error_detail)()) };
    crate::diag::detail(&[("state", state as i64), ("result", result as i64), ("error", error as i64), ("detail", detail as i64)]);
}

/// The still-armed mode-1 session. The callback may read its boot result. It
/// cannot enqueue another request or create a process.
pub(crate) struct LiveSession<'a> {
    stage: &'a Eac,
    shutdown: &'a mut ShutdownGuard,
}

impl LiveSession<'_> {
    /// Normal terminal and error 0. Anything else means the session is no longer
    /// a launch permission, even if this process is still running.
    pub(crate) fn healthy(&self) -> Result<(), RuntimeError> {
        if self.stage.boot_result() != NORMAL_TERMINAL || self.stage.error() != NO_ERROR { Err(RuntimeError::Runtime) } else { Ok(()) }
    }
}

/// Monitor the verified SDK and acknowledge exactly one final creation check.
/// A detached reader cannot delay failure or shutdown while waiting for stdin.
pub(crate) fn hold_until_finish(
    session: &LiveSession<'_>,
    stdin: impl std::io::Read + Send + 'static,
    limit: Duration,
) -> Result<(), RuntimeError> {
    hold_controls(
        stdin,
        limit,
        || session.healthy(),
        || {
            use std::io::Write;
            let mut stdout = std::io::stdout().lock();
            stdout.write_all(&[crate::ALIVE]).map_err(|_| RuntimeError::Protocol)?;
            stdout.flush().map_err(|_| RuntimeError::Protocol)
        },
    )
}

fn hold_controls(
    mut stdin: impl std::io::Read + Send + 'static,
    limit: Duration,
    mut healthy: impl FnMut() -> Result<(), RuntimeError>,
    mut acknowledge: impl FnMut() -> Result<(), RuntimeError>,
) -> Result<(), RuntimeError> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        loop {
            let mut byte = [0_u8; 1];
            let value = match stdin.read(&mut byte) {
                Ok(0) => Ok(None),
                Ok(_) => Ok(Some(byte[0])),
                Err(_) => Err(()),
            };
            let terminal = !matches!(value, Ok(Some(crate::CHECK)));
            if sender.send(value).is_err() || terminal {
                break;
            }
        }
    });
    let deadline = Instant::now() + limit;
    let mut checked = false;
    loop {
        if Instant::now() >= deadline {
            return Err(RuntimeError::Timeout);
        }
        healthy()?;
        match receiver.recv_timeout(POLL_INTERVAL) {
            Ok(Ok(Some(crate::FINISH))) | Ok(Ok(None)) => return healthy(),
            Ok(Ok(Some(crate::CHECK))) if !checked => {
                healthy()?;
                acknowledge()?;
                checked = true;
            }
            Ok(_) => return Err(RuntimeError::Protocol),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(RuntimeError::Protocol);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

/// Shared AVS boot. The 128 MiB buffer is retained for process lifetime and is
/// not freed after property destruction or shutdown.
struct Prepared {
    eac: Eac,
    shutdown: ShutdownGuard,
}

impl Prepared {
    fn open(avs: Avs, eac: Eac) -> Result<Self, RuntimeError> {
        let shutdown = ShutdownGuard { shutdown: Some(eac.shutdown), armed: false };
        let booted = {
            let (property, config, config_bytes) = build_config(&avs)?;
            // SAFETY: size is the fixed 128 MiB AVS contract. The pointer is retained
            // for process lifetime and is not freed after boot, because AVS threads can
            // still reference it after property destruction and shutdown.
            let buffer = unsafe { VirtualAlloc(std::ptr::null(), AVS_BUFFER_BYTES, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) } as *mut u8;
            if buffer.is_null() {
                // SAFETY: property is the non-null node returned by property_create.
                unsafe { (avs.property_destroy)(property) };
                return Err(RuntimeError::Runtime);
            }

            // SAFETY: config is the non-null `/config` node, buffer is the 128 MiB
            // region just allocated, and the remaining arguments are the updater's
            // zeros. `config_bytes` keeps the descriptor alive through this boot and
            // the following destroy. Mount sources are static.
            let booted = unsafe { (avs.boot)(config, buffer, AVS_BUFFER_BYTES as u32, 0, 0, 0) };
            // SAFETY: destroy does not release `buffer`. No later call reads the tree.
            unsafe { (avs.property_destroy)(property) };
            let _keep_config_bytes = config_bytes;
            booted
        };
        if booted != AVS_BOOTED {
            let _ = shutdown;
            return Err(RuntimeError::Runtime);
        }
        Ok(Self { eac, shutdown })
    }
}

fn verify_stages(eac: &impl EacStage, version: &str, token: &[u8], expected: &[u8; 32], timeout: Duration) -> Result<(), RuntimeError> {
    crate::diag::stage("boot-result");
    if eac.boot_result() != NORMAL_TERMINAL || eac.error() != NO_ERROR {
        return Err(RuntimeError::Runtime);
    }
    // Grammar is at most 20 bytes (`QBG:J:A:A:` plus 10 digits). The extra byte
    // stays NUL so the export always receives a C string, never a truncated one.
    let mut version_bytes = [0_u8; 21];
    let text = version.as_bytes();
    if text.len() >= version_bytes.len() || text.contains(&b'%') || text.contains(&0) {
        return Err(RuntimeError::Protocol);
    }
    version_bytes[..text.len()].copy_from_slice(text);
    crate::diag::stage("module-send");
    let sent = eac.module_send(version_bytes.as_ptr());
    if sent != REQUEST_ENQUEUED {
        crate::diag::detail(&[("ret", sent as i64)]);
        return Err(RuntimeError::Runtime);
    }
    crate::diag::stage("module-poll");
    wait_stage(eac, timeout, |stage| stage.module_poll())?;
    crate::diag::stage("module-copy");
    let mut digest = [0_u8; MODULE_HASH_CAP as usize];
    if eac.module_copy(digest.as_mut_ptr(), MODULE_HASH_CAP) != COPY_OK {
        return Err(RuntimeError::Runtime);
    }
    crate::diag::stage("module-compare");
    let returned = module_digest(&digest)?;
    if &returned != expected {
        return Err(RuntimeError::ModuleMismatch);
    }
    crate::diag::stage("start-ready");
    if eac.boot_result() != NORMAL_TERMINAL || eac.error() != NO_ERROR {
        return Err(RuntimeError::Runtime);
    }
    let mut copy = Zeroizing::new([0_u8; TOKEN_COPY_BYTES]);
    copy[..token.len()].copy_from_slice(token);
    // Nonzero means the official caller treats the request as sent. This pinned
    // export returns 1 on its paths, including failure, so a zero is not success
    // and error 0 is still required before the poll.
    crate::diag::stage("start-send");
    if eac.start_send(copy.as_ptr()) == 0 || eac.error() != NO_ERROR {
        return Err(RuntimeError::Runtime);
    }
    crate::diag::stage("start-poll");
    match wait_stage(eac, timeout, |stage| stage.start_poll()) {
        Ok(()) => {}
        Err(RuntimeError::Runtime) => return Err(RuntimeError::VerificationDenied),
        Err(error) => return Err(error),
    }
    crate::diag::stage("start-result");
    if eac.boot_result() != NORMAL_TERMINAL {
        return Err(RuntimeError::Runtime);
    }
    Ok(())
}

/// Calls shutdown once if boot was entered and the success path did not already
/// shut down. A hang here is bounded by the parent killing the worker.
struct ShutdownGuard {
    shutdown: Option<EacVoid>,
    armed: bool,
}

impl ShutdownGuard {
    fn disarm(&mut self) {
        if self.armed
            && let Some(shutdown) = self.shutdown.take()
        {
            // SAFETY: paired with the one boot call on this thread. The parent
            // bounds shutdown if initialization did not complete normally.
            unsafe { shutdown() };
        }
        self.armed = false;
    }
}

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        self.disarm();
    }
}

fn wait_until_finished(eac: &Eac) -> Result<(), RuntimeError> {
    let deadline = Instant::now() + BOOT_DEADLINE;
    loop {
        // SAFETY: the three exports take no arguments and only read the boot
        // state machine. None of them consumes the later resource request.
        let state = unsafe { (eac.state)() };
        let error = unsafe { (eac.error)() };
        let boot_result = unsafe { (eac.boot_result)() };
        if state == WORKER_FINISHED {
            // 1 is the normal terminal. 2 is a boot error or heartbeat exit.
            if error != NO_ERROR || boot_result != NORMAL_TERMINAL {
                return Err(RuntimeError::Runtime);
            }
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(RuntimeError::Timeout);
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn wait_until_released(eac: &Eac) -> Result<(), RuntimeError> {
    wait_for_resource(
        RESOURCE_DEADLINE,
        POLL_INTERVAL,
        // SAFETY: these no-argument exports belong to the one live boot.
        || unsafe { (eac.resource_poll)() },
        || unsafe { (eac.error)() },
    )
}

fn wait_stage(eac: &impl EacStage, timeout: Duration, poll: impl Fn(&dyn EacStage) -> i32) -> Result<(), RuntimeError> {
    wait_for_resource(timeout, POLL_INTERVAL, || poll(eac), || eac.error())
}

fn wait_for_resource(
    timeout: Duration,
    interval: Duration,
    mut poll: impl FnMut() -> i32,
    mut error: impl FnMut() -> i32,
) -> Result<(), RuntimeError> {
    let deadline = Instant::now() + timeout;
    loop {
        match poll() {
            // The request has been released, on either outcome. Do not poll again.
            REQUEST_RELEASED => {
                return if error() == NO_ERROR { Ok(()) } else { Err(RuntimeError::Runtime) };
            }
            0 => {}
            _ => return Err(RuntimeError::Runtime),
        }
        if Instant::now() >= deadline {
            return Err(RuntimeError::Timeout);
        }
        std::thread::sleep(interval);
    }
}

fn prepare_mounts(scratch: &Path) -> Result<Vec<File>, RuntimeError> {
    let dev = scratch.join("work").join("updater").join("dev");
    let raw = dev.join("raw");
    let log = raw.join("log");
    let nvram = dev.join("nvram");
    let mut pins = Vec::new();
    for directory in [scratch.join("work"), scratch.join("work").join("updater"), dev, raw, log, nvram] {
        fs::create_dir(&directory).map_err(|_| RuntimeError::Worker)?;
        pins.push(pin_directory(&directory)?);
    }
    Ok(pins)
}

fn build_config(avs: &Avs) -> Result<(*mut u8, *mut u8, ConfigBytes), RuntimeError> {
    let mut bytes = ConfigBytes { descriptor: vec![0_u8; PROPERTY_DESCRIPTOR_BYTES] };

    // SAFETY: descriptor is a zeroed 0xD18 buffer owned by `bytes` and live
    // until property_destroy. 0x17 is the updater's property-create type.
    let property = unsafe { (avs.property_create)(0x17, bytes.descriptor.as_mut_ptr(), PROPERTY_DESCRIPTOR_BYTES as i32) };
    if property.is_null() {
        return Err(RuntimeError::Runtime);
    }
    let built = build_tree(avs, property);
    if built.is_err() {
        // SAFETY: create returned this non-null property. Destroying it on a
        // failed tree drops every node created under it.
        unsafe { (avs.property_destroy)(property) };
        return Err(RuntimeError::Runtime);
    }
    // SAFETY: property is still the tree just built. Query rejects a non-zero
    // result. Search asks only for the fixed `/config` node.
    let query = unsafe { (avs.property_query)(property) };
    let config = unsafe { (avs.property_search)(property, std::ptr::null_mut(), c"/config".as_ptr().cast()) };
    if query != PROPERTY_QUERY_OK || config.is_null() {
        unsafe { (avs.property_destroy)(property) };
        return Err(RuntimeError::Runtime);
    }
    Ok((property, config, bytes))
}

/// Property descriptor. AVS reads it through boot, so it stays allocated until
/// property_destroy returns. Mount sources are static ASCII and outlive boot.
struct ConfigBytes {
    descriptor: Vec<u8>,
}

fn build_tree(avs: &Avs, property: *mut u8) -> Result<(), ()> {
    string(avs, property, std::ptr::null_mut(), NODE_STRING, b"/config/fs/root/device\0", b".\0")?;
    string(avs, property, std::ptr::null_mut(), NODE_STRING, b"/config/fs/root/option\0", b"posix=0\0")?;
    let table = node(avs, property, std::ptr::null_mut(), NODE_DIRECTORY, b"/config/fs/mounttable\0", 0)?;
    let fs = node(avs, std::ptr::null_mut(), table, NODE_DIRECTORY, b"vfs\0", 0)?;
    attribute(avs, fs, b"name@\0", b"boot\0")?;
    attribute(avs, fs, b"fstype@\0", b"fs\0")?;
    attribute(avs, fs, b"src@\0", RAW_SOURCE)?;
    attribute(avs, fs, b"dst@\0", b"/dev/raw\0")?;
    attribute(avs, fs, b"opt@\0", b"vf=0,posix=0\0")?;
    let nv = node(avs, std::ptr::null_mut(), table, NODE_DIRECTORY, b"vfs\0", 0)?;
    attribute(avs, nv, b"name@\0", b"boot\0")?;
    attribute(avs, nv, b"fstype@\0", b"nvram\0")?;
    attribute(avs, nv, b"src@\0", NVRAM_SOURCE)?;
    attribute(avs, nv, b"dst@\0", b"/dev/nvram\0")?;
    attribute(avs, nv, b"opt@\0", b"sz_block=512,nr_block=32,nr_mirror=4\0")?;
    node(avs, property, std::ptr::null_mut(), NODE_BOOL, b"/config/sntp/ea_on\0", 0)?;
    node(avs, property, std::ptr::null_mut(), NODE_BOOL, b"/config/net/enable_raw\0", 0)?;
    node(avs, property, std::ptr::null_mut(), NODE_U32, b"/config/fs/nr_mountpoint\0", 0x100)?;
    node(avs, property, std::ptr::null_mut(), NODE_U32, b"/config/thread/nr_mutex\0", 0x100)?;
    node(avs, property, std::ptr::null_mut(), NODE_U32, b"/config/fs/nr_filedesc\0", 0x100)?;
    string(avs, property, std::ptr::null_mut(), NODE_STRING, b"/config/log/level\0", b"DISABLE\0")?;
    node(avs, property, std::ptr::null_mut(), NODE_BOOL, b"/config/log/enable_file\0", 0)?;
    Ok(())
}

fn string(avs: &Avs, property: *mut u8, parent: *mut u8, kind: i32, name: &'static [u8], value: &[u8]) -> Result<(), ()> {
    let created = node(avs, property, parent, kind, name, value.as_ptr() as usize)?;
    if kind == NODE_STRING || kind == NODE_ATTRIBUTE {
        // SAFETY: created is the non-null node just returned for a string or
        // attribute. Refer is the updater's one-argument call on those nodes.
        unsafe { (avs.node_refer)(created) };
    }
    Ok(())
}

fn attribute(avs: &Avs, parent: *mut u8, name: &'static [u8], value: &[u8]) -> Result<(), ()> {
    string(avs, std::ptr::null_mut(), parent, NODE_ATTRIBUTE, name, value)
}

fn node(avs: &Avs, property: *mut u8, parent: *mut u8, kind: i32, name: &'static [u8], value: usize) -> Result<*mut u8, ()> {
    // SAFETY: a root node gets the property and a null parent; a child gets a
    // null property and the parent node. `name` is a static NUL-terminated
    // ASCII literal. `value` is either a 32-bit immediate or a pointer to a
    // NUL-terminated string that stays alive until property_destroy.
    let created = unsafe { (avs.node_create)(property, parent, kind, name.as_ptr(), value) };
    if created.is_null() { Err(()) } else { Ok(created) }
}

enum BootMode {
    /// Public resource discovery. Fixed placeholder, updater client version.
    Metadata,
    /// Normal launcher sequence. Fresh callback token, launcher client version.
    Launch,
}

/// Mode, game id, and client version come from the discovery profile. A field
/// that does not fit its fixed slot, including its terminating NUL, is rejected
/// before any byte is copied. Launch leaves the token slot empty for `write_token`.
/// The version is that profile's own string. It is not rewritten to a game PE code.
/// A services layout adds one zeroed C string. Empty selects the built-in URL.
/// This function takes no services argument and never writes a caller URL there.
fn boot_record(profile: &crate::DiscoveryProfile, mode: BootMode) -> Result<Zeroizing<Vec<u8>>, RuntimeError> {
    let length = profile.record_bytes();
    let expected = match profile.layout {
        crate::title::BootLayout::Legacy => LEGACY_RECORD_BYTES,
        crate::title::BootLayout::Services => SERVICES_RECORD_BYTES,
    };
    if length != expected {
        return Err(RuntimeError::Runtime);
    }
    let mut record = Zeroizing::new(vec![0_u8; length]);
    let (mode_value, version, token) = match mode {
        BootMode::Metadata => (MODE_METADATA, profile.updater_version, PLACEHOLDER),
        BootMode::Launch => (MODE_LAUNCH, profile.launcher_version, b"".as_slice()),
    };
    let game_id = profile.game_id.as_bytes();
    let version = version.as_bytes();
    if game_id.len() >= GAME_ID_CAP
        || version.len() >= VERSION_CAP
        || token.len() >= TOKEN_CAP
        || game_id.contains(&0)
        || version.contains(&0)
        || token.contains(&0)
    {
        return Err(RuntimeError::Runtime);
    }
    record[..4].copy_from_slice(&mode_value.to_le_bytes());
    record[GAME_ID_AT..GAME_ID_AT + game_id.len()].copy_from_slice(game_id);
    record[VERSION_AT..VERSION_AT + version.len()].copy_from_slice(version);
    if !token.is_empty() {
        record[TOKEN_AT..TOKEN_AT + token.len()].copy_from_slice(token);
    }
    record[FLAG_AT] = 0;
    Ok(record)
}

/// Same alphabet and 1..=64 bound as the existing callback validator. No extra
/// narrowing, and no format, NUL, control, or space bytes.
fn token_bytes(token: &[u8]) -> Result<Zeroizing<Vec<u8>>, RuntimeError> {
    if token.is_empty()
        || token.len() > 64
        || !token.iter().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'+' | b'/' | b'='))
    {
        return Err(RuntimeError::Protocol);
    }
    Ok(Zeroizing::new(token.to_vec()))
}

fn write_token(record: &mut [u8], token: &[u8]) -> Result<(), RuntimeError> {
    if record.len() < TOKEN_AT + TOKEN_CAP || token.len() >= TOKEN_CAP || token.contains(&0) {
        return Err(RuntimeError::Protocol);
    }
    record[TOKEN_AT..TOKEN_AT + token.len()].copy_from_slice(token);
    record[TOKEN_AT + token.len()] = 0;
    Ok(())
}

fn module_digest(bytes: &[u8]) -> Result<[u8; 32], RuntimeError> {
    let text = CStr::from_bytes_until_nul(bytes).map_err(|_| RuntimeError::ModuleMismatch)?;
    let text = text.to_bytes();
    if text.len() != 64 || !text.iter().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte)) {
        return Err(RuntimeError::ModuleMismatch);
    }
    let mut digest = [0_u8; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(std::str::from_utf8(&text[index * 2..index * 2 + 2]).map_err(|_| RuntimeError::ModuleMismatch)?, 16)
            .map_err(|_| RuntimeError::ModuleMismatch)?;
    }
    Ok(digest)
}

fn c_string(bytes: &[u8]) -> Result<String, RuntimeError> {
    let text = CStr::from_bytes_until_nul(bytes).map_err(|_| RuntimeError::Protocol)?;
    text.to_str().map(|value| value.to_string()).map_err(|_| RuntimeError::Protocol)
}

fn wide_path(path: &Path) -> Result<Vec<u16>, RuntimeError> {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(RuntimeError::Worker);
    }
    wide.push(0);
    Ok(wide)
}

/// Re-hash the two DLL copies, reject reparse points, and hold handles that
/// deny delete/rename for the DLLs and deny delete for every scratch ancestor.
fn pin_scratch(family: GameFamily, scratch: &Path) -> Result<Vec<File>, RuntimeError> {
    let mut held = pin_root(scratch)?;
    // A discovery profile is enough to pin the DLL pair. Launch still requires a
    // full title before it reaches this. The pins belong to this family and are
    // not interchangeable with another title's pair.
    crate::discovery_profile(family)?;
    let package = package_for(family);
    held.push(verify_copy(&scratch.join(AVS_NAME), package.avs2_core_sha256, package.avs2_core_bytes)?);
    held.push(verify_copy(&scratch.join(EAC_NAME), package.libeacnet_sha256, package.libeacnet_bytes)?);
    Ok(held)
}

// Hold the directory chain before the caller creates either DLL copy.
pub(crate) fn pin_root(scratch: &Path) -> Result<Vec<File>, RuntimeError> {
    if !scratch.is_absolute() || scratch.components().any(|part| matches!(part, Component::ParentDir)) {
        return Err(RuntimeError::Worker);
    }
    let mut ancestors = Vec::new();
    let mut cursor = scratch.to_path_buf();
    loop {
        ancestors.push(cursor.clone());
        if !cursor.pop() {
            break;
        }
    }
    let mut held = Vec::new();
    for directory in ancestors.into_iter().rev() {
        held.push(pin_directory(&directory)?);
    }
    Ok(held)
}

fn verify_copy(path: &Path, expected_sha256: &str, expected_bytes: u64) -> Result<File, RuntimeError> {
    let pin = pin_file(path)?;
    // Hash through a second read handle opened only after the no-delete pin
    // exists, so the pin's cursor stays at offset 0 for the loader.
    let mut reader = pin_file(path)?;
    let info = file_information(&pin)?;
    let reader_info = file_information(&reader)?;
    if info.nNumberOfLinks != 1
        || info.dwVolumeSerialNumber != reader_info.dwVolumeSerialNumber
        || info.nFileIndexHigh != reader_info.nFileIndexHigh
        || info.nFileIndexLow != reader_info.nFileIndexLow
    {
        return Err(RuntimeError::Runtime);
    }
    let size = ((info.nFileSizeHigh as u64) << 32) | info.nFileSizeLow as u64;
    if size != expected_bytes {
        return Err(RuntimeError::Runtime);
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let read = reader.read(&mut buffer).map_err(|_| RuntimeError::Runtime)?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > expected_bytes {
            return Err(RuntimeError::Runtime);
        }
        hasher.update(&buffer[..read]);
    }
    if total != expected_bytes {
        return Err(RuntimeError::Runtime);
    }
    let digest = hex_encode(&hasher.finalize());
    if digest != expected_sha256 {
        return Err(RuntimeError::Runtime);
    }
    Ok(pin)
}

pub(crate) fn pin_file(path: &Path) -> Result<File, RuntimeError> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| RuntimeError::Runtime)?;
    reject_reparse(&file)?;
    Ok(file)
}

fn pin_directory(path: &Path) -> Result<File, RuntimeError> {
    let directory = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .map_err(|_| RuntimeError::Worker)?;
    reject_reparse(&directory)?;
    Ok(directory)
}

fn reject_reparse(file: &File) -> Result<(), RuntimeError> {
    let info = file_information(file)?;
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(RuntimeError::Worker);
    }
    Ok(())
}

pub(crate) fn file_information(file: &File) -> Result<BY_HANDLE_FILE_INFORMATION, RuntimeError> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is an open file owned by `file`. The information
    // record is writable and its size matches the ABI struct.
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut info) };
    if ok == 0 { Err(RuntimeError::Runtime) } else { Ok(info) }
}

fn load_pair(avs: &Path, eac: &Path) -> Result<(isize, isize), RuntimeError> {
    // AVS first: libeacnet imports it, and the load-directory flag must not
    // search any other application directory for that import. A failed second
    // load leaves the first module mapped until process exit; unloading it
    // while its file pin is held is not required for the failure return.
    let avs_module = load_pinned(avs)?;
    let eac_module = load_pinned(eac)?;
    Ok((avs_module, eac_module))
}

fn load_pinned(path: &Path) -> Result<isize, RuntimeError> {
    let wide = wide_path(path)?;
    // SAFETY: `wide` is a NUL-terminated path to the pinned DLL. The flags
    // allow that DLL's directory and System32 only. The reserved handle is null.
    let module =
        unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32) };
    if module.is_null() { Err(RuntimeError::Runtime) } else { Ok(module as isize) }
}

/// SAFETY: `module` is a live HMODULE. Each ordinal is an `avs2-core` export
/// number from the updater call sites, made into the `MAKEINTRESOURCE` pointer
/// GetProcAddress accepts. The transmute keeps the exact system-ABI signature.
unsafe fn bind_avs(module: isize) -> Result<Avs, ()> {
    Ok(Avs {
        property_create: unsafe { ordinal(module, ORDINAL_PROPERTY_CREATE) }?,
        property_destroy: unsafe { ordinal(module, ORDINAL_PROPERTY_DESTROY) }?,
        node_create: unsafe { ordinal(module, ORDINAL_NODE_CREATE) }?,
        node_refer: unsafe { ordinal(module, ORDINAL_NODE_REFER) }?,
        property_query: unsafe { ordinal(module, ORDINAL_PROPERTY_QUERY) }?,
        property_search: unsafe { ordinal(module, ORDINAL_PROPERTY_SEARCH) }?,
        boot: unsafe { ordinal(module, ORDINAL_AVS_BOOT) }?,
    })
}

/// SAFETY: `module` is a live HMODULE. Every name is a compile-time NUL-terminated
/// export. The transmute keeps the exact system-ABI signature of that export.
unsafe fn bind_eac(module: isize) -> Result<Eac, ()> {
    Ok(Eac {
        boot: unsafe { export(module, EXPORT_BOOT) }?,
        state: unsafe { export(module, EXPORT_STATE) }?,
        shutdown: unsafe { export(module, EXPORT_SHUTDOWN) }?,
        error: unsafe { export(module, EXPORT_ERROR) }?,
        error_detail: unsafe { export(module, EXPORT_ERROR_DETAIL) }?,
        boot_result: unsafe { export(module, EXPORT_BOOT_RESULT) }?,
        resource_send: unsafe { export(module, EXPORT_RESOURCE_SEND) }?,
        resource_poll: unsafe { export(module, EXPORT_RESOURCE_POLL) }?,
        resource_copy: unsafe { export(module, EXPORT_RESOURCE_COPY) }?,
        module_send: unsafe { export(module, EXPORT_MODULE_SEND) }?,
        module_poll: unsafe { export(module, EXPORT_MODULE_POLL) }?,
        module_copy: unsafe { export(module, EXPORT_MODULE_COPY) }?,
        start_send: unsafe { export(module, EXPORT_START_SEND) }?,
        start_poll: unsafe { export(module, EXPORT_START_POLL) }?,
    })
}

/// SAFETY: caller guarantees `module` is loaded and `ordinal` is one of the
/// seven AVS exports above. The returned function is not called here.
unsafe fn ordinal<T>(module: isize, ordinal: u16) -> Result<T, ()> {
    // SAFETY: ordinals 1..=65535 are passed by pointer value, not as a string.
    let found = unsafe { GetProcAddress(module as _, ordinal as usize as *const u8) };
    match found {
        Some(function) => {
            // SAFETY: T is the system-ABI type declared for this ordinal.
            Ok(unsafe { std::mem::transmute_copy(&function) })
        }
        None => Err(()),
    }
}

/// SAFETY: caller guarantees `module` is loaded and `name` is the NUL-terminated
/// name of one export above. The returned function is not called here.
unsafe fn export<T>(module: isize, name: &'static [u8]) -> Result<T, ()> {
    // SAFETY: `name` includes its terminating NUL and contains no interior NUL.
    let found = unsafe { GetProcAddress(module as _, name.as_ptr()) };
    match found {
        Some(function) => Ok(unsafe { std::mem::transmute_copy(&function) }),
        None => Err(()),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn live_session_controls_acknowledge_only_one_healthy_creation_check() {
        let count = Cell::new(0);
        assert_eq!(
            hold_controls(
                std::io::Cursor::new(vec![crate::CHECK, crate::FINISH]),
                Duration::from_secs(1),
                || Ok(()),
                || {
                    count.set(count.get() + 1);
                    Ok(())
                }
            ),
            Ok(())
        );
        assert_eq!(count.get(), 1);
        let count = Cell::new(0);
        assert_eq!(
            hold_controls(
                std::io::Cursor::new(vec![crate::CHECK]),
                Duration::from_secs(1),
                || Err(RuntimeError::Runtime),
                || {
                    count.set(count.get() + 1);
                    Ok(())
                }
            ),
            Err(RuntimeError::Runtime)
        );
        assert_eq!(count.get(), 0);
        let reads = Cell::new(0);
        assert_eq!(
            hold_controls(
                std::io::Cursor::new(vec![crate::CHECK]),
                Duration::from_secs(1),
                || {
                    reads.set(reads.get() + 1);
                    if reads.get() >= 2 { Err(RuntimeError::Runtime) } else { Ok(()) }
                },
                || {
                    count.set(count.get() + 1);
                    Ok(())
                }
            ),
            Err(RuntimeError::Runtime)
        );
        assert_eq!(count.get(), 0);
        assert_eq!(
            hold_controls(
                std::io::Cursor::new(vec![crate::CHECK]),
                Duration::ZERO,
                || Ok(()),
                || {
                    count.set(count.get() + 1);
                    Ok(())
                }
            ),
            Err(RuntimeError::Timeout)
        );
        assert_eq!(count.get(), 0);
    }

    #[test]
    fn unknown_or_repeated_controls_do_not_become_another_permission() {
        for input in [vec![99], vec![crate::CHECK, crate::CHECK]] {
            let count = Cell::new(0);
            assert_eq!(
                hold_controls(
                    std::io::Cursor::new(input),
                    Duration::from_secs(1),
                    || Ok(()),
                    || {
                        count.set(count.get() + 1);
                        Ok(())
                    }
                ),
                Err(RuntimeError::Protocol)
            );
            assert!(count.get() <= 1);
        }
    }

    #[test]
    fn pinned_scratch_and_files_refuse_replacement_until_released() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("synthetic.dll");
        fs::write(&path, b"synthetic").unwrap();
        let directories = pin_root(scratch.path()).unwrap();
        let digest = hex_encode(&Sha256::digest(b"synthetic"));
        let file = verify_copy(&path, &digest, 9).unwrap();
        assert!(fs::rename(scratch.path(), scratch.path().with_extension("moved")).is_err());
        assert!(OpenOptions::new().write(true).truncate(true).open(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"synthetic");
        drop(file);
        drop(directories);
        fs::write(&path, b"changed").unwrap();
    }

    #[test]
    fn copy_validation_rejects_wrong_size_digest_and_hardlinks() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("synthetic.dll");
        fs::write(&path, b"synthetic").unwrap();
        let digest = hex_encode(&Sha256::digest(b"synthetic"));
        assert!(verify_copy(&path, &digest, 10).is_err());
        assert!(verify_copy(&path, &"00".repeat(32), 9).is_err());
        fs::hard_link(&path, scratch.path().join("alias.dll")).unwrap();
        assert!(verify_copy(&path, &digest, 9).is_err());
    }

    /// Every family `discovery_profile` admits. A full title is a separate gate:
    /// a metadata-only family is present here and still cannot launch.
    fn admitted() -> Vec<(GameFamily, &'static crate::DiscoveryProfile)> {
        GameFamily::ALL.into_iter().filter_map(|family| crate::discovery_profile(family).ok().map(|profile| (family, profile))).collect()
    }

    /// Both boot modes use the same slots. The version is that mode's own string,
    /// copied whole. It is not rewritten so its prefix matches the game PE code.
    fn assert_boot_wire(profile: &crate::DiscoveryProfile, mode: BootMode) {
        let game_id = profile.game_id.as_bytes();
        let (mode_value, version) = match mode {
            BootMode::Metadata => (MODE_METADATA, profile.updater_version.as_bytes()),
            BootMode::Launch => (MODE_LAUNCH, profile.launcher_version.as_bytes()),
        };
        assert!(!game_id.is_empty() && game_id.len() < GAME_ID_CAP && !game_id.contains(&0));
        assert!(!version.is_empty() && version.len() < VERSION_CAP && !version.contains(&0));
        assert_ne!(profile.updater_version, profile.launcher_version);
        assert!(profile.launcher_version.len() < VERSION_CAP);
        assert!(profile.updater_version.len() < VERSION_CAP);
        let record = boot_record(profile, mode).unwrap();
        assert_eq!(record.len(), profile.record_bytes());
        assert_eq!(u32::from_le_bytes(record[..4].try_into().unwrap()), mode_value);
        assert_eq!(&record[GAME_ID_AT..GAME_ID_AT + game_id.len()], game_id);
        assert_eq!(record[GAME_ID_AT + game_id.len()], 0);
        assert!(record[GAME_ID_AT + game_id.len()..VERSION_AT].iter().all(|byte| *byte == 0));
        assert_eq!(&record[VERSION_AT..VERSION_AT + version.len()], version);
        assert_eq!(record[VERSION_AT + version.len()], 0);
        assert!(record[VERSION_AT + version.len()..TOKEN_AT].iter().all(|byte| *byte == 0));
        match mode_value {
            MODE_METADATA => {
                assert_ne!(PLACEHOLDER, b"");
                assert!(PLACEHOLDER.len() < TOKEN_CAP);
                assert_eq!(&record[TOKEN_AT..TOKEN_AT + PLACEHOLDER.len()], PLACEHOLDER);
                assert_eq!(record[TOKEN_AT + PLACEHOLDER.len()], 0);
                assert!(record[TOKEN_AT + PLACEHOLDER.len()..FLAG_AT].iter().all(|byte| *byte == 0));
            }
            MODE_LAUNCH => {
                assert!(record[TOKEN_AT..FLAG_AT].iter().all(|byte| *byte == 0));
            }
            _ => panic!("boot mode is only metadata or launch"),
        }
        assert_eq!(record[FLAG_AT], 0);
        assert!(record[FLAG_AT + 1..].iter().all(|byte| *byte == 0));
        match profile.layout {
            crate::title::BootLayout::Legacy => assert_eq!(record.len(), LEGACY_RECORD_BYTES),
            crate::title::BootLayout::Services => {
                assert_eq!(record.len(), SERVICES_RECORD_BYTES);
                assert!(record[SERVICES_AT..].iter().all(|byte| *byte == 0));
            }
        }
    }

    #[test]
    fn boot_record_matches_the_mode2_layout() {
        let admitted = admitted();
        assert!(!admitted.is_empty());
        for (_, profile) in admitted {
            assert_boot_wire(profile, BootMode::Metadata);
        }
    }

    #[test]
    fn descriptor_slots_belong_to_the_admitted_title() {
        assert_eq!(GAME_ID_AT + GAME_ID_CAP, VERSION_AT);
        assert_eq!(VERSION_AT + VERSION_CAP, TOKEN_AT);
        assert_eq!(TOKEN_AT + TOKEN_CAP, FLAG_AT);
        assert_eq!(FLAG_AT + 1, SERVICES_AT);
        const { assert!(FLAG_AT < LEGACY_RECORD_BYTES) };
        const { assert!(SERVICES_AT + SERVICES_CAP == SERVICES_RECORD_BYTES) };
        let admitted = admitted();
        assert!(!admitted.is_empty());
        for (_, profile) in admitted {
            assert_boot_wire(profile, BootMode::Metadata);
            assert_boot_wire(profile, BootMode::Launch);
        }
        for layout in [crate::title::BootLayout::Legacy, crate::title::BootLayout::Services] {
            let packed = boot_record(&profile_wire(&PACKED, layout), BootMode::Metadata).unwrap();
            assert_eq!(packed.len(), layout_bytes(layout));
            assert_eq!(packed[GAME_ID_AT + 64], 0);
            assert_eq!(packed[VERSION_AT], b'Q');
            assert_eq!(packed[VERSION_AT + 20], 0);
            assert_eq!(packed[TOKEN_AT], PLACEHOLDER[0]);
            assert!(packed[FLAG_AT + 1..].iter().all(|byte| *byte == 0));
            for wire in [&WIDE_ID, &WIDE_VERSION, &INTERIOR_NUL] {
                let profile = profile_wire(wire, layout);
                assert_eq!(boot_record(&profile, BootMode::Metadata).map(|_| ()), Err(RuntimeError::Runtime));
                assert_eq!(boot_record(&profile, BootMode::Launch).map(|_| ()), Err(RuntimeError::Runtime));
            }
        }
        assert!(write_token(&mut [0_u8; TOKEN_AT + TOKEN_CAP - 1], b"ab").is_err());
    }

    fn layout_bytes(layout: crate::title::BootLayout) -> usize {
        match layout {
            crate::title::BootLayout::Legacy => LEGACY_RECORD_BYTES,
            crate::title::BootLayout::Services => SERVICES_RECORD_BYTES,
        }
    }

    struct Wire {
        game_id: &'static str,
        updater_version: &'static str,
        launcher_version: &'static str,
    }

    const PACKED: Wire = Wire {
        game_id: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        updater_version: "QBG:J:C:A:2021122100",
        launcher_version: "QBG:J:B:A:2021122100",
    };
    const WIDE_ID: Wire = Wire {
        game_id: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdefX",
        updater_version: "QBG:J:C:A:2021122100",
        launcher_version: "QBG:J:B:A:2021122100",
    };
    const WIDE_VERSION: Wire =
        Wire { game_id: "bomber-girl", updater_version: "QBG:J:C:A:20211221000", launcher_version: "QBG:J:B:A:20211221000" };
    const INTERIOR_NUL: Wire =
        Wire { game_id: "bomber\0girl", updater_version: "QBG:J:C:A:2021122100", launcher_version: "QBG:J:B:A:2021122100" };

    /// Overflow fixtures vary the three boot fields and the layout under test.
    /// Host and prefix stay the admitted profile's and are not what these cases change.
    fn profile_wire(wire: &Wire, layout: crate::title::BootLayout) -> crate::DiscoveryProfile {
        crate::DiscoveryProfile {
            game_id: wire.game_id,
            updater_version: wire.updater_version,
            launcher_version: wire.launcher_version,
            layout,
            ..*crate::discovery_profile(GameFamily::Bombergirl).unwrap()
        }
    }

    #[test]
    fn fixed_inputs_are_not_a_caller_supplied_dispatch() {
        let profile = crate::discovery_profile(GameFamily::Bombergirl).unwrap();
        assert_eq!(profile.game_id, "bomber-girl");
        assert_eq!(profile.updater_version, "QBG:J:C:A:2021122100");
        assert_eq!(profile.launcher_version, "QBG:J:B:A:2021122100");
        let sdvx = crate::discovery_profile(GameFamily::Sdvx).unwrap();
        assert_eq!(sdvx.updater_version, "QCV:J:C:A:2023021500");
        assert_eq!(sdvx.launcher_version, "QCV:J:A:A:2023021500");
        assert_eq!(sdvx.layout, crate::title::BootLayout::Services);
        assert_eq!(sdvx.record_bytes(), SERVICES_RECORD_BYTES);
        assert_eq!(profile.layout, crate::title::BootLayout::Legacy);
        assert_eq!(profile.record_bytes(), LEGACY_RECORD_BYTES);
        assert_eq!(PLACEHOLDER, b"public-metadata");
        assert_eq!(MODE_METADATA, 2);
        assert_eq!(MODE_LAUNCH, 1);
        assert_eq!(NORMAL_TERMINAL, 1);
        assert_eq!(WORKER_FINISHED, 2);
        assert_eq!(AVS_NAME, "avs2-core.dll");
        assert_eq!(EAC_NAME, "libeacnet.dll");
        assert_eq!(URL_CAP, 0x800);
        assert_eq!(HASH_CAP, 0x41);
        assert_eq!(RAW_SOURCE, b"work\\updater\\dev\\raw\0");
        assert_eq!(NVRAM_SOURCE, b"work\\updater\\dev\\nvram\0");
        assert!(RAW_SOURCE.is_ascii() && NVRAM_SOURCE.is_ascii());
        assert!(!RAW_SOURCE.contains(&b':') && !NVRAM_SOURCE.contains(&b':'));
        for name in [
            EXPORT_BOOT,
            EXPORT_STATE,
            EXPORT_SHUTDOWN,
            EXPORT_ERROR,
            EXPORT_BOOT_RESULT,
            EXPORT_RESOURCE_SEND,
            EXPORT_RESOURCE_POLL,
            EXPORT_RESOURCE_COPY,
        ] {
            assert!(name.ends_with(&[0]));
        }
        // Discovery stays on the public resource exports. Download stays unbound.
        let forbidden = [b"OA043711000033\0".as_slice()];
        for name in [
            EXPORT_BOOT,
            EXPORT_STATE,
            EXPORT_SHUTDOWN,
            EXPORT_ERROR,
            EXPORT_BOOT_RESULT,
            EXPORT_RESOURCE_SEND,
            EXPORT_RESOURCE_POLL,
            EXPORT_RESOURCE_COPY,
        ] {
            assert!(!forbidden.contains(&name));
        }
        assert!(!matches!(EXPORT_RESOURCE_SEND, EXPORT_MODULE_SEND | EXPORT_START_SEND));
    }

    #[test]
    fn resource_poll_is_called_once_and_stops_when_released() {
        let mut ticks = [0, 0, 2].into_iter();
        let mut reads = 0;
        assert_eq!(
            wait_for_resource(
                Duration::from_secs(1),
                Duration::ZERO,
                || ticks.next().expect("must not poll after completion"),
                || {
                    reads += 1;
                    0
                }
            ),
            Ok(())
        );
        assert_eq!(reads, 1);
    }

    #[test]
    fn released_poll_with_nonzero_error_is_not_success_and_is_not_polled_again() {
        let mut ticks = [2].into_iter();
        assert_eq!(
            wait_for_resource(Duration::from_secs(1), Duration::ZERO, || ticks.next().expect("released request must not be reused"), || 1),
            Err(RuntimeError::Runtime)
        );
    }

    #[test]
    fn pending_poll_that_never_releases_times_out_without_reading_error() {
        assert_eq!(
            wait_for_resource(Duration::ZERO, Duration::ZERO, || 0, || panic!("no completed response to read")),
            Err(RuntimeError::Timeout)
        );
        assert_eq!(
            wait_for_resource(Duration::from_secs(1), Duration::ZERO, || 3, || panic!("unknown poll result must fail closed")),
            Err(RuntimeError::Runtime)
        );
    }

    #[test]
    fn copied_strings_stop_at_the_first_nul() {
        let mut bytes = [0_u8; 8];
        bytes[..5].copy_from_slice(b"ab\0cd");
        assert_eq!(c_string(&bytes).unwrap(), "ab");
    }

    #[test]
    fn mount_sources_are_fixed_relative_ascii() {
        for source in [RAW_SOURCE, NVRAM_SOURCE] {
            assert_eq!(source.iter().filter(|byte| **byte == 0).count(), 1);
            assert!(source.ends_with(&[0]));
            let text = std::str::from_utf8(&source[..source.len() - 1]).unwrap();
            assert!(text.is_ascii());
            assert!(text.starts_with("work\\updater\\dev\\"));
            assert!(!text.contains(':') && !Path::new(text).is_absolute());
        }
    }

    struct Script {
        module_steps: Cell<Vec<Step>>,
        start_steps: Cell<Vec<Step>>,
        error: Cell<i32>,
        /// Error value returned on and after this many `error()` calls.
        error_after: Cell<i32>,
        error_reads: Cell<i32>,
        boot: Cell<i32>,
        calls: Cell<Vec<&'static str>>,
        start_return: Cell<i32>,
        version: Cell<Option<Vec<u8>>>,
        token: Cell<Option<Vec<u8>>>,
        digest: Vec<u8>,
    }

    enum Step {
        Pending,
        Released,
        Unknown,
        /// Stop answering. A later poll is a test failure, not a timeout.
        Closed,
    }

    impl Script {
        fn new(module_steps: Vec<Step>, start_steps: Vec<Step>, digest: &str) -> Self {
            let mut bytes = digest.as_bytes().to_vec();
            bytes.push(0);
            Self {
                module_steps: Cell::new(module_steps),
                start_steps: Cell::new(start_steps),
                error: Cell::new(0),
                error_after: Cell::new(i32::MAX),
                error_reads: Cell::new(0),
                boot: Cell::new(NORMAL_TERMINAL),
                calls: Cell::new(Vec::new()),
                start_return: Cell::new(1),
                version: Cell::new(None),
                token: Cell::new(None),
                digest: bytes,
            }
        }

        fn note(&self, name: &'static str) {
            let mut calls = self.calls.take();
            calls.push(name);
            self.calls.set(calls);
        }

        fn next(slot: &Cell<Vec<Step>>) -> i32 {
            let mut steps = slot.take();
            let step = steps.remove(0);
            slot.set(steps);
            match step {
                Step::Pending => 0,
                Step::Released => REQUEST_RELEASED,
                Step::Unknown => 7,
                Step::Closed => panic!("poll continued after the request was consumed"),
            }
        }
    }

    impl EacStage for Script {
        fn error(&self) -> i32 {
            self.note("error");
            let reads = self.error_reads.get() + 1;
            self.error_reads.set(reads);
            if reads >= self.error_after.get() { self.error.get() } else { 0 }
        }
        fn boot_result(&self) -> i32 {
            self.note("boot");
            self.boot.get()
        }
        fn module_send(&self, version: *const u8) -> i32 {
            self.note("module-send");
            // SAFETY: the stage passes a 21-byte NUL-terminated version.
            let text = unsafe { CStr::from_ptr(version.cast()) };
            self.version.set(Some(text.to_bytes().to_vec()));
            REQUEST_ENQUEUED
        }
        fn module_poll(&self) -> i32 {
            self.note("module-poll");
            Self::next(&self.module_steps)
        }
        fn module_copy(&self, out: *mut u8, capacity: i32) -> i32 {
            self.note("module-copy");
            assert_eq!(capacity, MODULE_HASH_CAP);
            assert!(self.digest.len() <= capacity as usize);
            // SAFETY: the stage passes a 65-byte writable buffer.
            unsafe {
                std::ptr::copy_nonoverlapping(self.digest.as_ptr(), out, self.digest.len());
            }
            COPY_OK
        }
        fn start_send(&self, token: *const u8) -> i32 {
            self.note("start-send");
            // SAFETY: the stage passes a 256-byte zeroed C string.
            let text = unsafe { CStr::from_ptr(token.cast()) };
            self.token.set(Some(text.to_bytes().to_vec()));
            self.start_return.get()
        }
        fn start_poll(&self) -> i32 {
            self.note("start-poll");
            Self::next(&self.start_steps)
        }
    }

    fn expected_digest() -> [u8; 32] {
        let mut digest = [0_u8; 32];
        digest[0] = 0xab;
        digest
    }

    fn hex_digest(digest: &[u8; 32]) -> String {
        hex_encode(digest)
    }

    fn run_script(script: &Script, digest: &[u8; 32]) -> Result<(), RuntimeError> {
        verify_stages_for(script, digest, STAGE_DEADLINE)
    }

    #[test]
    fn launch_record_is_mode1_with_launcher_version_and_dn_clear() {
        let admitted = admitted();
        assert!(!admitted.is_empty());
        for (_, profile) in admitted {
            assert_boot_wire(profile, BootMode::Launch);
            let mut record = boot_record(profile, BootMode::Launch).unwrap();
            let token = token_bytes(b"ab-CD_12").unwrap();
            write_token(record.as_mut_slice(), token.as_slice()).unwrap();
            assert_eq!(&record[VERSION_AT..VERSION_AT + profile.launcher_version.len()], profile.launcher_version.as_bytes());
            assert_ne!(profile.launcher_version, profile.updater_version);
            assert_eq!(&record[TOKEN_AT..TOKEN_AT + 8], b"ab-CD_12");
            assert_eq!(record[TOKEN_AT + 8], 0);
            assert_eq!(record[FLAG_AT], 0);
            assert!(record[TOKEN_AT + 9..].iter().all(|byte| *byte == 0));
            record.zeroize();
            assert!(record.iter().all(|byte| *byte == 0));
        }
    }

    #[test]
    fn token_bytes_keep_the_callback_alphabet_and_reject_format_or_space() {
        assert!(token_bytes(b"a").is_ok());
        assert!(token_bytes(b"ab-CD_12+/=xy").is_ok());
        assert!(token_bytes(&[b'a'; 64]).is_ok());
        assert!(token_bytes(b"").is_err());
        assert!(token_bytes(&[b'a'; 65]).is_err());
        for bad in [b"has space".as_slice(), b"per%cent", b"nu\0l", b"line\n", b"star*"] {
            assert!(token_bytes(bad).is_err());
        }
    }

    #[test]
    fn module_check_runs_before_start_and_a_matching_digest_returns_ok() {
        let digest = expected_digest();
        let script =
            Script::new(vec![Step::Pending, Step::Released, Step::Closed], vec![Step::Released, Step::Closed], &hex_digest(&digest));
        assert_eq!(run_script(&script, &digest), Ok(()));
        assert_eq!(
            script.calls.take(),
            [
                "boot",
                "error",
                "module-send",
                "module-poll",
                "module-poll",
                "error",
                "module-copy",
                "boot",
                "error",
                "start-send",
                "error",
                "start-poll",
                "error",
                "boot",
            ]
        );
        assert_eq!(script.version.take().unwrap(), b"QBG:J:A:A:2026041400");
        assert_eq!(script.token.take().unwrap(), b"fresh-token");
    }

    #[test]
    fn start_send_requires_nonzero_and_error_zero_before_the_poll() {
        let digest = expected_digest();
        let zero = Script::new(vec![Step::Released, Step::Closed], Vec::new(), &hex_digest(&digest));
        zero.start_return.set(0);
        assert_eq!(run_script(&zero, &digest), Err(RuntimeError::Runtime));
        assert!(!zero.calls.take().contains(&"start-poll"));

        // Boot, module release, pre-start, then the start-send error read.
        let dirty = Script::new(vec![Step::Released, Step::Closed], Vec::new(), &hex_digest(&digest));
        dirty.error.set(1);
        dirty.error_after.set(4);
        assert_eq!(run_script(&dirty, &digest), Err(RuntimeError::Runtime));
        let calls = dirty.calls.take();
        assert!(calls.contains(&"start-send"));
        assert!(!calls.contains(&"start-poll"));
    }

    #[test]
    fn a_released_poll_is_not_permission_and_is_not_polled_again() {
        let digest = expected_digest();
        let script = Script::new(vec![Step::Released, Step::Closed], vec![Step::Released, Step::Closed], &hex_digest(&digest));
        script.error.set(0);
        assert_eq!(run_script(&script, &digest), Ok(()));
        let calls = script.calls.take();
        assert_eq!(calls.iter().filter(|call| **call == "module-poll").count(), 1);
        assert_eq!(calls.iter().filter(|call| **call == "start-poll").count(), 1);
    }

    #[test]
    fn module_mismatch_and_a_malformed_digest_never_send_start() {
        let digest = expected_digest();
        let wrong = Script::new(vec![Step::Released, Step::Closed], Vec::new(), &"ab".repeat(32));
        assert_eq!(run_script(&wrong, &digest), Err(RuntimeError::ModuleMismatch));
        assert!(!wrong.calls.take().iter().any(|call| call.starts_with("start")));
        let malformed = Script::new(vec![Step::Released, Step::Closed], Vec::new(), "ABCDEF");
        assert_eq!(run_script(&malformed, &digest), Err(RuntimeError::ModuleMismatch));
        assert!(!malformed.calls.take().iter().any(|call| call.starts_with("start")));
        let empty = Script::new(vec![Step::Released, Step::Closed], Vec::new(), "");
        assert_eq!(run_script(&empty, &digest), Err(RuntimeError::ModuleMismatch));
    }

    #[test]
    fn a_nonzero_start_error_is_denied_without_a_retry() {
        let digest = expected_digest();
        // Boot, module release, pre-start, and the post-poll read. The fifth
        // error read is the one that approves or denies 000041.
        for code in [1, 2002, 2003, 2004, 2005] {
            let script = Script::new(vec![Step::Released, Step::Closed], vec![Step::Released, Step::Closed], &hex_digest(&digest));
            script.error.set(code);
            script.error_after.set(5);
            assert_eq!(run_script(&script, &digest), Err(RuntimeError::VerificationDenied));
            assert_eq!(script.calls.take().iter().filter(|call| **call == "start-poll").count(), 1);
        }
    }

    #[test]
    fn a_pending_stage_times_out_and_an_unknown_sentinel_fails_closed() {
        let digest = expected_digest();
        let pending = Script::new(vec![Step::Pending], Vec::new(), &hex_digest(&digest));
        assert_eq!(verify_stages_for(&pending, &digest, Duration::ZERO), Err(RuntimeError::Timeout));
        assert!(!pending.calls.take().iter().any(|call| call.starts_with("start")));
        let start_pending = Script::new(vec![Step::Released, Step::Closed], vec![Step::Pending], &hex_digest(&digest));
        assert_eq!(verify_stages_for(&start_pending, &digest, Duration::ZERO), Err(RuntimeError::Timeout));
        let unknown = Script::new(vec![Step::Unknown], Vec::new(), &hex_digest(&digest));
        assert_eq!(verify_stages_for(&unknown, &digest, Duration::from_secs(1)), Err(RuntimeError::Runtime));
        let start_unknown = Script::new(vec![Step::Released, Step::Closed], vec![Step::Unknown], &hex_digest(&digest));
        assert_eq!(verify_stages_for(&start_unknown, &digest, Duration::from_secs(1)), Err(RuntimeError::VerificationDenied));
    }

    fn verify_stages_for(eac: &Script, expected: &[u8; 32], timeout: Duration) -> Result<(), RuntimeError> {
        verify_stages(eac, "QBG:J:A:A:2026041400", b"fresh-token", expected, timeout)
    }

    #[test]
    fn heartbeat_failure_is_not_accepted_as_permission() {
        let digest = expected_digest();
        let script = Script::new(vec![Step::Released, Step::Closed], Vec::new(), &hex_digest(&digest));
        script.boot.set(2);
        assert_eq!(run_script(&script, &digest), Err(RuntimeError::Runtime));
        assert!(script.calls.take().iter().all(|call| !call.starts_with("module") && !call.starts_with("start")));
        // A healthy boot that later reports heartbeat failure is still not permission.
        let later = Script::new(vec![Step::Released, Step::Closed], vec![Step::Released, Step::Closed], &hex_digest(&digest));
        later.boot.set(NORMAL_TERMINAL);
        let stage = BootAfterModule { inner: &later, reads: Cell::new(0) };
        assert_eq!(verify_stages(&stage, "QBG:J:A:A:2026041400", b"fresh-token", &digest, STAGE_DEADLINE), Err(RuntimeError::Runtime));
        assert!(!later.calls.take().iter().any(|call| call.starts_with("start")));
    }

    struct BootAfterModule<'a> {
        inner: &'a Script,
        reads: Cell<i32>,
    }

    impl EacStage for BootAfterModule<'_> {
        fn error(&self) -> i32 {
            self.inner.error()
        }
        fn boot_result(&self) -> i32 {
            let reads = self.reads.get() + 1;
            self.reads.set(reads);
            if reads >= 2 { 2 } else { self.inner.boot_result() }
        }
        fn module_send(&self, version: *const u8) -> i32 {
            self.inner.module_send(version)
        }
        fn module_poll(&self) -> i32 {
            self.inner.module_poll()
        }
        fn module_copy(&self, out: *mut u8, capacity: i32) -> i32 {
            self.inner.module_copy(out, capacity)
        }
        fn start_send(&self, token: *const u8) -> i32 {
            self.inner.start_send(token)
        }
        fn start_poll(&self) -> i32 {
            self.inner.start_poll()
        }
    }
}
