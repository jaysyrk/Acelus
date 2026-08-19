use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use acelus_cas::{Hash, Materialize, Store};
use acelus_meta::assets::{AssetIndex, AssetObject};
use acelus_net::{Fetcher, Integrity};
use futures::StreamExt;
use url::Url;

use crate::extract;
use crate::lock::{LockArguments, LockArtifact, LockExtract, Lockfile, Role, LOCK_VERSION};
use crate::paths::{InstanceLayout, Paths};
use crate::plan::{Plan, PlannedArtifact};

pub const SHA1_NAMESPACE: &str = "sha1";
const ASSET_CONCURRENCY: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not fetch {what}")]
    Fetch {
        what: String,
        #[source]
        source: Box<acelus_net::Error>,
    },

    #[error("{url} is not a usable URL")]
    BadUrl { url: String },

    #[error("{path} has no source to be fetched from")]
    NoOrigin { path: String },

    #[error("the content addressed store rejected {what}")]
    Store {
        what: String,
        #[source]
        source: acelus_cas::Error,
    },

    #[error("could not unpack the natives for {what}")]
    Extract {
        what: String,
        #[source]
        source: Box<extract::Error>,
    },

    #[error("i/o failed at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Resolve,
    Download,
    Verify,
    Extract,
    Link,
    Done,
}

#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub phase: Phase,
    pub completed_bytes: u64,
    pub total_bytes: u64,
    pub completed_files: u64,
    pub total_files: u64,
    pub reused_bytes: u64,
    pub current: Option<String>,
}

#[derive(Default)]
struct Counters {
    completed_bytes: AtomicU64,
    completed_files: AtomicU64,
    reused_bytes: AtomicU64,
}

pub struct Installer {
    fetcher: Fetcher,
    store: Store,
    paths: Paths,
    asset_base_url: String,
}

impl Installer {
    pub fn new(fetcher: Fetcher, store: Store, paths: Paths) -> Self {
        Self {
            fetcher,
            store,
            paths,
            asset_base_url: acelus_meta::assets::RESOURCES_BASE_URL.to_string(),
        }
    }

    pub fn with_asset_base_url(mut self, base: impl Into<String>) -> Self {
        self.asset_base_url = base.into().trim_end_matches('/').to_string();
        self
    }

    fn asset_url(&self, object: &AssetObject) -> String {
        format!("{}/{}", self.asset_base_url, object.store_path())
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub async fn install(
        &self,
        plan: &Plan,
        layout: &InstanceLayout,
        report: &(impl Fn(&Progress) + Sync),
    ) -> Result<Lockfile> {
        let counters = Counters::default();
        let total_files = plan.artifacts.len() as u64
            + plan
                .asset_index
                .as_ref()
                .map(|index| index.objects.len() as u64)
                .unwrap_or(0);
        let total_bytes = plan.total_download_size();

        let emit = |phase: Phase, current: Option<String>| {
            report(&Progress {
                phase,
                completed_bytes: counters.completed_bytes.load(Ordering::Relaxed),
                total_bytes,
                completed_files: counters.completed_files.load(Ordering::Relaxed),
                total_files,
                reused_bytes: counters.reused_bytes.load(Ordering::Relaxed),
                current,
            })
        };

        emit(Phase::Resolve, None);

        let mut locked = Vec::with_capacity(plan.artifacts.len());
        for artifact in &plan.artifacts {
            emit(Phase::Download, Some(artifact.path.clone()));
            let hash = self.acquire(artifact, &counters).await?;
            locked.push(lock_entry(artifact, hash, self.size_of(&hash)?));
        }

        emit(Phase::Link, None);
        for (artifact, entry) in plan.artifacts.iter().zip(&locked) {
            self.place(artifact, entry, layout)?;
        }

        emit(Phase::Extract, None);
        for artifact in plan.artifacts_with_role(Role::Native) {
            self.unpack_native(artifact, layout)?;
        }

        if let Some(index) = plan.asset_index.as_ref() {
            emit(Phase::Download, Some("assets".into()));
            self.install_assets(index, &counters, &emit).await?;
        }

        emit(Phase::Done, None);

        Ok(Lockfile {
            lock_version: LOCK_VERSION,
            generated_by: Some(format!("acelus {}", env!("CARGO_PKG_VERSION"))),
            minecraft: plan.minecraft.clone(),
            loader: plan.loader.clone(),
            java: crate::lock::LockJava {
                component: plan.java.component.clone(),
                major_version: plan.java.major_version,
                source: plan.java.source,
            },
            arguments: LockArguments {
                game: plan.arguments.game.clone(),
                jvm: plan.arguments.jvm.clone(),
            },
            artifacts: locked,
        })
    }

    async fn acquire(&self, artifact: &PlannedArtifact, counters: &Counters) -> Result<Hash> {
        if let Some(sha1) = artifact.sha1.as_deref() {
            let existing = self
                .store
                .resolve_alias(SHA1_NAMESPACE, sha1)
                .map_err(|source| Error::Store {
                    what: artifact.path.clone(),
                    source,
                })?;

            if let Some(hash) = existing {
                counters
                    .reused_bytes
                    .fetch_add(artifact.size.unwrap_or(0), Ordering::Relaxed);
                counters.completed_files.fetch_add(1, Ordering::Relaxed);
                counters
                    .completed_bytes
                    .fetch_add(artifact.size.unwrap_or(0), Ordering::Relaxed);
                return Ok(hash);
            }
        }

        let url = artifact.origin.first().ok_or_else(|| Error::NoOrigin {
            path: artifact.path.clone(),
        })?;

        let hash = self
            .download_into_store(url, &artifact.integrity(), &artifact.path)
            .await?;

        if let Some(sha1) = artifact.sha1.as_deref() {
            self.store
                .set_alias(SHA1_NAMESPACE, sha1, &hash)
                .map_err(|source| Error::Store {
                    what: artifact.path.clone(),
                    source,
                })?;
        }

        counters.completed_files.fetch_add(1, Ordering::Relaxed);
        counters
            .completed_bytes
            .fetch_add(artifact.size.unwrap_or(0), Ordering::Relaxed);

        Ok(hash)
    }

    async fn download_into_store(
        &self,
        url: &str,
        integrity: &Integrity,
        what: &str,
    ) -> Result<Hash> {
        let parsed: Url = url.parse().map_err(|_| Error::BadUrl {
            url: url.to_string(),
        })?;

        let mut writer = self.store.writer().map_err(|source| Error::Store {
            what: what.to_string(),
            source,
        })?;

        self.fetcher
            .to_writer(&parsed, integrity, &mut writer)
            .await
            .map_err(|source| Error::Fetch {
                what: what.to_string(),
                source: Box::new(source),
            })?;

        writer.commit().map_err(|source| Error::Store {
            what: what.to_string(),
            source,
        })
    }

    fn size_of(&self, hash: &Hash) -> Result<u64> {
        self.store.size_of(hash).map_err(|source| Error::Store {
            what: hash.to_hex(),
            source,
        })
    }

    fn destination(&self, artifact: &PlannedArtifact, layout: &InstanceLayout) -> PathBuf {
        match artifact.role {
            Role::Asset => self.paths.root().join(&artifact.path),
            _ => layout.resolve(&artifact.path),
        }
    }

    fn place(
        &self,
        artifact: &PlannedArtifact,
        entry: &LockArtifact,
        layout: &InstanceLayout,
    ) -> Result<()> {
        let destination = self.destination(artifact, layout);
        let mode = if artifact.role.is_shared() {
            Materialize::Shared
        } else {
            Materialize::Private
        };

        let hash: Hash = entry
            .blake3
            .parse()
            .expect("the lock entry was built from a store hash");

        self.store
            .materialize(&hash, &destination, mode)
            .map(|_| ())
            .map_err(|source| Error::Store {
                what: artifact.path.clone(),
                source,
            })
    }

    fn unpack_native(&self, artifact: &PlannedArtifact, layout: &InstanceLayout) -> Result<()> {
        let source = layout.resolve(&artifact.path);
        let archive = std::fs::read(&source).map_err(|source_error| Error::Io {
            path: source.clone(),
            source: source_error,
        })?;

        let exclude = artifact.extract.clone().unwrap_or_default();

        extract::extract_natives(&archive, &layout.natives(), &exclude).map_err(|source| {
            Error::Extract {
                what: artifact.path.clone(),
                source: Box::new(source),
            }
        })?;

        Ok(())
    }

    async fn install_assets(
        &self,
        index: &AssetIndex,
        counters: &Counters,
        emit: &(impl Fn(Phase, Option<String>) + Sync),
    ) -> Result<()> {
        let objects: Vec<AssetObject> = index.objects.values().cloned().collect();

        futures::stream::iter(objects.into_iter().map(|object| async move {
            self.install_asset(&object, counters).await?;
            emit(Phase::Download, Some(object.hash.clone()));
            Ok::<(), Error>(())
        }))
        .buffer_unordered(ASSET_CONCURRENCY)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<()>>>()?;

        Ok(())
    }

    async fn install_asset(&self, object: &AssetObject, counters: &Counters) -> Result<()> {
        let destination = self.paths.asset_objects().join(object.store_path());

        if is_present_with_size(&destination, object.size) {
            counters
                .reused_bytes
                .fetch_add(object.size, Ordering::Relaxed);
            counters.completed_files.fetch_add(1, Ordering::Relaxed);
            counters
                .completed_bytes
                .fetch_add(object.size, Ordering::Relaxed);
            return Ok(());
        }

        let source = self.asset_url(object);
        let url: Url = source.parse().map_err(|_| Error::BadUrl {
            url: source.clone(),
        })?;

        let integrity = Integrity::sha1(object.hash.clone()).with_size(object.size);

        self.fetcher
            .to_file(&url, &integrity, &destination)
            .await
            .map_err(|source| Error::Fetch {
                what: format!("asset {}", object.hash),
                source: Box::new(source),
            })?;

        counters.completed_files.fetch_add(1, Ordering::Relaxed);
        counters
            .completed_bytes
            .fetch_add(object.size, Ordering::Relaxed);

        Ok(())
    }
}

fn is_present_with_size(path: &Path, size: u64) -> bool {
    std::fs::metadata(path)
        .map(|meta| meta.is_file() && meta.len() == size)
        .unwrap_or(false)
}

fn lock_entry(artifact: &PlannedArtifact, hash: Hash, size: u64) -> LockArtifact {
    LockArtifact {
        path: artifact.path.clone(),
        role: artifact.role,
        size,
        blake3: hash.to_hex(),
        sha1: artifact.sha1.clone(),
        sha512: artifact.sha512.clone(),
        origin: artifact.origin.clone(),
        extract: artifact.extract.as_ref().map(|exclude| LockExtract {
            exclude: exclude.clone(),
        }),
    }
}
