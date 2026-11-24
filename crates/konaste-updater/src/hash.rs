use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt};

/// Hash of an in-memory buffer. Used for the official manifest bytes.
pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

/// Streaming hash of an already-open file. The caller has checked the length.
pub(crate) async fn hash_file<R: AsyncRead + Unpin>(reader: &mut R) -> std::io::Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0_u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().into())
}

/// Incremental hasher used while a download is appended.
pub(crate) struct StreamingHash {
    inner: Sha256,
    bytes: u64,
}

impl StreamingHash {
    pub(crate) fn new() -> Self {
        Self { inner: Sha256::new(), bytes: 0 }
    }

    pub(crate) fn update(&mut self, chunk: &[u8]) {
        self.inner.update(chunk);
        self.bytes += chunk.len() as u64;
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.bytes
    }

    pub(crate) fn finish(self) -> [u8; 32] {
        self.inner.finalize().into()
    }
}
