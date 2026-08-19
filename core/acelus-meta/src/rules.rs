use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Windows,
    #[serde(rename = "osx")]
    MacOs,
    Linux,
}

impl Os {
    pub const fn current() -> Self {
        if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }

    pub const fn as_mojang_str(self) -> &'static str {
        match self {
            Os::Windows => "windows",
            Os::MacOs => "osx",
            Os::Linux => "linux",
        }
    }

    pub const fn classifier(self) -> &'static str {
        match self {
            Os::Windows => "natives-windows",
            Os::MacOs => "natives-osx",
            Os::Linux => "natives-linux",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Arch {
    X86,
    X86_64,
    Arm32,
    Arm64,
}

impl Arch {
    pub fn current() -> Self {
        match std::env::consts::ARCH {
            "x86" => Arch::X86,
            "aarch64" => Arch::Arm64,
            "arm" => Arch::Arm32,
            _ => Arch::X86_64,
        }
    }

    pub const fn as_mojang_str(self) -> &'static str {
        match self {
            Arch::X86 => "x86",
            Arch::X86_64 => "x86_64",
            Arch::Arm32 => "arm32",
            Arch::Arm64 => "arm64",
        }
    }

    pub const fn native_bits(self) -> &'static str {
        match self {
            Arch::X86 | Arch::Arm32 => "32",
            Arch::X86_64 | Arch::Arm64 => "64",
        }
    }

    fn matches(self, declared: &str) -> bool {
        match declared {
            "x86" => self == Arch::X86,
            "x86_64" | "amd64" => self == Arch::X86_64,
            "arm64" | "aarch64" => self == Arch::Arm64,
            "arm32" | "arm" => self == Arch::Arm32,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Features(BTreeMap<String, bool>);

impl Features {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, name: impl Into<String>, enabled: bool) -> Self {
        self.0.insert(name.into(), enabled);
        self
    }

    pub fn is_enabled(&self, name: &str) -> bool {
        self.0.get(name).copied().unwrap_or(false)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    pub os: Os,
    pub os_version: String,
    pub arch: Arch,
    pub features: Features,
}

impl Environment {
    pub fn current() -> Self {
        Self {
            os: Os::current(),
            os_version: os_version(),
            arch: Arch::current(),
            features: Features::new(),
        }
    }

    pub fn new(os: Os, arch: Arch) -> Self {
        Self {
            os,
            os_version: String::new(),
            arch,
            features: Features::new(),
        }
    }

    pub fn with_os_version(mut self, version: impl Into<String>) -> Self {
        self.os_version = version.into();
        self
    }

    pub fn with_features(mut self, features: Features) -> Self {
        self.features = features;
        self
    }
}

#[cfg(target_os = "windows")]
fn os_version() -> String {
    std::env::var("OS_VERSION").unwrap_or_default()
}

#[cfg(not(target_os = "windows"))]
fn os_version() -> String {
    String::new()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Allow,
    Disallow,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OsCondition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub action: Action,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<OsCondition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<BTreeMap<String, bool>>,
}

impl Rule {
    pub fn matches(&self, env: &Environment) -> bool {
        if let Some(os) = &self.os {
            if let Some(name) = &os.name {
                if name != env.os.as_mojang_str() {
                    return false;
                }
            }
            if let Some(arch) = &os.arch {
                if !env.arch.matches(arch) {
                    return false;
                }
            }
            if let Some(pattern) = &os.version {
                match regex::Regex::new(pattern) {
                    Ok(regex) if regex.is_match(&env.os_version) => {}
                    Ok(_) => return false,
                    Err(error) => {
                        tracing::warn!(%pattern, %error, "ignoring unparseable os version rule");
                        return false;
                    }
                }
            }
        }

        if let Some(features) = &self.features {
            for (name, expected) in features {
                if env.features.is_enabled(name) != *expected {
                    return false;
                }
            }
        }

        true
    }
}

pub fn evaluate(rules: &[Rule], env: &Environment) -> bool {
    if rules.is_empty() {
        return true;
    }

    let mut allowed = false;
    for rule in rules {
        if rule.matches(env) {
            allowed = rule.action == Action::Allow;
        }
    }
    allowed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(json: &str) -> Rule {
        serde_json::from_str(json).unwrap()
    }

    fn linux() -> Environment {
        Environment::new(Os::Linux, Arch::X86_64)
    }

    fn macos() -> Environment {
        Environment::new(Os::MacOs, Arch::Arm64)
    }

    #[test]
    fn no_rules_means_allowed() {
        assert!(evaluate(&[], &linux()));
    }

    #[test]
    fn rules_default_to_denied_when_nothing_matches() {
        let rules = vec![rule(r#"{"action":"allow","os":{"name":"osx"}}"#)];
        assert!(!evaluate(&rules, &linux()));
        assert!(evaluate(&rules, &macos()));
    }

    #[test]
    fn a_later_disallow_overrides_an_earlier_blanket_allow() {
        let rules = vec![
            rule(r#"{"action":"allow"}"#),
            rule(r#"{"action":"disallow","os":{"name":"osx"}}"#),
        ];
        assert!(evaluate(&rules, &linux()));
        assert!(!evaluate(&rules, &macos()));
    }

    #[test]
    fn architecture_conditions_are_honoured() {
        let rules = vec![rule(r#"{"action":"allow","os":{"arch":"x86"}}"#)];
        assert!(evaluate(&rules, &Environment::new(Os::Windows, Arch::X86)));
        assert!(!evaluate(
            &rules,
            &Environment::new(Os::Windows, Arch::X86_64)
        ));
    }

    #[test]
    fn architecture_aliases_resolve_to_the_same_target() {
        let arm64 = Environment::new(Os::MacOs, Arch::Arm64);
        assert!(evaluate(
            &[rule(r#"{"action":"allow","os":{"arch":"arm64"}}"#)],
            &arm64
        ));
        assert!(evaluate(
            &[rule(r#"{"action":"allow","os":{"arch":"aarch64"}}"#)],
            &arm64
        ));
    }

    #[test]
    fn os_version_is_matched_as_a_regular_expression() {
        let rules = vec![rule(
            r#"{"action":"allow","os":{"name":"windows","version":"^10\\."}}"#,
        )];
        let win10 = Environment::new(Os::Windows, Arch::X86_64).with_os_version("10.0");
        let win7 = Environment::new(Os::Windows, Arch::X86_64).with_os_version("6.1");
        assert!(evaluate(&rules, &win10));
        assert!(!evaluate(&rules, &win7));
    }

    #[test]
    fn an_unparseable_version_pattern_denies_rather_than_panics() {
        let rules = vec![rule(r#"{"action":"allow","os":{"version":"["}}"#)];
        assert!(!evaluate(&rules, &linux()));
    }

    #[test]
    fn feature_conditions_require_an_exact_match() {
        let rules = vec![rule(
            r#"{"action":"allow","features":{"is_demo_user":true}}"#,
        )];
        assert!(!evaluate(&rules, &linux()));

        let demo = linux().with_features(Features::new().with("is_demo_user", true));
        assert!(evaluate(&rules, &demo));
    }

    #[test]
    fn a_feature_expected_to_be_false_matches_when_absent() {
        let rules = vec![rule(
            r#"{"action":"allow","features":{"is_demo_user":false}}"#,
        )];
        assert!(evaluate(&rules, &linux()));
    }

    #[test]
    fn every_condition_in_a_rule_must_hold() {
        let rules = vec![rule(
            r#"{"action":"allow","os":{"name":"linux","arch":"x86_64"},"features":{"is_demo_user":true}}"#,
        )];
        assert!(!evaluate(&rules, &linux()));

        let demo = linux().with_features(Features::new().with("is_demo_user", true));
        assert!(evaluate(&rules, &demo));
    }
}
