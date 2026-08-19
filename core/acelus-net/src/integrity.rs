use std::fmt;

use sha1::Digest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    Sha1,
    Sha512,
}

impl fmt::Display for Algorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Algorithm::Sha1 => "sha1",
            Algorithm::Sha512 => "sha512",
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Integrity {
    pub size: Option<u64>,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
}

impl Integrity {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn sha1(digest: impl Into<String>) -> Self {
        Self {
            sha1: Some(digest.into().to_ascii_lowercase()),
            ..Self::default()
        }
    }

    pub fn sha512(digest: impl Into<String>) -> Self {
        Self {
            sha512: Some(digest.into().to_ascii_lowercase()),
            ..Self::default()
        }
    }

    pub fn with_size(mut self, size: u64) -> Self {
        self.size = Some(size);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.size.is_none() && self.sha1.is_none() && self.sha512.is_none()
    }

    pub fn verifier(&self) -> Verifier {
        Verifier {
            expected: self.clone(),
            sha1: self.sha1.as_ref().map(|_| sha1::Sha1::new()),
            sha512: self.sha512.as_ref().map(|_| sha2::Sha512::new()),
            observed: 0,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IntegrityError {
    #[error("expected {expected} bytes but received {actual}")]
    Size { expected: u64, actual: u64 },

    #[error("{algorithm} mismatch: expected {expected}, computed {actual}")]
    Digest {
        algorithm: Algorithm,
        expected: String,
        actual: String,
    },
}

pub struct Verifier {
    expected: Integrity,
    sha1: Option<sha1::Sha1>,
    sha512: Option<sha2::Sha512>,
    observed: u64,
}

impl Verifier {
    pub fn update(&mut self, chunk: &[u8]) {
        if let Some(hasher) = self.sha1.as_mut() {
            hasher.update(chunk);
        }
        if let Some(hasher) = self.sha512.as_mut() {
            hasher.update(chunk);
        }
        self.observed += chunk.len() as u64;
    }

    pub fn observed(&self) -> u64 {
        self.observed
    }

    pub fn finish(self) -> Result<u64, IntegrityError> {
        if let Some(expected) = self.expected.size {
            if expected != self.observed {
                return Err(IntegrityError::Size {
                    expected,
                    actual: self.observed,
                });
            }
        }

        if let (Some(hasher), Some(expected)) = (self.sha1, self.expected.sha1.as_ref()) {
            compare(Algorithm::Sha1, expected, &hex::encode(hasher.finalize()))?;
        }

        if let (Some(hasher), Some(expected)) = (self.sha512, self.expected.sha512.as_ref()) {
            compare(Algorithm::Sha512, expected, &hex::encode(hasher.finalize()))?;
        }

        Ok(self.observed)
    }
}

fn compare(algorithm: Algorithm, expected: &str, actual: &str) -> Result<(), IntegrityError> {
    if expected.eq_ignore_ascii_case(actual) {
        Ok(())
    } else {
        Err(IntegrityError::Digest {
            algorithm,
            expected: expected.to_owned(),
            actual: actual.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY_SHA1: &str = "da39a3ee5e6b4b0d3255bfef95601890afd80709";
    const ABC_SHA1: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";

    #[test]
    fn accepts_matching_digest_and_size() {
        let mut verifier = Integrity::sha1(ABC_SHA1).with_size(3).verifier();
        verifier.update(b"abc");
        assert_eq!(verifier.finish().unwrap(), 3);
    }

    #[test]
    fn accepts_digest_supplied_in_upper_case() {
        let mut verifier = Integrity::sha1(ABC_SHA1.to_uppercase()).verifier();
        verifier.update(b"abc");
        assert!(verifier.finish().is_ok());
    }

    #[test]
    fn rejects_wrong_digest() {
        let mut verifier = Integrity::sha1(EMPTY_SHA1).verifier();
        verifier.update(b"abc");
        assert!(matches!(
            verifier.finish(),
            Err(IntegrityError::Digest {
                algorithm: Algorithm::Sha1,
                ..
            })
        ));
    }

    #[test]
    fn rejects_wrong_size_before_digest() {
        let mut verifier = Integrity::sha1(EMPTY_SHA1).with_size(99).verifier();
        verifier.update(b"abc");
        assert!(matches!(
            verifier.finish(),
            Err(IntegrityError::Size {
                expected: 99,
                actual: 3
            })
        ));
    }

    #[test]
    fn chunked_updates_match_a_single_update() {
        let mut chunked = Integrity::sha1(ABC_SHA1).verifier();
        chunked.update(b"a");
        chunked.update(b"b");
        chunked.update(b"c");
        assert!(chunked.finish().is_ok());
    }

    #[test]
    fn empty_integrity_accepts_anything() {
        let mut verifier = Integrity::none().verifier();
        verifier.update(b"whatever");
        assert_eq!(verifier.finish().unwrap(), 8);
    }

    #[test]
    fn sha512_is_checked_when_supplied() {
        let expected = hex::encode(sha2::Sha512::digest(b"modrinth"));
        let mut verifier = Integrity::sha512(&expected).verifier();
        verifier.update(b"modrinth");
        assert!(verifier.finish().is_ok());

        let mut wrong = Integrity::sha512(expected).verifier();
        wrong.update(b"curseforge");
        assert!(matches!(
            wrong.finish(),
            Err(IntegrityError::Digest {
                algorithm: Algorithm::Sha512,
                ..
            })
        ));
    }
}
