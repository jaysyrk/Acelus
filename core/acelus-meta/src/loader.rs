use semver::Version;
use serde::{Deserialize, Serialize};

use crate::version::Coordinates;

pub const FABRIC_META: &str = "https://meta.fabricmc.net/v2";
pub const QUILT_META: &str = "https://meta.quiltmc.org/v3";
pub const FABRIC_MAVEN: &str = "https://maven.fabricmc.net/";
pub const QUILT_MAVEN: &str = "https://maven.quiltmc.org/repository/release/";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoaderKind {
    Fabric,
    Quilt,
}

impl LoaderKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            LoaderKind::Fabric => "fabric",
            LoaderKind::Quilt => "quilt",
        }
    }

    pub const fn meta_base(self) -> &'static str {
        match self {
            LoaderKind::Fabric => FABRIC_META,
            LoaderKind::Quilt => QUILT_META,
        }
    }

    pub const fn maven_base(self) -> &'static str {
        match self {
            LoaderKind::Fabric => FABRIC_MAVEN,
            LoaderKind::Quilt => QUILT_MAVEN,
        }
    }

    pub fn versions_url(self, game_version: &str) -> String {
        format!("{}/versions/loader/{game_version}", self.meta_base())
    }

    /// Whether the digests this loader publishes alongside its metadata describe the
    /// artifacts its Maven repository actually serves. Quilt's do not: for every version
    /// measured, the sha1, sha256 and sha512 in `meta.quiltmc.org` match neither the jar,
    /// the pom nor the sources jar at the same coordinate, while the repository's own
    /// `.sha1` sidecar matches the served bytes exactly. Its declared sizes are correct.
    pub const fn publishes_usable_digests(self) -> bool {
        match self {
            LoaderKind::Fabric => true,
            LoaderKind::Quilt => false,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "fabric" => Some(LoaderKind::Fabric),
            "quilt" => Some(LoaderKind::Quilt),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoaderLibrary {
    pub name: String,
    pub maven_base: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

impl LoaderLibrary {
    pub fn path(&self) -> Option<String> {
        Coordinates::parse(&self.name).map(|coordinates| coordinates.to_path())
    }

    pub fn url(&self) -> Option<String> {
        let path = self.path()?;
        let separator = if self.maven_base.ends_with('/') {
            ""
        } else {
            "/"
        };
        Some(format!("{}{separator}{path}", self.maven_base))
    }

    pub fn checksum_url(&self) -> Option<String> {
        self.url().map(|url| format!("{url}.sha1"))
    }

    pub fn needs_checksum(&self) -> bool {
        self.sha1.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoaderProfile {
    pub kind: LoaderKind,
    pub game_version: String,
    pub loader_version: String,
    pub intermediary_version: Option<String>,
    pub main_class: String,
    pub min_java_version: Option<u32>,
    pub libraries: Vec<LoaderLibrary>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the {kind} metadata could not be read")]
    Parse {
        kind: &'static str,
        #[source]
        source: serde_json::Error,
    },

    #[error("no {kind} loader is published for Minecraft {game_version}")]
    NoBuilds {
        kind: &'static str,
        game_version: String,
    },

    #[error("{kind} loader {version} is not published for Minecraft {game_version}")]
    UnknownVersion {
        kind: &'static str,
        version: String,
        game_version: String,
    },

    #[error("the {kind} metadata declares no client main class")]
    NoMainClass { kind: &'static str },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Deserialize)]
struct RawEntry {
    loader: RawArtifact,
    #[serde(default)]
    intermediary: Option<RawArtifact>,
    #[serde(default)]
    hashed: Option<RawArtifact>,
    #[serde(rename = "launcherMeta")]
    launcher_meta: RawLauncherMeta,
}

#[derive(Debug, Deserialize)]
struct RawArtifact {
    maven: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    sha1: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    file_size: Option<u64>,
    #[serde(default)]
    hashes: Option<RawHashes>,
}

#[derive(Debug, Deserialize)]
struct RawHashes {
    #[serde(default)]
    sha1: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawLauncherMeta {
    #[serde(rename = "mainClass")]
    main_class: RawMainClass,
    #[serde(default)]
    libraries: RawLibraries,
    #[serde(default)]
    min_java_version: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawMainClass {
    Plain(String),
    PerSide { client: String },
}

impl RawMainClass {
    fn client(self) -> String {
        match self {
            RawMainClass::Plain(value) => value,
            RawMainClass::PerSide { client } => client,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct RawLibraries {
    #[serde(default)]
    common: Vec<RawLibrary>,
    #[serde(default)]
    client: Vec<RawLibrary>,
}

#[derive(Debug, Deserialize)]
struct RawLibrary {
    name: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    sha1: Option<String>,
    #[serde(default)]
    size: Option<u64>,
}

impl RawArtifact {
    fn sha1(&self) -> Option<String> {
        self.sha1
            .clone()
            .or_else(|| self.hashes.as_ref().and_then(|hashes| hashes.sha1.clone()))
    }

    fn size(&self) -> Option<u64> {
        self.size.or(self.file_size)
    }

    fn into_library(self, maven_base: &str) -> LoaderLibrary {
        LoaderLibrary {
            sha1: self.sha1(),
            size: self.size(),
            name: self.maven,
            maven_base: maven_base.to_string(),
        }
    }
}

pub fn parse_versions(
    kind: LoaderKind,
    game_version: &str,
    bytes: &[u8],
) -> Result<Vec<LoaderProfile>> {
    let entries: Vec<RawEntry> = serde_json::from_slice(bytes).map_err(|source| Error::Parse {
        kind: kind.as_str(),
        source,
    })?;

    if entries.is_empty() {
        return Err(Error::NoBuilds {
            kind: kind.as_str(),
            game_version: game_version.to_string(),
        });
    }

    entries
        .into_iter()
        .map(|entry| build_profile(kind, game_version, entry))
        .collect()
}

pub fn select<'a>(
    profiles: &'a [LoaderProfile],
    wanted: Option<&str>,
    kind: LoaderKind,
    game_version: &str,
) -> Result<&'a LoaderProfile> {
    match wanted {
        None => newest(profiles).ok_or_else(|| Error::NoBuilds {
            kind: kind.as_str(),
            game_version: game_version.to_string(),
        }),
        Some(version) => profiles
            .iter()
            .find(|profile| profile.loader_version == version)
            .ok_or_else(|| Error::UnknownVersion {
                kind: kind.as_str(),
                version: version.to_string(),
                game_version: game_version.to_string(),
            }),
    }
}

fn newest(profiles: &[LoaderProfile]) -> Option<&LoaderProfile> {
    let mut stable: Option<(&LoaderProfile, Version)> = None;
    let mut prerelease: Option<(&LoaderProfile, Version)> = None;

    for profile in profiles {
        let Ok(version) = Version::parse(&profile.loader_version) else {
            continue;
        };

        let slot = if version.pre.is_empty() {
            &mut stable
        } else {
            &mut prerelease
        };

        let better = match slot.as_ref() {
            Some((_, best)) => version > *best,
            None => true,
        };
        if better {
            *slot = Some((profile, version));
        }
    }

    stable
        .or(prerelease)
        .map(|(profile, _)| profile)
        .or_else(|| profiles.first())
}

fn build_profile(kind: LoaderKind, game_version: &str, entry: RawEntry) -> Result<LoaderProfile> {
    let main_class = entry.launcher_meta.main_class.client();
    if main_class.is_empty() {
        return Err(Error::NoMainClass {
            kind: kind.as_str(),
        });
    }

    let loader_version = entry
        .loader
        .version
        .clone()
        .or_else(|| Coordinates::parse(&entry.loader.maven).map(|c| c.version))
        .unwrap_or_default();

    let intermediary_version = entry
        .intermediary
        .as_ref()
        .and_then(|artifact| {
            artifact
                .version
                .clone()
                .or_else(|| Coordinates::parse(&artifact.maven).map(|c| c.version))
        })
        .or_else(|| {
            entry
                .hashed
                .as_ref()
                .and_then(|artifact| Coordinates::parse(&artifact.maven).map(|c| c.version))
        });

    let mut libraries = Vec::new();
    libraries.push(entry.loader.into_library(kind.maven_base()));

    if let Some(hashed) = entry.hashed {
        libraries.push(hashed.into_library(kind.maven_base()));
    }
    if let Some(intermediary) = entry.intermediary {
        libraries.push(intermediary.into_library(FABRIC_MAVEN));
    }

    for library in entry
        .launcher_meta
        .libraries
        .common
        .into_iter()
        .chain(entry.launcher_meta.libraries.client)
    {
        libraries.push(LoaderLibrary {
            maven_base: library.url.unwrap_or_else(|| kind.maven_base().to_string()),
            name: library.name,
            sha1: library.sha1,
            size: library.size,
        });
    }

    if !kind.publishes_usable_digests() {
        for library in &mut libraries {
            library.sha1 = None;
        }
    }

    Ok(LoaderProfile {
        kind,
        game_version: game_version.to_string(),
        loader_version,
        intermediary_version,
        main_class,
        min_java_version: entry.launcher_meta.min_java_version,
        libraries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FABRIC: &[u8] = br#"[
      {
        "loader": {"maven":"net.fabricmc:fabric-loader:0.19.3","version":"0.19.3","stable":true},
        "intermediary": {"maven":"net.fabricmc:intermediary:1.21.11","version":"1.21.11"},
        "launcherMeta": {
          "version": 2,
          "min_java_version": 8,
          "mainClass": {"client":"net.fabricmc.loader.impl.launch.knot.KnotClient","server":"x"},
          "libraries": {
            "client": [],
            "common": [
              {"name":"org.ow2.asm:asm:9.10.1","url":"https://maven.fabricmc.net/",
               "sha1":"ada2141c0cc52ee8f5c48cd5fa4ce0e794f22236","size":126151}
            ],
            "server": []
          }
        }
      },
      {
        "loader": {"maven":"net.fabricmc:fabric-loader:0.19.2","version":"0.19.2"},
        "intermediary": {"maven":"net.fabricmc:intermediary:1.21.11","version":"1.21.11"},
        "launcherMeta": {"version":2,"mainClass":{"client":"c","server":"s"},"libraries":{}}
      }
    ]"#;

    const QUILT: &[u8] = br#"[
      {
        "loader": {"maven":"org.quiltmc:quilt-loader:0.20.0-beta.9","version":"0.20.0-beta.9",
                   "file_size":1550096,"hashes":{"sha1":"bed0a01be87c4378d9002611d9642e8f9e9c2cff"}},
        "hashed": {"maven":"org.quiltmc:hashed:1.21.11","file_size":1103658,
                   "hashes":{"sha1":"bb8e21976c94855ff170a1cca66f737349f1b1f4"}},
        "intermediary": {"maven":"net.fabricmc:intermediary:1.21.11"},
        "launcherMeta": {
          "version": 1,
          "mainClass": {"client":"org.quiltmc.loader.impl.launch.knot.KnotClient","server":"x"},
          "libraries": {"client":[],"common":[
            {"name":"net.fabricmc:tiny-remapper:0.8.6","url":"https://maven.fabricmc.net/"}
          ],"server":[]}
        }
      }
    ]"#;

    fn fabric() -> Vec<LoaderProfile> {
        parse_versions(LoaderKind::Fabric, "1.21.11", FABRIC).unwrap()
    }

    fn quilt() -> Vec<LoaderProfile> {
        parse_versions(LoaderKind::Quilt, "1.21.11", QUILT).unwrap()
    }

    #[test]
    fn a_fabric_profile_carries_its_main_class_and_loader_version() {
        let profile = &fabric()[0];

        assert_eq!(profile.kind, LoaderKind::Fabric);
        assert_eq!(profile.loader_version, "0.19.3");
        assert_eq!(profile.intermediary_version.as_deref(), Some("1.21.11"));
        assert_eq!(
            profile.main_class,
            "net.fabricmc.loader.impl.launch.knot.KnotClient"
        );
        assert_eq!(profile.min_java_version, Some(8));
    }

    #[test]
    fn a_fabric_profile_includes_the_loader_and_intermediary_jars() {
        let profile = &fabric()[0];
        let names: Vec<&str> = profile
            .libraries
            .iter()
            .map(|library| library.name.as_str())
            .collect();

        assert!(names.contains(&"net.fabricmc:fabric-loader:0.19.3"));
        assert!(names.contains(&"net.fabricmc:intermediary:1.21.11"));
        assert!(names.contains(&"org.ow2.asm:asm:9.10.1"));
    }

    #[test]
    fn fabric_publishes_digests_for_its_declared_libraries() {
        let profile = &fabric()[0];
        let asm = profile
            .libraries
            .iter()
            .find(|library| library.name.starts_with("org.ow2.asm"))
            .unwrap();

        assert_eq!(
            asm.sha1.as_deref(),
            Some("ada2141c0cc52ee8f5c48cd5fa4ce0e794f22236")
        );
        assert_eq!(asm.size, Some(126151));
        assert!(!asm.needs_checksum());
    }

    #[test]
    fn the_fabric_loader_jar_has_no_inline_digest_and_needs_its_sidecar() {
        let profile = &fabric()[0];
        let loader = profile
            .libraries
            .iter()
            .find(|library| library.name.contains("fabric-loader"))
            .unwrap();

        assert!(
            loader.needs_checksum(),
            "the loader jar carries no digest inline, so its sidecar must be fetched"
        );
        assert_eq!(
            loader.checksum_url().as_deref(),
            Some("https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.19.3/fabric-loader-0.19.3.jar.sha1")
        );
    }

    #[test]
    fn a_quilt_profile_keeps_its_sizes_and_drops_its_digests() {
        let profile = &quilt()[0];

        assert_eq!(profile.kind, LoaderKind::Quilt);
        assert_eq!(profile.loader_version, "0.20.0-beta.9");
        assert_eq!(
            profile.main_class,
            "org.quiltmc.loader.impl.launch.knot.KnotClient"
        );

        let loader = profile
            .libraries
            .iter()
            .find(|library| library.name.contains("quilt-loader"))
            .unwrap();
        assert_eq!(loader.sha1, None);
        assert_eq!(loader.size, Some(1550096));
    }

    #[test]
    fn quilt_declares_libraries_without_digests_so_sidecars_are_required() {
        let profile = &quilt()[0];
        let remapper = profile
            .libraries
            .iter()
            .find(|library| library.name.contains("tiny-remapper"))
            .unwrap();

        assert!(
            remapper.needs_checksum(),
            "Quilt publishes no digests for its libraries, so every one needs a sidecar"
        );
    }

    #[test]
    fn quilt_uses_its_hashed_mappings_rather_than_intermediary_alone() {
        let profile = &quilt()[0];
        let names: Vec<&str> = profile
            .libraries
            .iter()
            .map(|library| library.name.as_str())
            .collect();

        assert!(names.contains(&"org.quiltmc:hashed:1.21.11"));
        assert!(names.contains(&"net.fabricmc:intermediary:1.21.11"));
    }

    #[test]
    fn libraries_resolve_to_urls_under_the_repository_that_declared_them() {
        let profile = &quilt()[0];

        let loader = profile
            .libraries
            .iter()
            .find(|library| library.name.contains("quilt-loader"))
            .unwrap();
        assert_eq!(
            loader.url().as_deref(),
            Some("https://maven.quiltmc.org/repository/release/org/quiltmc/quilt-loader/0.20.0-beta.9/quilt-loader-0.20.0-beta.9.jar")
        );

        let intermediary = profile
            .libraries
            .iter()
            .find(|library| library.name.contains("intermediary"))
            .unwrap();
        assert!(
            intermediary.url().unwrap().starts_with(FABRIC_MAVEN),
            "intermediary is published by Fabric even when Quilt uses it"
        );
    }

    #[test]
    fn the_newest_build_is_chosen_when_no_version_is_requested() {
        let profiles = fabric();
        let chosen = select(&profiles, None, LoaderKind::Fabric, "1.21.11").unwrap();
        assert_eq!(chosen.loader_version, "0.19.3");
    }

    #[test]
    fn an_explicit_loader_version_is_honoured() {
        let profiles = fabric();
        let chosen = select(&profiles, Some("0.19.2"), LoaderKind::Fabric, "1.21.11").unwrap();
        assert_eq!(chosen.loader_version, "0.19.2");
    }

    #[test]
    fn an_unpublished_loader_version_is_named_rather_than_guessed_at() {
        let profiles = fabric();
        let error = select(&profiles, Some("9.9.9"), LoaderKind::Fabric, "1.21.11").unwrap_err();

        match error {
            Error::UnknownVersion { version, .. } => assert_eq!(version, "9.9.9"),
            other => panic!("expected the version to be named, got {other:?}"),
        }
    }

    #[test]
    fn a_game_version_with_no_builds_is_reported() {
        let error = parse_versions(LoaderKind::Fabric, "1.2.3", b"[]").unwrap_err();
        assert!(matches!(error, Error::NoBuilds { .. }));
    }

    #[test]
    fn loader_names_round_trip() {
        assert_eq!(LoaderKind::parse("fabric"), Some(LoaderKind::Fabric));
        assert_eq!(LoaderKind::parse("Quilt"), Some(LoaderKind::Quilt));
        assert_eq!(LoaderKind::parse("forge"), None);
        assert_eq!(LoaderKind::Fabric.as_str(), "fabric");
    }

    const UNORDERED: &[u8] = br#"[
      {"loader":{"maven":"org.quiltmc:quilt-loader:0.20.0-beta.9","version":"0.20.0-beta.9",
                 "hashes":{"sha1":"bed0a01be87c4378d9002611d9642e8f9e9c2cff"},"file_size":1550096},
       "hashed":{"maven":"org.quiltmc:hashed:1.21.4","version":"1.21.4"},
       "launcherMeta":{"version":1,"mainClass":{"client":"org.quiltmc.loader.impl.launch.knot.KnotClient"},
                       "libraries":{"client":[],"common":[]}}},
      {"loader":{"maven":"org.quiltmc:quilt-loader:0.24.0","version":"0.24.0"},
       "launcherMeta":{"version":1,"mainClass":{"client":"org.quiltmc.loader.impl.launch.knot.KnotClient"},
                       "libraries":{"client":[],"common":[]}}},
      {"loader":{"maven":"org.quiltmc:quilt-loader:0.22.0","version":"0.22.0"},
       "launcherMeta":{"version":1,"mainClass":{"client":"org.quiltmc.loader.impl.launch.knot.KnotClient"},
                       "libraries":{"client":[],"common":[]}}}
    ]"#;

    #[test]
    fn the_newest_stable_build_wins_however_the_service_orders_its_list() {
        let profiles = parse_versions(LoaderKind::Quilt, "1.21.4", UNORDERED).unwrap();
        let chosen = select(&profiles, None, LoaderKind::Quilt, "1.21.4").unwrap();
        assert_eq!(
            chosen.loader_version, "0.24.0",
            "quilt lists its builds unordered, so taking the first entry hands the player a beta"
        );
    }

    #[test]
    fn a_prerelease_is_taken_only_when_nothing_stable_is_published() {
        let only_betas: &[u8] = br#"[
          {"loader":{"maven":"org.quiltmc:quilt-loader:0.20.0-beta.2","version":"0.20.0-beta.2"},
           "launcherMeta":{"version":1,"mainClass":{"client":"C"},"libraries":{"client":[],"common":[]}}},
          {"loader":{"maven":"org.quiltmc:quilt-loader:0.20.0-beta.10","version":"0.20.0-beta.10"},
           "launcherMeta":{"version":1,"mainClass":{"client":"C"},"libraries":{"client":[],"common":[]}}}
        ]"#;

        let profiles = parse_versions(LoaderKind::Quilt, "1.21.4", only_betas).unwrap();
        let chosen = select(&profiles, None, LoaderKind::Quilt, "1.21.4").unwrap();
        assert_eq!(chosen.loader_version, "0.20.0-beta.10");
    }

    #[test]
    fn quilt_digests_are_discarded_because_they_do_not_describe_the_published_jars() {
        let profiles = parse_versions(LoaderKind::Quilt, "1.21.4", UNORDERED).unwrap();
        let beta = profiles
            .iter()
            .find(|profile| profile.loader_version == "0.20.0-beta.9")
            .unwrap();

        assert!(
            beta.libraries.iter().all(|library| library.sha1.is_none()),
            "quilt publishes digests that match no artifact it serves, so they must never \
             be used to verify a download"
        );
        assert!(beta
            .libraries
            .iter()
            .all(|library| library.needs_checksum()));
        assert_eq!(beta.libraries[0].size, Some(1_550_096));
    }

    #[test]
    fn fabric_digests_are_kept_because_they_do_describe_the_published_jars() {
        let profiles = parse_versions(LoaderKind::Fabric, "1.21.11", FABRIC).unwrap();
        let asm = profiles[0]
            .libraries
            .iter()
            .find(|library| library.name.starts_with("org.ow2.asm:asm:"))
            .unwrap();

        assert_eq!(
            asm.sha1.as_deref(),
            Some("ada2141c0cc52ee8f5c48cd5fa4ce0e794f22236")
        );
        assert!(!asm.needs_checksum());
    }

    #[test]
    fn version_listing_urls_follow_each_projects_api() {
        assert_eq!(
            LoaderKind::Fabric.versions_url("1.21.11"),
            "https://meta.fabricmc.net/v2/versions/loader/1.21.11"
        );
        assert_eq!(
            LoaderKind::Quilt.versions_url("1.21.11"),
            "https://meta.quiltmc.org/v3/versions/loader/1.21.11"
        );
    }
}
