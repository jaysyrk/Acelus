use std::collections::HashSet;

use acelus_meta::loader::{self, LoaderKind, LoaderLibrary, LoaderProfile};
use acelus_meta::version::Coordinates;
use acelus_net::{Fetcher, Integrity};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::lock::{LockLoader, Role};
use crate::plan::{Plan, PlannedArtifact};

const CHECKSUM_CONCURRENCY: usize = 8;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not reach the {kind} metadata service")]
    Fetch {
        kind: &'static str,
        #[source]
        source: Box<acelus_net::Error>,
    },

    #[error(transparent)]
    Metadata(#[from] loader::Error),

    #[error("{url} is not a usable URL")]
    BadUrl { url: String },

    #[error("{name} is not a Maven coordinate, so it cannot be located")]
    BadCoordinates { name: String },

    #[error(
        "no checksum is published for {name}, and Acelus does not install files it cannot verify"
    )]
    NoChecksum { name: String },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderRequest {
    pub kind: LoaderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

impl LoaderRequest {
    pub fn latest(kind: LoaderKind) -> Self {
        Self {
            kind,
            version: None,
        }
    }
}

pub struct LoaderResolver {
    fetcher: Fetcher,
    fabric_meta: String,
    quilt_meta: String,
}

impl LoaderResolver {
    pub fn new(fetcher: Fetcher) -> Self {
        Self {
            fetcher,
            fabric_meta: loader::FABRIC_META.to_string(),
            quilt_meta: loader::QUILT_META.to_string(),
        }
    }

    pub fn with_endpoints(
        fetcher: Fetcher,
        fabric_meta: impl Into<String>,
        quilt_meta: impl Into<String>,
    ) -> Self {
        Self {
            fetcher,
            fabric_meta: fabric_meta.into(),
            quilt_meta: quilt_meta.into(),
        }
    }

    fn versions_url(&self, kind: LoaderKind, game_version: &str) -> String {
        let base = match kind {
            LoaderKind::Fabric => &self.fabric_meta,
            LoaderKind::Quilt => &self.quilt_meta,
        };
        format!(
            "{}/versions/loader/{game_version}",
            base.trim_end_matches('/')
        )
    }

    pub async fn profiles(
        &self,
        kind: LoaderKind,
        game_version: &str,
    ) -> Result<Vec<LoaderProfile>> {
        let bytes = self
            .get(&self.versions_url(kind, game_version), kind)
            .await?;
        Ok(loader::parse_versions(kind, game_version, &bytes)?)
    }

    pub async fn resolve(
        &self,
        request: &LoaderRequest,
        game_version: &str,
    ) -> Result<LoaderProfile> {
        let profiles = self.profiles(request.kind, game_version).await?;
        let chosen = loader::select(
            &profiles,
            request.version.as_deref(),
            request.kind,
            game_version,
        )?;
        Ok(chosen.clone())
    }

    pub async fn apply(&self, plan: &mut Plan, profile: &LoaderProfile) -> Result<()> {
        let libraries = self.with_checksums(profile).await?;

        let mut seen: HashSet<String> = libraries
            .iter()
            .filter_map(|library| dedup_key(&library.name))
            .collect();

        let mut planned = Vec::with_capacity(libraries.len());
        for library in &libraries {
            planned.push(planned_library(library)?);
        }

        plan.artifacts.retain(|artifact| {
            if artifact.role != Role::Library {
                return true;
            }
            match artifact_key(artifact) {
                Some(key) => seen.insert(key),
                None => true,
            }
        });

        let insert_at = plan
            .artifacts
            .iter()
            .position(|artifact| artifact.role == Role::Library)
            .unwrap_or(plan.artifacts.len());

        for (offset, artifact) in planned.into_iter().enumerate() {
            plan.artifacts.insert(insert_at + offset, artifact);
        }

        plan.minecraft.main_class = profile.main_class.clone();
        plan.loader = Some(LockLoader {
            kind: profile.kind,
            version: profile.loader_version.clone(),
            intermediary_version: profile.intermediary_version.clone(),
        });

        if let Some(required) = profile.min_java_version {
            plan.java.major_version = plan.java.major_version.max(required);
        }

        Ok(())
    }

    async fn with_checksums(&self, profile: &LoaderProfile) -> Result<Vec<LoaderLibrary>> {
        let owned: Vec<LoaderLibrary> = profile.libraries.clone();
        let pending = owned.into_iter().map(|library| async move {
            if !library.needs_checksum() {
                return Ok(library);
            }
            let sha1 = self.checksum(&library).await?;
            Ok(LoaderLibrary {
                sha1: Some(sha1),
                ..library
            })
        });

        let resolved: Vec<Result<LoaderLibrary>> = futures::stream::iter(pending)
            .buffer_unordered(CHECKSUM_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;

        let mut libraries = Vec::with_capacity(resolved.len());
        for outcome in resolved {
            libraries.push(outcome?);
        }

        libraries.sort_by_key(|library| {
            profile
                .libraries
                .iter()
                .position(|original| original.name == library.name)
                .unwrap_or(usize::MAX)
        });

        Ok(libraries)
    }

    async fn checksum(&self, library: &LoaderLibrary) -> Result<String> {
        let source = library
            .checksum_url()
            .ok_or_else(|| Error::BadCoordinates {
                name: library.name.clone(),
            })?;

        let url: Url = source.parse().map_err(|_| Error::BadUrl {
            url: source.clone(),
        })?;

        let mut buffer = Vec::new();
        self.fetcher
            .to_writer(&url, &Integrity::none(), &mut buffer)
            .await
            .map_err(|_| Error::NoChecksum {
                name: library.name.clone(),
            })?;

        let text = String::from_utf8_lossy(&buffer);
        let digest = text
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();

        if digest.len() != 40 || !digest.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::NoChecksum {
                name: library.name.clone(),
            });
        }

        Ok(digest)
    }

    async fn get(&self, url: &str, kind: LoaderKind) -> Result<Vec<u8>> {
        let parsed: Url = url.parse().map_err(|_| Error::BadUrl {
            url: url.to_string(),
        })?;

        let mut buffer = Vec::new();
        self.fetcher
            .to_writer(&parsed, &Integrity::none(), &mut buffer)
            .await
            .map_err(|source| Error::Fetch {
                kind: kind.as_str(),
                source: Box::new(source),
            })?;

        Ok(buffer)
    }
}

fn planned_library(library: &LoaderLibrary) -> Result<PlannedArtifact> {
    let path = library.path().ok_or_else(|| Error::BadCoordinates {
        name: library.name.clone(),
    })?;
    let url = library.url().ok_or_else(|| Error::BadCoordinates {
        name: library.name.clone(),
    })?;

    let sha1 = library.sha1.clone().ok_or_else(|| Error::NoChecksum {
        name: library.name.clone(),
    })?;

    let mut artifact = PlannedArtifact::new(format!("libraries/{path}"), Role::Library, url);
    artifact.sha1 = Some(sha1);
    artifact.size = library.size;
    Ok(artifact)
}

fn dedup_key(name: &str) -> Option<String> {
    Coordinates::parse(name).map(|coordinates| coordinates.dedup_key())
}

fn artifact_key(artifact: &PlannedArtifact) -> Option<String> {
    let path = artifact.path.strip_prefix("libraries/")?;
    let file = path.rsplit('/').next()?;
    let mut segments: Vec<&str> = path.split('/').collect();
    segments.pop();
    let version = segments.pop()?;
    let name = segments.pop()?;
    let group = segments.join(".");

    let stem = file.strip_suffix(".jar").unwrap_or(file);
    let prefix = format!("{name}-{version}");
    let classifier = stem
        .strip_prefix(&prefix)
        .and_then(|rest| rest.strip_prefix('-'))
        .filter(|rest| !rest.is_empty());

    Some(match classifier {
        Some(classifier) => format!("{group}:{name}:{classifier}"),
        None => format!("{group}:{name}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(path: &str) -> PlannedArtifact {
        PlannedArtifact::new(path, Role::Library, "https://example/x.jar")
    }

    #[test]
    fn a_library_path_maps_back_to_its_coordinates() {
        assert_eq!(
            artifact_key(&artifact("libraries/org/ow2/asm/asm/9.10.1/asm-9.10.1.jar")).as_deref(),
            Some("org.ow2.asm:asm")
        );
        assert_eq!(
            artifact_key(&artifact(
                "libraries/com/mojang/jtracy/1.0.37/jtracy-1.0.37-natives-linux.jar"
            ))
            .as_deref(),
            Some("com.mojang:jtracy:natives-linux")
        );
    }

    #[test]
    fn the_path_key_agrees_with_the_coordinate_key() {
        for name in [
            "org.ow2.asm:asm:9.10.1",
            "com.mojang:jtracy:1.0.37:natives-linux",
            "net.fabricmc:fabric-loader:0.19.3",
        ] {
            let coordinates = Coordinates::parse(name).unwrap();
            let path = format!("libraries/{}", coordinates.to_path());
            assert_eq!(
                artifact_key(&artifact(&path)),
                dedup_key(name),
                "the two ways of keying {name} must agree or deduplication silently fails"
            );
        }
    }

    #[test]
    fn a_path_that_is_not_a_library_has_no_key() {
        assert_eq!(artifact_key(&artifact("client.jar")), None);
    }
}
