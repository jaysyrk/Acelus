use serde::{Deserialize, Serialize};

pub const LOCK_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Client,
    Library,
    Native,
    Asset,
    LoggingConfig,
    Mod,
    Runtime,
    Override,
}

impl Role {
    pub fn is_classpath(self) -> bool {
        matches!(self, Role::Client | Role::Library | Role::Mod)
    }

    pub fn is_shared(self) -> bool {
        !matches!(self, Role::Override)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoaderKind {
    Fabric,
    Quilt,
    NeoForge,
    Forge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JavaSource {
    Mojang,
    Adoptium,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lockfile {
    pub lock_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_by: Option<String>,
    pub minecraft: LockMinecraft,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loader: Option<LockLoader>,
    pub java: LockJava,
    pub arguments: LockArguments,
    pub artifacts: Vec<LockArtifact>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockArguments {
    pub game: Vec<String>,
    pub jvm: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockMinecraft {
    pub id: String,
    #[serde(rename = "type")]
    pub release_type: String,
    pub manifest_sha1: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_index_id: Option<String>,
    pub main_class: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compliance_level: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockLoader {
    pub kind: LoaderKind,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intermediary_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockJava {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    pub major_version: u32,
    pub source: JavaSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockArtifact {
    pub path: String,
    pub role: Role,
    pub size: u64,
    pub blake3: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha512: Option<String>,
    pub origin: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extract: Option<LockExtract>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockExtract {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the lockfile could not be read")]
    Io(#[from] std::io::Error),

    #[error("the lockfile is not valid JSON")]
    Json(#[from] serde_json::Error),

    #[error("this lockfile was written by a newer version of Acelus (format {found}, expected {LOCK_VERSION})")]
    UnsupportedVersion { found: u32 },
}

impl Lockfile {
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let lockfile: Lockfile = serde_json::from_slice(bytes)?;
        if lockfile.lock_version != LOCK_VERSION {
            return Err(Error::UnsupportedVersion {
                found: lockfile.lock_version,
            });
        }
        Ok(lockfile)
    }

    pub fn to_json(&self) -> Result<String, Error> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn total_size(&self) -> u64 {
        self.artifacts.iter().map(|a| a.size).sum()
    }

    pub fn artifacts_with_role(&self, role: Role) -> impl Iterator<Item = &LockArtifact> {
        self.artifacts.iter().filter(move |a| a.role == role)
    }

    pub fn classpath(&self) -> Vec<&LockArtifact> {
        let mut entries: Vec<&LockArtifact> = self
            .artifacts
            .iter()
            .filter(|a| a.role.is_classpath())
            .collect();
        entries.sort_by(|a, b| a.role.cmp_order().cmp(&b.role.cmp_order()));
        entries
    }
}

impl Role {
    fn cmp_order(self) -> u8 {
        match self {
            Role::Mod => 0,
            Role::Library => 1,
            Role::Client => 2,
            _ => 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(path: &str, role: Role) -> LockArtifact {
        LockArtifact {
            path: path.into(),
            role,
            size: 10,
            blake3: "a".repeat(64),
            sha1: Some("b".repeat(40)),
            sha512: None,
            origin: vec!["https://libraries.minecraft.net/x.jar".into()],
            extract: None,
        }
    }

    fn lockfile(artifacts: Vec<LockArtifact>) -> Lockfile {
        Lockfile {
            lock_version: LOCK_VERSION,
            generated_by: Some("acelus 0.1.0".into()),
            minecraft: LockMinecraft {
                id: "26.2".into(),
                release_type: "release".into(),
                manifest_sha1: "c".repeat(40),
                asset_index_id: Some("32".into()),
                main_class: "net.minecraft.client.main.Main".into(),
                compliance_level: Some(1),
            },
            loader: None,
            java: LockJava {
                component: Some("java-runtime-epsilon".into()),
                major_version: 25,
                source: JavaSource::Mojang,
            },
            arguments: LockArguments {
                game: vec!["--username".into(), "${auth_player_name}".into()],
                jvm: vec!["-cp".into(), "${classpath}".into()],
            },
            artifacts,
        }
    }

    #[test]
    fn a_lockfile_round_trips_through_json() {
        let original = lockfile(vec![artifact("client.jar", Role::Client)]);
        let parsed = Lockfile::parse(original.to_json().unwrap().as_bytes()).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn a_future_lock_version_is_refused_rather_than_misread() {
        let mut future = lockfile(Vec::new());
        future.lock_version = LOCK_VERSION + 1;
        let json = serde_json::to_string(&future).unwrap();

        assert!(matches!(
            Lockfile::parse(json.as_bytes()),
            Err(Error::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn roles_serialise_in_the_spelling_the_schema_declares() {
        assert_eq!(
            serde_json::to_string(&Role::LoggingConfig).unwrap(),
            "\"logging-config\""
        );
        assert_eq!(serde_json::to_string(&Role::Client).unwrap(), "\"client\"");
        assert_eq!(
            serde_json::to_string(&LoaderKind::NeoForge).unwrap(),
            "\"neoforge\""
        );
    }

    #[test]
    fn only_runnable_roles_reach_the_classpath() {
        assert!(Role::Client.is_classpath());
        assert!(Role::Library.is_classpath());
        assert!(Role::Mod.is_classpath());

        assert!(!Role::Native.is_classpath());
        assert!(!Role::Asset.is_classpath());
        assert!(!Role::Runtime.is_classpath());
        assert!(!Role::LoggingConfig.is_classpath());
    }

    #[test]
    fn player_editable_files_are_never_shared_between_instances() {
        assert!(
            !Role::Override.is_shared(),
            "an override is edited in place and must be a private copy"
        );
        assert!(Role::Library.is_shared());
        assert!(Role::Asset.is_shared());
    }

    #[test]
    fn mods_precede_libraries_on_the_classpath() {
        let lock = lockfile(vec![
            artifact("client.jar", Role::Client),
            artifact("libraries/a.jar", Role::Library),
            artifact("minecraft/mods/m.jar", Role::Mod),
            artifact("natives/n.jar", Role::Native),
        ]);

        let classpath: Vec<&str> = lock.classpath().iter().map(|a| a.path.as_str()).collect();
        assert_eq!(
            classpath,
            vec!["minecraft/mods/m.jar", "libraries/a.jar", "client.jar"]
        );
    }

    #[test]
    fn sizes_are_totalled_across_every_artifact() {
        let lock = lockfile(vec![
            artifact("a", Role::Library),
            artifact("b", Role::Asset),
        ]);
        assert_eq!(lock.total_size(), 20);
        assert_eq!(lock.artifacts_with_role(Role::Asset).count(), 1);
    }
}
