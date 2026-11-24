use std::path::PathBuf;

use thiserror::Error;

/// Failures a caller can distinguish without inspecting updater internals.
///
/// Download failures ([`UpdateError::Download`]) are resumable: staged bytes
/// and the journal are left in place. Filesystem failures during replacement
/// are recoverable from the journal; they do not discard a verified stage.
#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("installation root is not a canonical directory: {0}")]
    Root(String),

    #[error("manifest is not usable: {0}")]
    Manifest(String),

    #[error("refusing path `{path}`: {reason}")]
    PathRejected { path: String, reason: String },

    #[error("download policy rejected `{url}`: {reason}")]
    Policy { url: String, reason: String },

    #[error("not enough free space on the installation volume: need {needed} bytes, available {available}")]
    DiskSpace { needed: u64, available: u64 },

    #[error("session is not ready for that call: {0}")]
    State(String),

    /// An interrupted Installing/Publishing journal does not match the source
    /// just presented. The journal and official stage were left untouched.
    /// Call `recover` on the source that matches the journal, not a new one.
    #[error("interrupted replacement does not match the fresh source and was not overwritten")]
    RecoveryRequired,

    /// The on-disk journal is a different plan from the source bound by `prepare`.
    /// `run` and `recover` refuse it until the same source is bound again.
    #[error("bound source does not match the on-disk journal")]
    PlanChanged,

    /// The caller's guard refused a destination replacement, official publication,
    /// or recovery mutation. Journal and stage were left as they were.
    #[error("operation rejected by guard: {0}")]
    OperationRejected(String),

    #[error("download of `{relative}` is incomplete and can be resumed: {message}")]
    Download { relative: String, message: String },

    #[error("staged file `{relative}` failed verification and was discarded: {message}")]
    Staging { relative: String, message: String },

    #[error("installation change for `{path}` failed; journal remains recoverable: {message}")]
    Install { path: PathBuf, message: String },

    #[error("update journal is damaged: {0}")]
    Journal(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

impl UpdateError {
    pub(crate) fn manifest(message: impl Into<String>) -> Self {
        Self::Manifest(message.into())
    }

    pub(crate) fn path(path: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::PathRejected { path: path.into(), reason: reason.into() }
    }

    pub(crate) fn policy(url: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Policy { url: url.into(), reason: reason.into() }
    }

    pub(crate) fn state(message: impl Into<String>) -> Self {
        Self::State(message.into())
    }

    /// Guard refused the mutation. `reason` is the caller's message.
    pub fn operation_rejected(reason: impl Into<String>) -> Self {
        Self::OperationRejected(reason.into())
    }

    pub(crate) fn download(relative: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Download { relative: relative.into(), message: message.into() }
    }

    pub(crate) fn journal(message: impl Into<String>) -> Self {
        Self::Journal(message.into())
    }

    /// True when another `run` can continue without a new `prepare`.
    pub fn is_resumable(&self) -> bool {
        matches!(self, Self::Download { .. })
    }
}
