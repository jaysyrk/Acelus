use acelus_meta::assets::AssetIndex;
use acelus_meta::java::{Platform, RuntimeManifest, JAVA_RUNTIME_MANIFEST_URL};
use acelus_meta::manifest::VERSION_MANIFEST_URL;
use acelus_meta::{Environment, VersionJson, VersionManifest};
use acelus_net::{Fetcher, Integrity};
use url::Url;

use crate::lock::{JavaSource, LockMinecraft, Role};
use crate::plan::{Plan, PlannedArtifact, PlannedJava};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not fetch {what}")]
    Fetch {
        what: String,
        #[source]
        source: Box<acelus_net::Error>,
    },

    #[error("{what} is not valid metadata")]
    Parse {
        what: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("{url} is not a usable URL")]
    BadUrl { url: String },

    #[error("no Minecraft version is published with the id {id}")]
    UnknownVersion { id: String },

    #[error("version {id} declares no client download, so there is nothing to launch")]
    NoClientDownload { id: String },

    #[error(
        "Mojang publishes no Java runtime for this platform, \
         so a runtime must be provided another way"
    )]
    NoRuntimeForPlatform,
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Resolver {
    fetcher: Fetcher,
    version_manifest_url: String,
    java_manifest_url: String,
}

impl Resolver {
    pub fn new(fetcher: Fetcher) -> Self {
        Self {
            fetcher,
            version_manifest_url: VERSION_MANIFEST_URL.to_string(),
            java_manifest_url: JAVA_RUNTIME_MANIFEST_URL.to_string(),
        }
    }

    pub fn with_endpoints(
        fetcher: Fetcher,
        version_manifest_url: impl Into<String>,
        java_manifest_url: impl Into<String>,
    ) -> Self {
        Self {
            fetcher,
            version_manifest_url: version_manifest_url.into(),
            java_manifest_url: java_manifest_url.into(),
        }
    }

    pub async fn version_manifest(&self) -> Result<VersionManifest> {
        let bytes = self
            .get(
                &self.version_manifest_url,
                "the version manifest",
                &Integrity::none(),
            )
            .await?;
        VersionManifest::parse(&bytes).map_err(|source| Error::Parse {
            what: "the version manifest".into(),
            source,
        })
    }

    pub async fn resolve(&self, version_id: &str, env: &Environment) -> Result<Plan> {
        let manifest = self.version_manifest().await?;
        let summary = manifest
            .find(version_id)
            .ok_or_else(|| Error::UnknownVersion {
                id: version_id.to_string(),
            })?;

        let integrity = match &summary.sha1 {
            Some(sha1) => Integrity::sha1(sha1.clone()),
            None => Integrity::none(),
        };
        let bytes = self
            .get(&summary.url, "the version metadata", &integrity)
            .await?;

        let version = VersionJson::parse(&bytes).map_err(|source| Error::Parse {
            what: format!("the metadata for {version_id}"),
            source,
        })?;

        self.plan_for(&version, summary.sha1.clone().unwrap_or_default(), env)
            .await
    }

    async fn plan_for(
        &self,
        version: &VersionJson,
        manifest_sha1: String,
        env: &Environment,
    ) -> Result<Plan> {
        let mut artifacts = Vec::new();

        let client = version
            .downloads
            .as_ref()
            .and_then(|d| d.client.as_ref())
            .ok_or_else(|| Error::NoClientDownload {
                id: version.id.clone(),
            })?;

        artifacts.push(
            PlannedArtifact::new("client.jar", Role::Client, client.url.clone())
                .with_sha1(client.sha1.clone())
                .with_size(client.size),
        );

        let libraries = version.resolve_libraries(env);

        for library in &libraries.classpath {
            let mut artifact = PlannedArtifact::new(
                format!("libraries/{}", library.path),
                Role::Library,
                library.url.clone(),
            );
            artifact.sha1 = library.sha1.clone();
            artifact.size = library.size;
            artifacts.push(artifact);
        }

        for native in &libraries.natives {
            let mut artifact = PlannedArtifact::new(
                format!("natives/jars/{}", native.artifact.path),
                Role::Native,
                native.artifact.url.clone(),
            )
            .with_extract(native.exclude.clone());
            artifact.sha1 = native.artifact.sha1.clone();
            artifact.size = native.artifact.size;
            artifacts.push(artifact);
        }

        if let Some(logging) = version.logging.as_ref().and_then(|l| l.client.as_ref()) {
            artifacts.push(
                PlannedArtifact::new(
                    format!("logging/{}", logging.file.id),
                    Role::LoggingConfig,
                    logging.file.url.clone(),
                )
                .with_sha1(logging.file.sha1.clone())
                .with_size(logging.file.size),
            );
        }

        let asset_index = match version.asset_index.as_ref() {
            Some(reference) => {
                artifacts.push(
                    PlannedArtifact::new(
                        format!("assets/indexes/{}.json", reference.id),
                        Role::Asset,
                        reference.url.clone(),
                    )
                    .with_sha1(reference.sha1.clone())
                    .with_size(reference.size),
                );

                let bytes = self
                    .get(
                        &reference.url,
                        "the asset index",
                        &Integrity::sha1(reference.sha1.clone()).with_size(reference.size),
                    )
                    .await?;

                Some(AssetIndex::parse(&bytes).map_err(|source| Error::Parse {
                    what: "the asset index".into(),
                    source,
                })?)
            }
            None => None,
        };

        let java = self.plan_java(version, env).await?;

        Ok(Plan {
            loader: None,
            minecraft: LockMinecraft {
                id: version.id.clone(),
                release_type: release_type_name(version).to_string(),
                manifest_sha1,
                asset_index_id: version.assets.clone(),
                main_class: version.main_class.clone(),
                compliance_level: version.compliance_level,
            },
            java,
            artifacts,
            arguments: version.resolve_arguments(env),
            asset_index,
        })
    }

    async fn plan_java(&self, version: &VersionJson, env: &Environment) -> Result<PlannedJava> {
        let declared = version.java_version.as_ref();
        let major_version = declared.map(|j| j.major_version).unwrap_or(8);
        let component = declared.map(|j| j.component.clone());

        let Some(platform) = Platform::for_target(env.os, env.arch) else {
            return Ok(PlannedJava {
                component,
                major_version,
                source: JavaSource::Adoptium,
                manifest_url: None,
                manifest_sha1: None,
            });
        };

        let Some(component_name) = component.clone() else {
            return Ok(PlannedJava {
                component,
                major_version,
                source: JavaSource::Adoptium,
                manifest_url: None,
                manifest_sha1: None,
            });
        };

        let bytes = self
            .get(
                &self.java_manifest_url,
                "the Java runtime manifest",
                &Integrity::none(),
            )
            .await?;

        let manifest = RuntimeManifest::parse(&bytes).map_err(|source| Error::Parse {
            what: "the Java runtime manifest".into(),
            source,
        })?;

        match manifest.resolve(platform, &component_name) {
            Some(entry) => Ok(PlannedJava {
                component,
                major_version,
                source: JavaSource::Mojang,
                manifest_url: Some(entry.manifest.url.clone()),
                manifest_sha1: Some(entry.manifest.sha1.clone()),
            }),
            None => Ok(PlannedJava {
                component,
                major_version,
                source: JavaSource::Adoptium,
                manifest_url: None,
                manifest_sha1: None,
            }),
        }
    }

    async fn get(&self, url: &str, what: &str, integrity: &Integrity) -> Result<Vec<u8>> {
        let parsed: Url = url.parse().map_err(|_| Error::BadUrl {
            url: url.to_string(),
        })?;

        let mut buffer = Vec::new();
        self.fetcher
            .to_writer(&parsed, integrity, &mut buffer)
            .await
            .map_err(|source| Error::Fetch {
                what: what.to_string(),
                source: Box::new(source),
            })?;

        Ok(buffer)
    }
}

fn release_type_name(version: &VersionJson) -> &'static str {
    use acelus_meta::ReleaseType;
    match version.release_type {
        ReleaseType::Release => "release",
        ReleaseType::Snapshot => "snapshot",
        ReleaseType::OldBeta => "old_beta",
        ReleaseType::OldAlpha => "old_alpha",
    }
}
