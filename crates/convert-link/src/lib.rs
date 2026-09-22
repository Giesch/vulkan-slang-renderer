//! Wind Waker Link asset conversion, shared by the `convert_link_animations`
//! binary and its integration tests.

pub mod animation;

use sha2::{Digest, Sha256};

/// Lowercase hex SHA-256, matching the extraction script's `hashlib` output
/// and the committed `scripts/*.sha256` goldens.
pub fn sha256_hex(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
