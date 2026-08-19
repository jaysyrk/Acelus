use std::path::{Path, PathBuf};
use std::process::Command;

use acelus_meta::java::{RuntimeDownloads, RuntimeFiles};
use acelus_meta::Os;
use acelus_net::{Fetcher, Integrity};
use futures::StreamExt;
use url::Url;

use crate::extract::safe_join;
use crate::paths::Paths;
use crate::plan::PlannedJava;

pub const MARKER_FILE: &str = ".acelus-runtime";
const FILE_CONCURRENCY: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not fetch {what}")]
    Fetch {
        what: String,
        #[source]
        source: Box<acelus_net::Error>,
    },

    #[error("the Java runtime file listing is not valid metadata")]
    Parse {
        #[source]
        source: serde_json::Error,
    },

    #[error("{url} is not a usable URL")]
    BadUrl { url: String },

    #[error("i/o failed at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("the runtime listing entry {entry} resolves outside the runtime directory")]
    PathEscape { entry: String },

    #[error("the provisioned runtime contains no java executable")]
    NoJavaExecutable,

    #[error(
        "Mojang publishes no Java runtime for this platform. \
         Install Java {major_version} or newer and point Acelus at it, \
         or set the instance to use a system runtime"
    )]
    NoMojangRuntime { major_version: u32 },

    #[error("no system Java runtime of version {required} or newer was found")]
    NoSystemJava { required: u32 },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledRuntime {
    pub root: PathBuf,
    pub java_executable: PathBuf,
    pub major_version: u32,
}

pub struct JavaProvisioner {
    fetcher: Fetcher,
    paths: Paths,
}

impl JavaProvisioner {
    pub fn new(fetcher: Fetcher, paths: Paths) -> Self {
        Self { fetcher, paths }
    }

    pub async fn provision(&self, java: &PlannedJava, os: Os) -> Result<InstalledRuntime> {
        match (&java.manifest_url, &java.component) {
            (Some(url), Some(component)) => {
                self.provision_from_mojang(url, component, java.manifest_sha1.as_deref(), os, java)
                    .await
            }
            _ => Err(Error::NoMojangRuntime {
                major_version: java.major_version,
            }),
        }
    }

    async fn provision_from_mojang(
        &self,
        manifest_url: &str,
        component: &str,
        manifest_sha1: Option<&str>,
        os: Os,
        java: &PlannedJava,
    ) -> Result<InstalledRuntime> {
        let root = self.paths.runtime(component);

        if let Some(sha1) = manifest_sha1 {
            if is_complete(&root, sha1) {
                return finish(&root, os, java.major_version, None);
            }
        }

        let integrity = match manifest_sha1 {
            Some(sha1) => Integrity::sha1(sha1.to_string()),
            None => Integrity::none(),
        };

        let bytes = self
            .get(manifest_url, "the Java runtime file listing", &integrity)
            .await?;

        let listing = RuntimeFiles::parse(&bytes).map_err(|source| Error::Parse { source })?;

        for directory in listing.directories() {
            let target = resolve(&root, directory)?;
            std::fs::create_dir_all(&target).map_err(|source| Error::Io {
                path: target,
                source,
            })?;
        }

        self.download_files(&listing, &root).await?;
        create_links(&listing, &root)?;

        if let Some(sha1) = manifest_sha1 {
            mark_complete(&root, sha1)?;
        }

        finish(&root, os, java.major_version, Some(&listing))
    }

    async fn download_files(&self, listing: &RuntimeFiles, root: &Path) -> Result<()> {
        let entries: Vec<(String, RuntimeDownloads, bool)> = listing
            .regular_files()
            .map(|(path, downloads, executable)| (path.to_string(), downloads.clone(), executable))
            .collect();

        futures::stream::iter(entries.into_iter().map(
            |(path, downloads, executable)| async move {
                let target = resolve(root, &path)?;

                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }

                let integrity =
                    Integrity::sha1(downloads.raw.sha1.clone()).with_size(downloads.raw.size);

                if !is_present_and_sized(&target, downloads.raw.size) {
                    let url: Url = downloads.raw.url.parse().map_err(|_| Error::BadUrl {
                        url: downloads.raw.url.clone(),
                    })?;

                    self.fetcher
                        .to_file(&url, &integrity, &target)
                        .await
                        .map_err(|source| Error::Fetch {
                            what: path.clone(),
                            source: Box::new(source),
                        })?;
                }

                if executable {
                    set_executable(&target)?;
                }

                Ok::<(), Error>(())
            },
        ))
        .buffer_unordered(FILE_CONCURRENCY)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<()>>>()?;

        Ok(())
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

fn resolve(root: &Path, entry: &str) -> Result<PathBuf> {
    safe_join(root, entry).ok_or(Error::PathEscape {
        entry: entry.to_string(),
    })
}

fn finish(
    root: &Path,
    os: Os,
    major_version: u32,
    listing: Option<&RuntimeFiles>,
) -> Result<InstalledRuntime> {
    let relative = match listing {
        Some(listing) => listing
            .java_executable(os)
            .map(str::to_string)
            .ok_or(Error::NoJavaExecutable)?,
        None => discover_java_executable(root, os).ok_or(Error::NoJavaExecutable)?,
    };

    let java_executable = resolve(root, &relative)?;
    if !java_executable.is_file() {
        return Err(Error::NoJavaExecutable);
    }

    Ok(InstalledRuntime {
        root: root.to_path_buf(),
        java_executable,
        major_version,
    })
}

fn discover_java_executable(root: &Path, os: Os) -> Option<String> {
    let candidates = match os {
        Os::Windows => ["bin/java.exe"].as_slice(),
        Os::MacOs => ["jre.bundle/Contents/Home/bin/java", "bin/java"].as_slice(),
        Os::Linux => ["bin/java"].as_slice(),
    };

    candidates
        .iter()
        .find(|relative| root.join(relative).is_file())
        .map(|relative| relative.to_string())
}

fn is_present_and_sized(path: &Path, size: u64) -> bool {
    std::fs::metadata(path)
        .map(|meta| meta.is_file() && meta.len() == size)
        .unwrap_or(false)
}

fn is_complete(root: &Path, manifest_sha1: &str) -> bool {
    std::fs::read_to_string(root.join(MARKER_FILE))
        .map(|recorded| recorded.trim() == manifest_sha1)
        .unwrap_or(false)
}

fn mark_complete(root: &Path, manifest_sha1: &str) -> Result<()> {
    let path = root.join(MARKER_FILE);
    std::fs::write(&path, manifest_sha1).map_err(|source| Error::Io { path, source })
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::metadata(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;

    let mut permissions = metadata.permissions();
    permissions.set_mode(permissions.mode() | 0o111);

    std::fs::set_permissions(path, permissions).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn create_links(listing: &RuntimeFiles, root: &Path) -> Result<()> {
    for (entry, target) in listing.links() {
        let link = resolve(root, entry)?;

        if let Some(parent) = link.parent() {
            std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let _ = std::fs::remove_file(&link);

        std::os::unix::fs::symlink(target, &link)
            .map_err(|source| Error::Io { path: link, source })?;
    }

    Ok(())
}

#[cfg(not(unix))]
fn create_links(listing: &RuntimeFiles, root: &Path) -> Result<()> {
    for (entry, target) in listing.links() {
        let link = resolve(root, entry)?;

        let Some(parent) = link.parent() else {
            continue;
        };
        std::fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.to_path_buf(),
            source,
        })?;

        let source_path = resolve(parent, target)?;
        if source_path.is_file() {
            let _ = std::fs::remove_file(&link);
            std::fs::copy(&source_path, &link)
                .map_err(|source| Error::Io { path: link, source })?;
        }
    }

    Ok(())
}

pub fn parse_java_major(version_output: &str) -> Option<u32> {
    let quoted = version_output.split('"').nth(1)?;
    let mut parts = quoted.split(['.', '_', '-']);
    let first: u32 = parts.next()?.parse().ok()?;

    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

pub fn discover_system_java(required_major: u32) -> Result<InstalledRuntime> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(home) = std::env::var("JAVA_HOME") {
        if !home.is_empty() {
            candidates.push(Path::new(&home).join("bin").join(executable_name()));
        }
    }
    candidates.push(PathBuf::from(executable_name()));

    for candidate in candidates {
        let Ok(output) = Command::new(&candidate).arg("-version").output() else {
            continue;
        };

        let reported = String::from_utf8_lossy(&output.stderr);
        let Some(major) = parse_java_major(&reported) else {
            continue;
        };

        if major >= required_major {
            return Ok(InstalledRuntime {
                root: candidate
                    .parent()
                    .and_then(Path::parent)
                    .unwrap_or(Path::new("."))
                    .to_path_buf(),
                java_executable: candidate,
                major_version: major,
            });
        }
    }

    Err(Error::NoSystemJava {
        required: required_major,
    })
}

const fn executable_name() -> &'static str {
    if cfg!(windows) {
        "java.exe"
    } else {
        "java"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modern_version_strings_report_their_major_version() {
        assert_eq!(
            parse_java_major(r#"openjdk version "21.0.1" 2023-10-17"#),
            Some(21)
        );
        assert_eq!(
            parse_java_major(r#"openjdk version "25" 2025-09-16"#),
            Some(25)
        );
        assert_eq!(
            parse_java_major(r#"java version "17.0.9" 2023-10-17 LTS"#),
            Some(17)
        );
    }

    #[test]
    fn legacy_version_strings_report_eight_rather_than_one() {
        assert_eq!(
            parse_java_major(r#"java version "1.8.0_392""#),
            Some(8),
            "1.8 is Java 8, not Java 1"
        );
        assert_eq!(parse_java_major(r#"openjdk version "1.7.0_262""#), Some(7));
    }

    #[test]
    fn unparseable_output_reports_nothing_rather_than_guessing() {
        assert_eq!(parse_java_major("command not found"), None);
        assert_eq!(parse_java_major(""), None);
        assert_eq!(parse_java_major(r#"openjdk version "" "#), None);
    }

    #[test]
    fn a_runtime_listing_entry_cannot_escape_its_directory() {
        let root = Path::new("/runtimes/java-runtime-delta");
        assert!(resolve(root, "bin/java").is_ok());
        assert!(matches!(
            resolve(root, "../../etc/passwd"),
            Err(Error::PathEscape { .. })
        ));
    }

    #[test]
    fn completion_is_recorded_against_the_manifest_digest() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let sha1 = "a".repeat(40);

        assert!(!is_complete(root, &sha1));
        mark_complete(root, &sha1).unwrap();
        assert!(is_complete(root, &sha1));

        assert!(
            !is_complete(root, &"b".repeat(40)),
            "a runtime installed from a different listing must be reprovisioned"
        );
    }
}
