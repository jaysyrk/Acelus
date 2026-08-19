use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::rules::{evaluate, Environment, Rule};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Argument {
    Literal(String),
    Conditional {
        #[serde(default)]
        rules: Vec<Rule>,
        value: Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    One(String),
    Many(Vec<String>),
}

impl Value {
    pub fn into_vec(self) -> Vec<String> {
        match self {
            Value::One(value) => vec![value],
            Value::Many(values) => values,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedArguments {
    pub game: Vec<String>,
    pub jvm: Vec<String>,
}

pub fn resolve(arguments: &[Argument], env: &Environment) -> Vec<String> {
    let mut resolved = Vec::new();
    for argument in arguments {
        match argument {
            Argument::Literal(value) => resolved.push(value.clone()),
            Argument::Conditional { rules, value } => {
                if evaluate(rules, env) {
                    resolved.extend(value.clone().into_vec());
                }
            }
        }
    }
    resolved
}

#[derive(Debug, thiserror::Error)]
#[error("unresolved placeholder{} in launch arguments: {}", if .names.len() == 1 { "" } else { "s" }, .names.join(", "))]
pub struct UnresolvedPlaceholders {
    pub names: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Substitutions(BTreeMap<String, String>);

impl Substitutions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.0.insert(name.into(), value.into());
        self
    }

    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.0.insert(name.into(), value.into());
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }
}

pub fn substitute(
    arguments: &[String],
    substitutions: &Substitutions,
) -> Result<Vec<String>, UnresolvedPlaceholders> {
    let mut output = Vec::with_capacity(arguments.len());
    let mut missing = Vec::new();

    for argument in arguments {
        let mut result = String::with_capacity(argument.len());
        let mut rest = argument.as_str();

        while let Some(start) = rest.find("${") {
            result.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            match after.find('}') {
                Some(end) => {
                    let name = &after[..end];
                    match substitutions.get(name) {
                        Some(value) => result.push_str(value),
                        None => {
                            if !missing.iter().any(|m| m == name) {
                                missing.push(name.to_string());
                            }
                            result.push_str("${");
                            result.push_str(name);
                            result.push('}');
                        }
                    }
                    rest = &after[end + 1..];
                }
                None => {
                    result.push_str("${");
                    rest = after;
                }
            }
        }

        result.push_str(rest);
        output.push(result);
    }

    if missing.is_empty() {
        Ok(output)
    } else {
        Err(UnresolvedPlaceholders { names: missing })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{Arch, Features, Os};

    fn args(json: &str) -> Vec<Argument> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn literals_pass_through_in_order() {
        let resolved = resolve(
            &args(r#"["--username","${auth_player_name}"]"#),
            &Environment::new(Os::Linux, Arch::X86_64),
        );
        assert_eq!(resolved, vec!["--username", "${auth_player_name}"]);
    }

    #[test]
    fn conditional_arguments_follow_their_rules() {
        let arguments = args(
            r#"[{"rules":[{"action":"allow","os":{"name":"osx"}}],"value":"-XstartOnFirstThread"}]"#,
        );
        assert!(resolve(&arguments, &Environment::new(Os::Linux, Arch::X86_64)).is_empty());
        assert_eq!(
            resolve(&arguments, &Environment::new(Os::MacOs, Arch::Arm64)),
            vec!["-XstartOnFirstThread"]
        );
    }

    #[test]
    fn a_conditional_may_contribute_several_arguments() {
        let arguments = args(
            r#"[{"rules":[{"action":"allow","features":{"has_custom_resolution":true}}],"value":["--width","${resolution_width}"]}]"#,
        );
        let env = Environment::new(Os::Linux, Arch::X86_64)
            .with_features(Features::new().with("has_custom_resolution", true));
        assert_eq!(
            resolve(&arguments, &env),
            vec!["--width", "${resolution_width}"]
        );
    }

    #[test]
    fn substitution_replaces_known_placeholders() {
        let subs = Substitutions::new()
            .set("auth_player_name", "Notch")
            .set("version_name", "26.2");
        let out = substitute(
            &[
                "--username".into(),
                "${auth_player_name}".into(),
                "${version_name}".into(),
            ],
            &subs,
        )
        .unwrap();
        assert_eq!(out, vec!["--username", "Notch", "26.2"]);
    }

    #[test]
    fn substitution_handles_several_placeholders_in_one_argument() {
        let subs = Substitutions::new()
            .set("natives_directory", "/n")
            .set("a", "x");
        let out = substitute(&["-Dp=${natives_directory}/java/${a}".into()], &subs).unwrap();
        assert_eq!(out, vec!["-Dp=/n/java/x"]);
    }

    #[test]
    fn unresolved_placeholders_are_reported_rather_than_passed_to_the_game() {
        let error = substitute(
            &["${auth_access_token}".into(), "${auth_uuid}".into()],
            &Substitutions::new(),
        )
        .unwrap_err();
        assert_eq!(error.names, vec!["auth_access_token", "auth_uuid"]);
    }

    #[test]
    fn a_placeholder_is_reported_once_however_often_it_appears() {
        let error = substitute(&["${x}".into(), "${x}".into()], &Substitutions::new()).unwrap_err();
        assert_eq!(error.names, vec!["x"]);
    }

    #[test]
    fn an_unterminated_placeholder_is_left_alone() {
        let out = substitute(&["-Dweird=${unclosed".into()], &Substitutions::new()).unwrap();
        assert_eq!(out, vec!["-Dweird=${unclosed"]);
    }

    #[test]
    fn text_without_placeholders_is_untouched() {
        let out = substitute(&["-Xmx2G".into()], &Substitutions::new()).unwrap();
        assert_eq!(out, vec!["-Xmx2G"]);
    }
}
