use thiserror::Error;

/// Failures a caller can distinguish without reading bootstrap internals.
#[derive(Debug, Error)]
pub enum BootstrapError {
    /// `acquire` was called with `consent == false`. Nothing was downloaded.
    #[error("end-user consent to the official download terms is required before this package can be fetched")]
    ConsentRequired,

    /// Not Windows x64. The recorded packages are 64-bit Windows binaries.
    #[error("official runtime bootstrap is supported only on Windows x64")]
    UnsupportedPlatform,

    /// The downloaded MSI does not match the recorded digest. Not accepted.
    #[error("official package digest changed (expected {expected}, got {actual}); refusing because the launcher ABI is pinned")]
    PackageChanged { expected: String, actual: String },

    /// URL, redirect, size, or timeout rejected the transfer.
    #[error("download rejected: {0}")]
    Fetch(String),

    /// Authenticode check failed, or the platform could not perform it.
    #[error("package signature rejected: {0}")]
    Signature(String),

    /// The MSI did not contain exactly the two expected embedded DLLs.
    #[error("package layout is not the recorded runtime pair: {0}")]
    PackageLayout(String),

    /// A file or cabinet name was unsafe, ambiguous, or not the exact target.
    #[error("refusing archive name `{name}`: {reason}")]
    NameRejected { name: String, reason: String },

    /// Cache root or a published pair failed validation.
    #[error("cache rejected: {0}")]
    Cache(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl BootstrapError {
    pub(crate) fn fetch(message: impl Into<String>) -> Self {
        Self::Fetch(message.into())
    }

    pub(crate) fn signature(message: impl Into<String>) -> Self {
        Self::Signature(message.into())
    }

    pub(crate) fn layout(message: impl Into<String>) -> Self {
        Self::PackageLayout(message.into())
    }

    pub(crate) fn name(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::NameRejected { name: name.into(), reason: reason.into() }
    }

    pub(crate) fn cache(message: impl Into<String>) -> Self {
        Self::Cache(message.into())
    }
}
