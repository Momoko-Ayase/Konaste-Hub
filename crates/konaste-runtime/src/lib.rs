//! Pinned official runtime worker for verified-title discovery and launch.
//!
//! Discovery uses normal mode 2 with a fixed non-account placeholder. Verification
//! and launch preparation use normal mode 1 with one fresh native token, the
//! official module digest, and game-start authorization. Operations and receipts
//! are not interchangeable. No installation directory is mounted and no updater
//! plan is returned. The game is created only by [`PreparedLaunch::start`], after
//! the SDK worker has stayed alive through verification. Tokens exist only in
//! zeroizing native memory, private pipes, and the normal game parameter
//! string. They never enter worker argv, environment, settings, snapshots, or a
//! frontend reply.
//!
//! The caller supplies its own executable, whose entry point must dispatch
//! [`WORKER_ARGUMENT`] to [`worker_main`] before starting its GUI/runtime. Only
//! that exact native executable is spawned as the SDK worker; no shell is used.
//! The worker is job-limited, not a general filesystem sandbox. Its verified
//! native contract mounts only fresh scratch storage, never an installation. The
//! game process is not placed in that job.

pub mod infinitas;
mod installation;
mod settings;
mod title;
pub use installation::{InstallationError, InstallationPin, InstalledGame, inspect_installation};
pub use settings::{
    LAUNCHER_SETTINGS_PATH, PreparedSettings, SDVX_SETTINGS_PATH, SettingsPin, SettingsSession, prepare_settings, settings_path,
};
pub use title::{DiscoveryProfile, GitadoraMode, Title, discovery_profile, title};

#[cfg(all(windows, target_arch = "x86_64"))]
mod arguments;
#[cfg(all(windows, target_arch = "x86_64"))]
pub use arguments::popn_resource_dir;
mod diag;
pub use diag::last_worker_note;
#[cfg(all(windows, target_arch = "x86_64"))]
mod game;
#[cfg(all(windows, target_arch = "x86_64"))]
pub(crate) mod official;
#[cfg(all(windows, target_arch = "x86_64"))]
mod process;

use std::{io::Read, path::Path};

use konaste_bootstrap::GameFamily;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::Zeroizing;

pub const WORKER_ARGUMENT: &str = "--konaste-runtime-worker";
const PROTOCOL: u32 = 4;
const MAX_REQUEST_BYTES: u64 = 64 * 1024;
const MAX_RECEIPT_BYTES: usize = 4096;
/// Fixed native session controls, never export dispatch or a spawn command.
const FINISH: u8 = 0;
const CHECK: u8 = 1;
const ALIVE: u8 = 0xA5;
#[cfg(test)]
const HOST: &str = "d1q1iflay6y5jj.cloudfront.net";
/// How long a Ready receipt may be turned into a game process.
const PREPARED_TTL: std::time::Duration = std::time::Duration::from_secs(20);

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RuntimeError {
    #[error("the requested native contract is not verified for this title")]
    Unsupported,
    #[error("a pinned official runtime pair is missing")]
    ComponentsMissing,
    #[error("official runtime validation or initialization failed")]
    Runtime,
    #[error("the official runtime worker failed")]
    Worker,
    #[error("the official runtime worker timed out")]
    Timeout,
    #[error("invalid official worker response")]
    Protocol,
    #[error("the official service did not authorize this launch attempt")]
    VerificationDenied,
    #[error("the game module does not match the official module hash")]
    ModuleMismatch,
    #[error("choose GuitarFreaks or DrumMania before launching GITADORA")]
    GameModeRequired,
    #[error("the game settings could not be read safely")]
    GameSettings,
    #[error("the game process could not be created")]
    GameStart,
    #[error("the game process could not be observed")]
    GameWait,
}

/// Public resource URL and digest returned by one fresh worker.
pub struct ResourceLocation {
    pub family: GameFamily,
    pub url: String,
    pub sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    protocol: u32,
    family: GameFamily,
    cache_root: std::path::PathBuf,
    operation: Operation,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Operation {
    Discover,
    Verify(VerificationInput),
    /// Same mode-1 pipeline as Verify, but the child stays alive after success.
    Prepare(VerificationInput),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VerificationInput {
    pub product_version: String,
    pub module_sha256: [u8; 32],
    pub token: LaunchToken,
}

// No Debug/Display, frontend serialization, environment or command-line handoff.
pub(crate) struct LaunchToken(Zeroizing<String>);
impl LaunchToken {
    fn new(value: Zeroizing<String>) -> Result<Self, RuntimeError> {
        if value.is_empty() || value.len() > 64 || !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_-+/=".contains(&byte)) {
            return Err(RuntimeError::Protocol);
        }
        Ok(Self(value))
    }
    pub(crate) fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}
impl Serialize for LaunchToken {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for LaunchToken {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Zeroizing::new(String::deserialize(deserializer)?);
        Self::new(value).map_err(|_| serde::de::Error::custom("invalid launch token"))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WorkerReceipt {
    Discovery(Receipt),
    Verification(VerificationReceipt),
    /// Typed success while the SDK session is still alive. Not a reusable grant.
    Prepared(VerificationReceipt),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VerificationReceipt {
    protocol: u32,
    family: GameFamily,
    status: VerificationStatus,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum VerificationStatus {
    Allowed,
    Denied,
    ModuleMismatch,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    protocol: u32,
    family: GameFamily,
    url: String,
    sha256: String,
}

/// Blocking; call outside the GUI thread. Never downloads bootstrap packages.
///
/// A fresh worker owns a fresh scratch directory and pinned DLL copies. The
/// parent enforces a 45-second worker lifetime. A failed discovery cannot return
/// a retained successful result.
pub fn discover(family: GameFamily, cache_root: &Path, executable: &Path) -> Result<ResourceLocation, RuntimeError> {
    discovery_profile(family)?;
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        match process::request(family, cache_root, executable, Operation::Discover)? {
            WorkerReceipt::Discovery(receipt) => checked_receipt(&receipt, family),
            _ => Err(RuntimeError::Protocol),
        }
    }
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        let _ = (cache_root, executable);
        Err(RuntimeError::Unsupported)
    }
}

/// Verify one fresh native token against the normal official mode-1/module/start
/// checks. The caller holds `game`'s immutable file leases throughout the call.
/// This function never creates a game process or returns reusable entitlement.
/// The SDK worker shuts down before this returns.
pub fn verify_launch(cache_root: &Path, executable: &Path, game: &InstalledGame, token: Zeroizing<String>) -> Result<(), RuntimeError> {
    require_supported(game.family())?;
    let token = LaunchToken::new(token)?;
    if !installation::valid_game_version(game.family(), game.product_version()) {
        return Err(RuntimeError::Protocol);
    }
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        let input = VerificationInput { product_version: game.product_version().into(), module_sha256: game.module_sha256(), token };
        let receipt = process::request(game.family(), cache_root, executable, Operation::Verify(input))?;
        checked_verification(receipt, game.family())
    }
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        let _ = (cache_root, executable, token);
        Err(RuntimeError::Unsupported)
    }
}

/// Run the same mode-1 pipeline in a fresh job-limited SDK child and return only
/// after verification succeeds, while that child is still alive. Does not create
/// a game. Dropping the result shuts the child down. There is no cached grant.
pub fn prepare_launch(
    cache_root: &Path,
    executable: &Path,
    game: InstalledGame,
    token: Zeroizing<String>,
    mode: Option<GitadoraMode>,
) -> Result<PreparedLaunch, RuntimeError> {
    require_supported(game.family())?;
    let token = LaunchToken::new(token)?;
    if !installation::valid_game_version(game.family(), game.product_version()) {
        return Err(RuntimeError::Protocol);
    }
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        let input = VerificationInput { product_version: game.product_version().into(), module_sha256: game.module_sha256(), token };
        let arguments = arguments::Arguments::load(title(game.family())?.launch()?.arguments, mode, game.root())?;
        let held = process::prepare(game.family(), cache_root, executable, &input)?;
        Ok(PreparedLaunch { game, arguments, token: input.token, ready: std::time::Instant::now(), session: held })
    }
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        let _ = (cache_root, executable, token, mode);
        Err(RuntimeError::Unsupported)
    }
}

/// One verified SDK session, still alive, plus the install leases and the fresh
/// token. Not serializable, not `Clone`, and not a reusable entitlement.
pub struct PreparedLaunch {
    #[cfg_attr(not(all(windows, target_arch = "x86_64")), allow(dead_code))]
    game: InstalledGame,
    #[cfg_attr(not(all(windows, target_arch = "x86_64")), allow(dead_code))]
    token: LaunchToken,
    ready: std::time::Instant,
    #[cfg(all(windows, target_arch = "x86_64"))]
    session: process::HeldSession,
    #[cfg(all(windows, target_arch = "x86_64"))]
    arguments: arguments::Arguments,
}

impl PreparedLaunch {
    pub fn root(&self) -> &Path {
        self.game.root()
    }

    /// Create this title's pinned game once. Fails before creation when this receipt
    /// is older than [`PREPARED_TTL`] or the SDK child has already exited. After
    /// creation, SDK shutdown failure is not reported as a launch failure.
    /// The native host's final guard runs after the live SDK acknowledgment,
    /// immediately before creation, while the host holds its identity locks.
    pub fn start<E: From<RuntimeError>>(mut self, final_guard: impl FnOnce() -> Result<(), E>) -> Result<GameSession, E> {
        #[cfg(all(windows, target_arch = "x86_64"))]
        {
            if self.ready.elapsed() >= PREPARED_TTL || !self.session.alive() {
                return Err(E::from(RuntimeError::GameStart));
            }
            // A living process may already be stuck shutting down a failed SDK.
            // Require a fresh acknowledgment from the still-healthy session.
            self.session.confirm().map_err(E::from)?;
            if self.ready.elapsed() >= PREPARED_TTL || !self.session.alive() {
                return Err(E::from(RuntimeError::GameStart));
            }
            final_guard()?;
            let created = game::start(&self.game, self.token.expose(), &self.arguments);
            // The SDK session ends with creation, not with the game. A shutdown
            // failure after the process exists does not undo that process.
            drop(self.session);
            let process = created.map_err(E::from)?;
            Ok(GameSession { _game: self.game, process })
        }
        #[cfg(not(all(windows, target_arch = "x86_64")))]
        {
            let _ = (self, final_guard);
            Err(E::from(RuntimeError::Unsupported))
        }
    }
}

/// Retained game process and the install leases. Drop closes the handle and the
/// leases. It does not terminate the game.
pub struct GameSession {
    #[cfg_attr(not(all(windows, target_arch = "x86_64")), allow(dead_code))]
    _game: InstalledGame,
    #[cfg(all(windows, target_arch = "x86_64"))]
    process: game::GameProcess,
}

impl GameSession {
    /// Block until the game exits and return that exit code. `259` is a real
    /// exit code here, not `STILL_ACTIVE`, because the wait has completed.
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

fn checked_verification(receipt: WorkerReceipt, family: GameFamily) -> Result<(), RuntimeError> {
    let WorkerReceipt::Verification(receipt) = receipt else {
        return Err(RuntimeError::Protocol);
    };
    checked_stage(&receipt, family)
}

pub(crate) fn checked_prepared(receipt: &WorkerReceipt, family: GameFamily) -> Result<(), RuntimeError> {
    let WorkerReceipt::Prepared(receipt) = receipt else {
        return Err(RuntimeError::Protocol);
    };
    checked_stage(receipt, family)
}

fn checked_stage(receipt: &VerificationReceipt, family: GameFamily) -> Result<(), RuntimeError> {
    require_supported(family)?;
    if receipt.protocol != PROTOCOL || receipt.family != family {
        return Err(RuntimeError::Protocol);
    }
    match receipt.status {
        VerificationStatus::Allowed => Ok(()),
        VerificationStatus::Denied => Err(RuntimeError::VerificationDenied),
        VerificationStatus::ModuleMismatch => Err(RuntimeError::ModuleMismatch),
    }
}

fn require_supported(family: GameFamily) -> Result<(), RuntimeError> {
    title(family)?.launch().map(|_| ())
}

fn inspected_pair(family: GameFamily, cache_root: &Path) -> Result<konaste_bootstrap::RuntimePair, RuntimeError> {
    match konaste_bootstrap::inspect(family, cache_root).map_err(|_| RuntimeError::Runtime)? {
        konaste_bootstrap::PairStatus::Present(pair) => Ok(pair),
        konaste_bootstrap::PairStatus::Missing => Err(RuntimeError::ComponentsMissing),
        konaste_bootstrap::PairStatus::Stale { .. } => Err(RuntimeError::Runtime),
    }
}

/// Worker entry point for the native executable, before any UI setup.
///
/// Stdin is one length-prefixed native-only request. Stdout is one length-prefixed
/// typed receipt; errors carry no raw library/page/HTTP text. Discover and Verify
/// then exit. Prepare writes its receipt while the SDK session is still alive and
/// accepts one fixed live-health check and a finish byte. None is frontend IPC.
pub fn worker_main() -> Result<(), RuntimeError> {
    let result = worker_run();
    if let Err(error) = &result {
        diag::report(error);
    }
    result
}

fn worker_run() -> Result<(), RuntimeError> {
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        diag::stage("request");
        let mut stdin = std::io::stdin();
        let request = read_frame(&mut stdin, MAX_REQUEST_BYTES as usize)?;
        let request: Request = serde_json::from_slice(&request).map_err(|_| RuntimeError::Protocol)?;
        if request.protocol != PROTOCOL {
            return Err(RuntimeError::Protocol);
        }
        match &request.operation {
            Operation::Discover => {
                discovery_profile(request.family)?;
            }
            Operation::Verify(_) | Operation::Prepare(_) => require_supported(request.family)?,
        }
        if let Operation::Verify(input) | Operation::Prepare(input) = &request.operation
            && !installation::valid_game_version(request.family, &input.product_version)
        {
            return Err(RuntimeError::Protocol);
        }
        process::require_job()?;
        let scratch = std::env::current_dir().map_err(|_| RuntimeError::Worker)?;
        let _scratch_pins = official::pin_root(&scratch)?;
        // Only the parent-created empty scratch root is usable, never an install.
        if !scratch.file_name().is_some_and(|name| name.to_string_lossy().starts_with("konaste-runtime-"))
            || std::fs::read_dir(&scratch).map_err(|_| RuntimeError::Worker)?.next().is_some()
        {
            return Err(RuntimeError::Worker);
        }
        diag::stage("pair");
        let pair = inspected_pair(request.family, &request.cache_root)?;
        let package = konaste_bootstrap::package_for(request.family);
        for (source, name, length) in
            [(&pair.avs2_core, "avs2-core.dll", package.avs2_core_bytes), (&pair.libeacnet, "libeacnet.dll", package.libeacnet_bytes)]
        {
            let source = std::fs::File::open(source).map_err(|_| RuntimeError::Runtime)?;
            let mut target =
                std::fs::OpenOptions::new().write(true).create_new(true).open(scratch.join(name)).map_err(|_| RuntimeError::Worker)?;
            if std::io::copy(&mut source.take(length + 1), &mut target).map_err(|_| RuntimeError::Runtime)? != length {
                return Err(RuntimeError::Runtime);
            }
        }
        let receipt = match request.operation {
            Operation::Discover => {
                let receipt = official::discover(request.family, &scratch)?;
                checked_receipt(&receipt, request.family)?;
                WorkerReceipt::Discovery(receipt)
            }
            Operation::Verify(input) => {
                WorkerReceipt::Verification(stage_receipt(request.family, official::verify(request.family, &scratch, &input))?)
            }
            Operation::Prepare(input) => {
                // The callback runs after verification and before SDK shutdown.
                // A denied or mismatched stage never reaches it. The hold keeps
                // reading the live boot result; a dead session is not permission.
                let mut sent = false;
                let result = official::prepare(request.family, &scratch, &input, |session| {
                    session.healthy()?;
                    let receipt = stage_receipt(request.family, Ok(()))?;
                    write_receipt(&WorkerReceipt::Prepared(receipt))?;
                    sent = true;
                    // The detached reader accepts only health-check and finish
                    // controls. A failed SDK never acknowledges a creation check.
                    official::hold_until_finish(session, stdin, PREPARED_TTL)
                });
                if sent {
                    return result;
                }
                return match result {
                    Err(error @ (RuntimeError::VerificationDenied | RuntimeError::ModuleMismatch)) => {
                        write_receipt(&WorkerReceipt::Prepared(stage_receipt(request.family, Err(error))?))
                    }
                    Err(error) => Err(error),
                    Ok(()) => Err(RuntimeError::Protocol),
                };
            }
        };
        write_receipt(&receipt)
    }
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        Err(RuntimeError::Unsupported)
    }
}

fn stage_receipt(family: GameFamily, result: Result<(), RuntimeError>) -> Result<VerificationReceipt, RuntimeError> {
    let status = match result {
        Ok(()) => VerificationStatus::Allowed,
        Err(RuntimeError::VerificationDenied) => VerificationStatus::Denied,
        Err(RuntimeError::ModuleMismatch) => VerificationStatus::ModuleMismatch,
        Err(error) => return Err(error),
    };
    Ok(VerificationReceipt { protocol: PROTOCOL, family, status })
}

fn write_receipt(receipt: &WorkerReceipt) -> Result<(), RuntimeError> {
    use std::io::Write;
    let body = Zeroizing::new(serde_json::to_vec(receipt).map_err(|_| RuntimeError::Protocol)?);
    if body.len() > MAX_RECEIPT_BYTES {
        return Err(RuntimeError::Protocol);
    }
    let mut frame = Zeroizing::new(Vec::with_capacity(4 + body.len()));
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&frame).map_err(|_| RuntimeError::Protocol)?;
    stdout.flush().map_err(|_| RuntimeError::Protocol)
}

/// One strict length prefix plus a bounded body. A short read, an interior length
/// that does not match, or a body past the cap is a protocol failure.
fn read_frame(reader: &mut impl Read, limit: usize) -> Result<Zeroizing<Vec<u8>>, RuntimeError> {
    let mut length = [0_u8; 4];
    reader.read_exact(&mut length).map_err(|_| RuntimeError::Protocol)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > limit {
        return Err(RuntimeError::Protocol);
    }
    let mut body = Zeroizing::new(vec![0_u8; length]);
    reader.read_exact(&mut body).map_err(|_| RuntimeError::Protocol)?;
    Ok(body)
}

fn checked_receipt(receipt: &Receipt, family: GameFamily) -> Result<ResourceLocation, RuntimeError> {
    discovery_profile(family)?;
    if receipt.protocol != PROTOCOL
        || receipt.family != family
        || receipt.url.len() >= 0x800
        || !receipt.url.is_ascii()
        || receipt.url.bytes().any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        || receipt.sha256.len() != 64
        || !receipt.sha256.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(RuntimeError::Protocol);
    }
    discovery_profile(family)?.resource_url(&receipt.url).ok_or(RuntimeError::Protocol)?;
    let mut digest = [0; 32];
    for (i, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&receipt.sha256[2 * i..2 * i + 2], 16).map_err(|_| RuntimeError::Protocol)?;
    }
    let _ = digest;
    Ok(ResourceLocation { family, url: receipt.url.clone(), sha256: receipt.sha256.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked_verification(receipt: WorkerReceipt) -> Result<(), RuntimeError> {
        super::checked_verification(receipt, GameFamily::Bombergirl)
    }

    fn checked_prepared(receipt: &WorkerReceipt) -> Result<(), RuntimeError> {
        super::checked_prepared(receipt, GameFamily::Bombergirl)
    }

    fn receipt() -> Receipt {
        Receipt {
            protocol: PROTOCOL,
            family: GameFamily::Bombergirl,
            url: format!("https://{HOST}/ResourceInfo/synthetic/test.xml"),
            sha256: "a0".repeat(32),
        }
    }

    #[test]
    fn launch_tokens_match_native_callback_bounds_without_truncation() {
        for value in ["synthetic-token", "Base64_-/+=0123"] {
            let token = LaunchToken::new(Zeroizing::new(value.into())).unwrap();
            assert_eq!(token.as_bytes(), value.as_bytes());
        }
        for value in [
            "".to_string(),
            "a".repeat(65),
            "with space".into(),
            "bad%value".into(),
            "bad\0value".into(),
            "bad\nvalue".into(),
            "非ASCII".into(),
        ] {
            assert!(LaunchToken::new(Zeroizing::new(value)).is_err());
        }
        assert!(LaunchToken::new(Zeroizing::new("a".repeat(64))).is_ok());
    }

    #[test]
    fn verification_receipts_bind_the_operation_protocol_and_title() {
        let make = |protocol, family, status| WorkerReceipt::Verification(VerificationReceipt { protocol, family, status });
        assert_eq!(checked_verification(make(PROTOCOL, GameFamily::Bombergirl, VerificationStatus::Allowed)), Ok(()));
        assert_eq!(
            checked_prepared(&WorkerReceipt::Prepared(VerificationReceipt {
                protocol: PROTOCOL,
                family: GameFamily::Bombergirl,
                status: VerificationStatus::Allowed,
            })),
            Ok(())
        );
        assert_eq!(
            checked_prepared(&WorkerReceipt::Verification(VerificationReceipt {
                protocol: PROTOCOL,
                family: GameFamily::Bombergirl,
                status: VerificationStatus::Allowed,
            })),
            Err(RuntimeError::Protocol)
        );
        assert_eq!(
            checked_verification(make(PROTOCOL - 1, GameFamily::Bombergirl, VerificationStatus::Allowed)),
            Err(RuntimeError::Protocol)
        );
        assert_eq!(checked_verification(make(PROTOCOL, GameFamily::Sdvx, VerificationStatus::Allowed)), Err(RuntimeError::Protocol));
        assert_eq!(
            checked_verification(make(PROTOCOL, GameFamily::Bombergirl, VerificationStatus::Denied)),
            Err(RuntimeError::VerificationDenied)
        );
        assert_eq!(
            checked_verification(make(PROTOCOL, GameFamily::Bombergirl, VerificationStatus::ModuleMismatch)),
            Err(RuntimeError::ModuleMismatch)
        );
        assert_eq!(checked_verification(WorkerReceipt::Discovery(receipt())), Err(RuntimeError::Protocol));
        let mut value = serde_json::to_value(make(PROTOCOL, GameFamily::Bombergirl, VerificationStatus::Allowed)).unwrap();
        value["verification"]["token"] = "synthetic-not-a-live-token".into();
        assert!(serde_json::from_value::<WorkerReceipt>(value).is_err());
    }

    #[test]
    fn stage_receipts_never_cross_title_even_when_the_status_is_allowed() {
        for expected in GameFamily::ALL {
            if title(expected).is_err() {
                continue;
            }
            for actual in GameFamily::ALL {
                let receipt = VerificationReceipt { protocol: PROTOCOL, family: actual, status: VerificationStatus::Allowed };
                assert_eq!(
                    checked_stage(&receipt, expected),
                    if !title(expected).unwrap().launch_supported() {
                        Err(RuntimeError::Unsupported)
                    } else if actual == expected {
                        Ok(())
                    } else {
                        Err(RuntimeError::Protocol)
                    }
                );
            }
        }
    }

    #[test]
    fn request_operations_are_fixed_and_verification_input_is_strict() {
        let request = Request {
            protocol: PROTOCOL,
            family: GameFamily::Bombergirl,
            cache_root: "synthetic-cache".into(),
            operation: Operation::Verify(VerificationInput {
                product_version: "QBG:J:A:A:2026091500".into(),
                module_sha256: [42; 32],
                token: LaunchToken::new(Zeroizing::new("synthetic-token".into())).unwrap(),
            }),
        };
        let mut value = serde_json::to_value(&request).unwrap();
        value["operation"]["verify"]["spawn"] = true.into();
        assert!(serde_json::from_value::<Request>(value).is_err());
        let mut value = serde_json::to_value(&request).unwrap();
        value["operation"] = "spawn".into();
        assert!(serde_json::from_value::<Request>(value).is_err());
        let mut value = serde_json::to_value(&request).unwrap();
        value["operation"]["verify"]["token"] = "a".repeat(65).into();
        assert!(serde_json::from_value::<Request>(value).is_err());
        let mut value = serde_json::to_value(&request).unwrap();
        value["operation"] = serde_json::json!({
            "prepare": value["operation"]["verify"].take()
        });
        assert!(serde_json::from_value::<Request>(value).is_ok());
        let mut value = serde_json::to_value(&request).unwrap();
        value["operation"]["prepare"] = value["operation"]["verify"].take();
        value["operation"]["prepare"]["exe"] = "bomber.exe".into();
        assert!(serde_json::from_value::<Request>(value).is_err());
    }

    #[test]
    fn frames_reject_truncation_oversize_and_trailing_confusion() {
        let body = b"{}".to_vec();
        let mut exact = (body.len() as u32).to_le_bytes().to_vec();
        exact.extend_from_slice(&body);
        let mut cursor = std::io::Cursor::new(exact);
        assert_eq!(read_frame(&mut cursor, 64).unwrap().as_slice(), b"{}");
        assert_eq!(cursor.position(), 6);

        let mut short = (8_u32).to_le_bytes().to_vec();
        short.extend_from_slice(b"short");
        assert_eq!(read_frame(&mut std::io::Cursor::new(short), 64).map(|_| ()), Err(RuntimeError::Protocol));
        assert_eq!(read_frame(&mut std::io::Cursor::new([0, 0, 0, 0]), 64).map(|_| ()), Err(RuntimeError::Protocol));
        let mut huge = (65_u32).to_le_bytes().to_vec();
        huge.extend(std::iter::repeat_n(b'a', 65));
        assert_eq!(read_frame(&mut std::io::Cursor::new(huge), 64).map(|_| ()), Err(RuntimeError::Protocol));
        assert_eq!(read_frame(&mut std::io::Cursor::new([1, 0]), 64).map(|_| ()), Err(RuntimeError::Protocol));
    }

    #[test]
    fn receipts_bind_protocol_title_digest_and_exact_https_source() {
        let value = receipt();
        let location = checked_receipt(&value, GameFamily::Bombergirl).unwrap();
        assert_eq!(location.family, GameFamily::Bombergirl);
        assert_eq!(location.url, value.url);
        assert_eq!(location.sha256, "a0".repeat(32));
        for bad in [
            format!("http://{HOST}/ResourceInfo/test.xml"),
            format!("https://{HOST}:444/ResourceInfo/test.xml"),
            format!("https://user@{HOST}/ResourceInfo/test.xml"),
            format!("https://{HOST}.example.test/ResourceInfo/test.xml"),
            format!("https://{HOST}/Installer/test.xml"),
            format!("https://{HOST}/ResourceInfo/test.xml?tk=not-a-token"),
            format!("https://{HOST}/ResourceInfo/test.xml#fragment"),
            format!(" https://{HOST}/ResourceInfo/test.xml"),
        ] {
            let mut value = receipt();
            value.url = bad;
            assert_eq!(checked_receipt(&value, GameFamily::Bombergirl).map(|_| ()), Err(RuntimeError::Protocol));
        }
        let mut value = receipt();
        value.protocol = PROTOCOL + 1;
        assert!(checked_receipt(&value, GameFamily::Bombergirl).is_err());
        value = receipt();
        value.family = GameFamily::Sdvx;
        assert!(checked_receipt(&value, GameFamily::Bombergirl).is_err());
        value = receipt();
        value.sha256 = "AA".repeat(32);
        assert!(checked_receipt(&value, GameFamily::Bombergirl).is_err());
        value.sha256 = "aa".repeat(33);
        assert!(checked_receipt(&value, GameFamily::Bombergirl).is_err());
    }

    #[test]
    fn checked_sdvx_discovery_never_accepts_a_bombergirl_location() {
        let mut value = receipt();
        value.family = GameFamily::Sdvx;
        assert!(checked_receipt(&value, GameFamily::Sdvx).is_err());
        let profile = discovery_profile(GameFamily::Sdvx).unwrap();
        value.url = format!("https://{}/vi/downloads/resource_info/synthetic.xml", profile.host);
        assert_eq!(checked_receipt(&value, GameFamily::Sdvx).unwrap().family, GameFamily::Sdvx);
    }

    #[test]
    fn receipt_fields_are_strict_and_not_an_arbitrary_operation_interface() {
        let mut value = serde_json::to_value(receipt()).unwrap();
        value["token"] = "not-a-token".into();
        assert!(serde_json::from_value::<Receipt>(value).is_err());
        let mut value = serde_json::to_value(Request {
            protocol: PROTOCOL,
            family: GameFamily::Bombergirl,
            cache_root: "cache".into(),
            operation: Operation::Discover,
        })
        .unwrap();
        value["operation"] = "launch".into();
        assert!(serde_json::from_value::<Request>(value).is_err());
    }
}
