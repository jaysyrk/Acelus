use std::fmt;
use std::str::FromStr;

pub const HASH_LEN: usize = 32;
pub const HASH_HEX_LEN: usize = HASH_LEN * 2;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hash([u8; HASH_LEN]);

impl Hash {
    pub fn from_bytes(bytes: [u8; HASH_LEN]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; HASH_LEN] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        hex::encode(self.0)
    }
}

impl From<blake3::Hash> for Hash {
    fn from(value: blake3::Hash) -> Self {
        Self(*value.as_bytes())
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}

impl fmt::Debug for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hash({})", hex::encode(self.0))
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid blake3 hash: expected {HASH_HEX_LEN} hex characters")]
pub struct ParseHashError;

impl FromStr for Hash {
    type Err = ParseHashError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != HASH_HEX_LEN {
            return Err(ParseHashError);
        }
        let mut bytes = [0u8; HASH_LEN];
        hex::decode_to_slice(s, &mut bytes).map_err(|_| ParseHashError)?;
        Ok(Self(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let hash = Hash::from(blake3::hash(b"acelus"));
        let parsed: Hash = hash.to_hex().parse().unwrap();
        assert_eq!(hash, parsed);
    }

    #[test]
    fn rejects_wrong_length() {
        assert!("abcd".parse::<Hash>().is_err());
    }

    #[test]
    fn rejects_non_hex() {
        let bad = "z".repeat(HASH_HEX_LEN);
        assert!(bad.parse::<Hash>().is_err());
    }
}
