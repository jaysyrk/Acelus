use std::collections::HashSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;
use url::Url;

use crate::integrity::{Integrity, IntegrityError, Verifier};

pub const DEFAULT_ALLOWED_HOSTS: &[&str] = &[
    "piston-meta.mojang.com",
    "piston-data.mojang.com",
    "launchermeta.mojang.com",
    "launcher.mojang.com",
    "libraries.minecraft.net",
    "resources.download.minecraft.net",
    "api.minecraftservices.com",
    "meta.fabricmc.net",
    "maven.fabricmc.net",
    "meta.quiltmc.org",
    "maven.quiltmc.org",
    "maven.neoforged.net",
    "maven.minecraftforge.net",
    "api.modrinth.com",
    "cdn.modrinth.com",
    "api.curseforge.com",
    "api.adoptium.net",
];

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{url} is not an allowed download host")]
    HostNotAllowed { url: Url },

    #[error("request to {url} failed")]
    Transport {
        url: Url,
        #[source]
        source: reqwest::Error,
    },

    #[error("{url} returned HTTP {status}")]
    Status { url: Url, status: u16 },

    #[error("{url} failed integrity verification")]
    Integrity {
        url: Url,
        #[source]
        source: Box<IntegrityError>,
    },

    #[error("i/o failed at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("writing the response body for {url} failed")]
    Sink {
        url: Url,
        #[source]
        source: io::Error,
    },

    #[error("gave up on {url} after {attempts} attempts")]
    Exhausted {
        url: Url,
        attempts: u32,
        #[source]
        source: Box<Error>,
    },
}

impl Error {
    pub fn is_retryable(&self) -> bool {
        match self {
            Error::Transport { source, .. } => {
                source.is_timeout() || source.is_connect() || source.is_request()
            }
            Error::Status { status, .. } => {
                *status == 408 || *status == 429 || (500..600).contains(status)
            }
            Error::Sink { .. } | Error::Io { .. } => false,
            Error::Integrity { .. } => false,
            Error::HostNotAllowed { .. } => false,
            Error::Exhausted { .. } => false,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone)]
pub struct Fetcher {
    client: reqwest::Client,
    permits: Arc<Semaphore>,
    attempts: u32,
    backoff: Duration,
    allowed_hosts: Option<Arc<HashSet<String>>>,
}

#[derive(Debug, Clone)]
pub struct Builder {
    user_agent: String,
    concurrency: usize,
    attempts: u32,
    backoff: Duration,
    timeout: Duration,
    connect_timeout: Duration,
    allowed_hosts: Option<HashSet<String>>,
    require_https: bool,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            user_agent: format!("Acelus/{}", env!("CARGO_PKG_VERSION")),
            concurrency: 16,
            attempts: 4,
            backoff: Duration::from_millis(250),
            timeout: Duration::from_secs(120),
            connect_timeout: Duration::from_secs(15),
            allowed_hosts: Some(
                DEFAULT_ALLOWED_HOSTS
                    .iter()
                    .map(|h| h.to_string())
                    .collect(),
            ),
            require_https: true,
        }
    }
}

impl Builder {
    pub fn user_agent(mut self, value: impl Into<String>) -> Self {
        self.user_agent = value.into();
        self
    }

    pub fn concurrency(mut self, value: usize) -> Self {
        self.concurrency = value.max(1);
        self
    }

    pub fn attempts(mut self, value: u32) -> Self {
        self.attempts = value.max(1);
        self
    }

    pub fn backoff(mut self, value: Duration) -> Self {
        self.backoff = value;
        self
    }

    pub fn timeout(mut self, value: Duration) -> Self {
        self.timeout = value;
        self
    }

    pub fn allow_host(mut self, host: impl Into<String>) -> Self {
        self.allowed_hosts
            .get_or_insert_with(HashSet::new)
            .insert(host.into());
        self
    }

    pub fn allow_any_host(mut self) -> Self {
        self.allowed_hosts = None;
        self
    }

    pub fn require_https(mut self, value: bool) -> Self {
        self.require_https = value;
        self
    }

    pub fn build(self) -> reqwest::Result<Fetcher> {
        let client = reqwest::Client::builder()
            .user_agent(self.user_agent)
            .timeout(self.timeout)
            .connect_timeout(self.connect_timeout)
            .pool_max_idle_per_host(self.concurrency)
            .https_only(self.require_https)
            .build()?;

        Ok(Fetcher {
            client,
            permits: Arc::new(Semaphore::new(self.concurrency)),
            attempts: self.attempts,
            backoff: self.backoff,
            allowed_hosts: self.allowed_hosts.map(Arc::new),
        })
    }
}

impl Fetcher {
    pub fn builder() -> Builder {
        Builder::default()
    }

    pub fn new() -> reqwest::Result<Self> {
        Builder::default().build()
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    fn check_host(&self, url: &Url) -> Result<()> {
        let Some(allowed) = self.allowed_hosts.as_ref() else {
            return Ok(());
        };
        match url.host_str() {
            Some(host) if allowed.contains(host) => Ok(()),
            _ => Err(Error::HostNotAllowed { url: url.clone() }),
        }
    }

    pub async fn bytes(&self, url: &Url) -> Result<Vec<u8>> {
        self.retrying(url, |attempt| async move {
            let _permit = self
                .permits
                .acquire()
                .await
                .expect("semaphore is never closed");
            let response = self.send(url, None, attempt).await?;
            response
                .bytes()
                .await
                .map(|b| b.to_vec())
                .map_err(|source| Error::Transport {
                    url: url.clone(),
                    source,
                })
        })
        .await
    }

    pub async fn to_writer<W: Write>(
        &self,
        url: &Url,
        integrity: &Integrity,
        mut sink: W,
    ) -> Result<u64> {
        let _permit = self
            .permits
            .acquire()
            .await
            .expect("semaphore is never closed");
        let mut response = self.send(url, None, 0).await?;
        let mut verifier = integrity.verifier();

        while let Some(chunk) = response.chunk().await.map_err(|source| Error::Transport {
            url: url.clone(),
            source,
        })? {
            verifier.update(&chunk);
            sink.write_all(&chunk).map_err(|source| Error::Sink {
                url: url.clone(),
                source,
            })?;
        }

        sink.flush().map_err(|source| Error::Sink {
            url: url.clone(),
            source,
        })?;

        verifier.finish().map_err(|source| Error::Integrity {
            url: url.clone(),
            source: Box::new(source),
        })
    }

    pub async fn to_file(&self, url: &Url, integrity: &Integrity, path: &Path) -> Result<u64> {
        self.retrying(url, |attempt| async move {
            let _permit = self
                .permits
                .acquire()
                .await
                .expect("semaphore is never closed");
            self.download_resumable(url, integrity, path, attempt).await
        })
        .await
    }

    async fn download_resumable(
        &self,
        url: &Url,
        integrity: &Integrity,
        path: &Path,
        attempt: u32,
    ) -> Result<u64> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|source| Error::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
        }

        let resume_from = resumable_prefix(path, integrity).await?;
        let mut response = self.send(url, resume_from, attempt).await?;
        let resumed = resume_from.is_some() && response.status().as_u16() == 206;

        let mut verifier = integrity.verifier();
        let mut file = if resumed {
            seed_verifier_from_prefix(path, resume_from.unwrap_or(0), &mut verifier).await?;
            open_for_append(path).await?
        } else {
            create_truncated(path).await?
        };

        while let Some(chunk) = response.chunk().await.map_err(|source| Error::Transport {
            url: url.clone(),
            source,
        })? {
            verifier.update(&chunk);
            write_all(&mut file, path, &chunk).await?;
        }

        flush(&mut file, path).await?;

        verifier.finish().map_err(|source| Error::Integrity {
            url: url.clone(),
            source: Box::new(source),
        })
    }

    async fn send(
        &self,
        url: &Url,
        resume_from: Option<u64>,
        attempt: u32,
    ) -> Result<reqwest::Response> {
        self.check_host(url)?;

        let mut request = self.client.get(url.clone());
        if let Some(offset) = resume_from {
            request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
        }

        tracing::debug!(%url, attempt, resume_from, "fetching");

        let response = request.send().await.map_err(|source| Error::Transport {
            url: url.clone(),
            source,
        })?;

        let status = response.status();
        if status.is_success() {
            Ok(response)
        } else {
            Err(Error::Status {
                url: url.clone(),
                status: status.as_u16(),
            })
        }
    }

    async fn retrying<T, F, Fut>(&self, url: &Url, mut operation: F) -> Result<T>
    where
        F: FnMut(u32) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut last: Option<Error> = None;

        for attempt in 0..self.attempts {
            if attempt > 0 {
                tokio::time::sleep(self.backoff * 2u32.pow(attempt - 1)).await;
            }
            match operation(attempt).await {
                Ok(value) => return Ok(value),
                Err(error) if error.is_retryable() => {
                    tracing::warn!(%url, attempt, %error, "retrying");
                    last = Some(error);
                }
                Err(error) => return Err(error),
            }
        }

        Err(Error::Exhausted {
            url: url.clone(),
            attempts: self.attempts,
            source: Box::new(last.expect("at least one attempt always runs")),
        })
    }
}

async fn resumable_prefix(path: &Path, integrity: &Integrity) -> Result<Option<u64>> {
    let Some(total) = integrity.size else {
        return Ok(None);
    };
    match tokio::fs::metadata(path).await {
        Ok(meta) if meta.len() > 0 && meta.len() < total => Ok(Some(meta.len())),
        Ok(_) => Ok(None),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

async fn seed_verifier_from_prefix(path: &Path, len: u64, verifier: &mut Verifier) -> Result<()> {
    use tokio::io::AsyncReadExt;

    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let mut remaining = len;
    let mut buffer = vec![0u8; 64 * 1024];

    while remaining > 0 {
        let want = buffer.len().min(remaining as usize);
        let read = file
            .read(&mut buffer[..want])
            .await
            .map_err(|source| Error::Io {
                path: path.to_path_buf(),
                source,
            })?;
        if read == 0 {
            break;
        }
        verifier.update(&buffer[..read]);
        remaining -= read as u64;
    }

    Ok(())
}

async fn open_for_append(path: &Path) -> Result<tokio::fs::File> {
    tokio::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .await
        .map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })
}

async fn create_truncated(path: &Path) -> Result<tokio::fs::File> {
    tokio::fs::File::create(path)
        .await
        .map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })
}

async fn write_all(file: &mut tokio::fs::File, path: &Path, chunk: &[u8]) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    file.write_all(chunk).await.map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

async fn flush(file: &mut tokio::fs::File, path: &Path) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    file.flush().await.map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}
