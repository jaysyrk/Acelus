use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::loader::LoaderRequest;
use crate::paths::{InstanceLayout, Paths};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("i/o failed at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("the instance descriptor at {path} is not valid JSON")]
    Corrupt {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("no instance named {name} exists")]
    NotFound { name: String },

    #[error("an instance named {name} already exists")]
    AlreadyExists { name: String },

    #[error("{name} is not a usable instance name")]
    InvalidName { name: String },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Descriptor {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader: Option<LoaderRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_played: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_megabytes: Option<u32>,
    #[serde(default)]
    pub extra_jvm_arguments: Vec<String>,
}

impl Descriptor {
    pub fn new(id: impl Into<String>, name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            version: version.into(),
            loader: None,
            last_played: None,
            memory_megabytes: None,
            extra_jvm_arguments: Vec::new(),
        }
    }

    pub fn with_loader(mut self, loader: Option<LoaderRequest>) -> Self {
        self.loader = loader;
        self
    }
}

pub fn sanitise_id(name: &str) -> Option<String> {
    let id: String = name
        .trim()
        .chars()
        .map(|c| match c {
            'a'..='z' | '0'..='9' | '-' | '_' => c,
            'A'..='Z' => c.to_ascii_lowercase(),
            ' ' | '.' => '-',
            _ => '\0',
        })
        .filter(|c| *c != '\0')
        .collect();

    let trimmed = id.trim_matches('-').to_string();

    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        None
    } else {
        Some(trimmed)
    }
}

pub struct Instances {
    paths: Paths,
}

impl Instances {
    pub fn new(paths: Paths) -> Self {
        Self { paths }
    }

    pub fn layout(&self, id: &str) -> InstanceLayout {
        InstanceLayout::new(self.paths.instance(id))
    }

    pub fn list(&self) -> Result<Vec<Descriptor>> {
        let root = self.paths.instances();
        let entries = match std::fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(Error::Io { path: root, source }),
        };

        let mut found = Vec::new();
        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let descriptor_path = InstanceLayout::new(entry.path()).descriptor();
            if let Ok(descriptor) = read(&descriptor_path) {
                found.push(descriptor);
            }
        }

        found.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(found)
    }

    pub fn get(&self, id: &str) -> Result<Descriptor> {
        read(&self.layout(id).descriptor()).map_err(|error| match error {
            Error::Io { .. } => Error::NotFound {
                name: id.to_string(),
            },
            other => other,
        })
    }

    pub fn exists(&self, id: &str) -> bool {
        self.layout(id).descriptor().is_file()
    }

    pub fn create(
        &self,
        name: &str,
        version: &str,
        loader: Option<LoaderRequest>,
    ) -> Result<Descriptor> {
        let id = sanitise_id(name).ok_or_else(|| Error::InvalidName {
            name: name.to_string(),
        })?;

        if self.exists(&id) {
            return Err(Error::AlreadyExists { name: id });
        }

        let layout = self.layout(&id);
        std::fs::create_dir_all(layout.game_dir()).map_err(|source| Error::Io {
            path: layout.game_dir(),
            source,
        })?;

        let descriptor = Descriptor::new(&id, name, version).with_loader(loader);
        self.save(&descriptor)?;
        Ok(descriptor)
    }

    pub fn save(&self, descriptor: &Descriptor) -> Result<()> {
        let path = self.layout(&descriptor.id).descriptor();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let json = serde_json::to_vec_pretty(descriptor).map_err(|source| Error::Corrupt {
            path: path.clone(),
            source,
        })?;

        std::fs::write(&path, json).map_err(|source| Error::Io { path, source })
    }

    pub fn delete(&self, id: &str, keep_user_data: bool) -> Result<()> {
        if !self.exists(id) {
            return Err(Error::NotFound {
                name: id.to_string(),
            });
        }

        let layout = self.layout(id);

        if keep_user_data {
            for managed in [
                layout.libraries(),
                layout.natives(),
                layout.client_jar(),
                layout.logging_config(),
                layout.lockfile(),
                layout.descriptor(),
            ] {
                remove(&managed)?;
            }
            return Ok(());
        }

        std::fs::remove_dir_all(layout.root()).map_err(|source| Error::Io {
            path: layout.root().to_path_buf(),
            source,
        })
    }
}

fn remove(path: &Path) -> Result<()> {
    let outcome = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };

    match outcome {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn read(path: &Path) -> Result<Descriptor> {
    let bytes = std::fs::read(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;

    serde_json::from_slice(&bytes).map_err(|source| Error::Corrupt {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_become_safe_directory_identifiers() {
        assert_eq!(sanitise_id("Survival").as_deref(), Some("survival"));
        assert_eq!(sanitise_id("My Pack 1.21").as_deref(), Some("my-pack-1-21"));
        assert_eq!(sanitise_id("all_the_mods").as_deref(), Some("all_the_mods"));
    }

    #[test]
    fn a_name_cannot_escape_the_instances_directory() {
        assert_eq!(sanitise_id("../etc/passwd").as_deref(), Some("etcpasswd"));
        assert_eq!(sanitise_id("..").as_deref(), None);
        assert_eq!(sanitise_id("/").as_deref(), None);
        assert_eq!(sanitise_id("./..").as_deref(), None);

        for hostile in ["../../root", "..\\..\\windows"] {
            let id = sanitise_id(hostile).unwrap();
            assert!(
                !id.contains('/') && !id.contains('\\') && !id.contains(".."),
                "{hostile} sanitised to {id}, which can still traverse"
            );
        }
    }

    #[test]
    fn a_name_with_nothing_usable_is_rejected() {
        assert_eq!(sanitise_id("").as_deref(), None);
        assert_eq!(sanitise_id("   ").as_deref(), None);
        assert_eq!(sanitise_id("!!!").as_deref(), None);
    }
}
