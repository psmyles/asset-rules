//! Content digests, written as `sha256:<hex>` in results and cache keys.

use sha2::{Digest, Sha256};

/// The SHA-256 of `bytes`, as `sha256:` followed by 64 lowercase hex digits.
pub(crate) fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{}", sha256_hex(bytes))
}

/// The SHA-256 of `bytes`, as 64 lowercase hex digits.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for &byte in Sha256::digest(bytes).iter() {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0xf)]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::sha256;

    #[test]
    fn matches_the_standard_test_vector() {
        assert_eq!(
            sha256(b"abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
