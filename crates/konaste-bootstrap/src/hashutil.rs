use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::MAX_PACKAGE_BYTES;
use crate::error::BootstrapError;

pub(crate) fn sha256_file(path: &Path) -> Result<String, BootstrapError> {
    crate::cache::reject_reparse_path(path)?;
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > MAX_PACKAGE_BYTES {
            return Err(BootstrapError::fetch(format!("file exceeds {MAX_PACKAGE_BYTES} byte package budget")));
        }
        hasher.update(&buf[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub(crate) fn sha256_reader(mut reader: impl Read, limit: u64) -> io::Result<(Vec<u8>, String)> {
    let mut hasher = Sha256::new();
    let mut out = Vec::new();
    let mut buf = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let read = reader.read(&mut buf)?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > limit {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("stream exceeds {limit} byte budget")));
        }
        hasher.update(&buf[..read]);
        out.extend_from_slice(&buf[..read]);
    }
    Ok((out, hex::encode(hasher.finalize())))
}
