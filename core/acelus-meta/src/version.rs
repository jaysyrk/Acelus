use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::arguments::{Argument, ResolvedArguments};
use crate::rules::{evaluate, Environment, Rule};

pub const LEGACY_JVM_ARGUMENTS: &[&str] = &[
    "-Djava.library.path=${natives_directory}",
    "-cp",
    "${classpath}",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseType {
    Release,
    Snapshot,
    OldBeta,
    OldAlpha,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    pub id: String,
    #[serde(rename = "type")]
    pub release_type: ReleaseType,
    pub main_class: String,

    #[serde(default)]
    pub libraries: Vec<Library>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Arguments>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minecraft_arguments: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_index: Option<AssetIndexRef>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<Downloads>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_version: Option<JavaVersion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging: Option<Logging>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compliance_level: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_launcher_version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherits_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Argument>,
    #[serde(default)]
    pub jvm: Vec<Argument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetIndexRef {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_size: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Downloads {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<Artifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<Artifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_mappings: Option<Artifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_mappings: Option<Artifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    pub component: String,
    pub major_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Logging {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<LoggingClient>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingClient {
    pub argument: String,
    pub file: LoggingFile,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingFile {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibraryDownloads {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<Artifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifiers: Option<BTreeMap<String, Artifact>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Extract {
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: LibraryDownloads,
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub natives: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract: Option<Extract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedArtifact {
    pub name: String,
    pub path: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedNative {
    pub artifact: ResolvedArtifact,
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedLibraries {
    pub classpath: Vec<ResolvedArtifact>,
    pub natives: Vec<ResolvedNative>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coordinates {
    pub group: String,
    pub artifact: String,
    pub version: String,
    pub classifier: Option<String>,
    pub extension: String,
}

impl Coordinates {
    pub fn parse(name: &str) -> Option<Self> {
        let (coords, extension) = match name.split_once('@') {
            Some((left, ext)) => (left, ext.to_string()),
            None => (name, "jar".to_string()),
        };

        let mut parts = coords.split(':');
        let group = parts.next()?.to_string();
        let artifact = parts.next()?.to_string();
        let version = parts.next()?.to_string();
        let classifier = parts.next().map(|c| c.to_string());

        if group.is_empty() || artifact.is_empty() || version.is_empty() {
            return None;
        }

        Some(Self {
            group,
            artifact,
            version,
            classifier,
            extension,
        })
    }

    pub fn to_path(&self) -> String {
        let group = self.group.replace('.', "/");
        let suffix = match &self.classifier {
            Some(classifier) => format!("-{classifier}"),
            None => String::new(),
        };
        format!(
            "{group}/{}/{}/{}-{}{suffix}.{}",
            self.artifact, self.version, self.artifact, self.version, self.extension
        )
    }

    pub fn dedup_key(&self) -> String {
        match &self.classifier {
            Some(classifier) => format!("{}:{}:{classifier}", self.group, self.artifact),
            None => format!("{}:{}", self.group, self.artifact),
        }
    }
}

impl Library {
    pub fn is_applicable(&self, env: &Environment) -> bool {
        evaluate(&self.rules, env)
    }

    fn native_classifier(&self, env: &Environment) -> Option<String> {
        let natives = self.natives.as_ref()?;
        let template = natives.get(env.os.as_mojang_str())?;
        Some(template.replace("${arch}", env.arch.native_bits()))
    }

    fn resolve_artifact(&self, artifact: &Artifact) -> Option<ResolvedArtifact> {
        let path = artifact
            .path
            .clone()
            .or_else(|| Coordinates::parse(&self.name).map(|c| c.to_path()))?;
        Some(ResolvedArtifact {
            name: self.name.clone(),
            path,
            sha1: Some(artifact.sha1.clone()),
            size: Some(artifact.size),
            url: artifact.url.clone(),
        })
    }

    fn resolve_from_maven_url(&self) -> Option<ResolvedArtifact> {
        let base = self.url.as_ref()?;
        let path = Coordinates::parse(&self.name)?.to_path();
        let separator = if base.ends_with('/') { "" } else { "/" };
        Some(ResolvedArtifact {
            name: self.name.clone(),
            url: format!("{base}{separator}{path}"),
            path,
            sha1: None,
            size: None,
        })
    }
}

impl VersionJson {
    pub fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    pub fn resolve_libraries(&self, env: &Environment) -> ResolvedLibraries {
        let mut resolved = ResolvedLibraries::default();
        let mut seen = std::collections::HashSet::new();

        for library in &self.libraries {
            if !library.is_applicable(env) {
                continue;
            }

            if let Some(classifier) = library.native_classifier(env) {
                if let Some(artifact) = library
                    .downloads
                    .classifiers
                    .as_ref()
                    .and_then(|c| c.get(&classifier))
                    .and_then(|a| library.resolve_artifact(a))
                {
                    resolved.natives.push(ResolvedNative {
                        artifact,
                        exclude: library
                            .extract
                            .as_ref()
                            .map(|e| e.exclude.clone())
                            .unwrap_or_default(),
                    });
                }
            }

            let classpath_entry = library
                .downloads
                .artifact
                .as_ref()
                .and_then(|a| library.resolve_artifact(a))
                .or_else(|| library.resolve_from_maven_url());

            if let Some(entry) = classpath_entry {
                let key = Coordinates::parse(&library.name)
                    .map(|c| c.dedup_key())
                    .unwrap_or_else(|| library.name.clone());
                if seen.insert(key) {
                    resolved.classpath.push(entry);
                }
            }
        }

        resolved
    }

    pub fn resolve_arguments(&self, env: &Environment) -> ResolvedArguments {
        match &self.arguments {
            Some(arguments) => ResolvedArguments {
                game: crate::arguments::resolve(&arguments.game, env),
                jvm: crate::arguments::resolve(&arguments.jvm, env),
            },
            None => ResolvedArguments {
                game: self
                    .minecraft_arguments
                    .as_deref()
                    .unwrap_or_default()
                    .split_whitespace()
                    .map(str::to_string)
                    .collect(),
                jvm: LEGACY_JVM_ARGUMENTS.iter().map(|s| s.to_string()).collect(),
            },
        }
    }
}
