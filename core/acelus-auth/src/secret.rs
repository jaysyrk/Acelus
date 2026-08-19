use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([redacted])")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "eyJhbGciOiJIUzI1NiJ9.super-secret-token-material";

    #[test]
    fn debug_output_never_contains_the_secret() {
        let secret = Secret::new(TOKEN);
        let rendered = format!("{secret:?}");
        assert_eq!(rendered, "Secret([redacted])");
        assert!(!rendered.contains("super-secret"));
    }

    #[test]
    fn display_output_never_contains_the_secret() {
        let secret = Secret::new(TOKEN);
        assert_eq!(format!("{secret}"), "[redacted]");
    }

    #[test]
    fn a_secret_nested_in_a_struct_stays_redacted() {
        #[derive(Debug)]
        struct Session {
            name: String,
            token: Secret,
        }

        let session = Session {
            name: "Notch".into(),
            token: Secret::new(TOKEN),
        };
        let rendered = format!("{session:?}");

        assert_eq!(session.name, "Notch");
        assert_eq!(session.token.expose(), TOKEN);
        assert!(rendered.contains("Notch"));
        assert!(!rendered.contains("super-secret"));
    }

    #[test]
    fn the_value_is_still_reachable_when_deliberately_exposed() {
        assert_eq!(Secret::new(TOKEN).expose(), TOKEN);
    }
}
