use std::path::{Path, PathBuf};

use acelus_meta::LoaderKind;

use crate::loader::LoaderRequest;

const MANAGED: &[&str] = &[
    "assets",
    "libraries",
    "versions",
    "natives",
    "bin",
    ".fabric",
    "logs",
];

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path} does not look like a Prism or MultiMC instance")]
    NotAnInstance { path: PathBuf },

    #[error("the pack description at {path} is not valid JSON")]
    Corrupt {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("i/o failed at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Foreign {
    pub path: PathBuf,
    pub name: String,
    pub version: String,
    pub loader: Option<LoaderRequest>,
    pub blocked_by: Option<String>,
    pub game_dir: PathBuf,
    pub memory_megabytes: Option<u32>,
    pub jvm_arguments: Vec<String>,
}

impl Foreign {
    pub fn is_importable(&self) -> bool {
        self.blocked_by.is_none()
    }
}

pub fn search_paths(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".local/share/PrismLauncher/instances"),
        home.join(".var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher/instances"),
        home.join(".local/share/multimc/instances"),
        home.join(".local/share/PolyMC/instances"),
        home.join("AppData/Roaming/PrismLauncher/instances"),
        home.join("Library/Application Support/PrismLauncher/instances"),
    ]
}

pub fn scan(root: &Path) -> Vec<Foreign> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };

    let mut found: Vec<Foreign> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| read(&entry.path()).ok())
        .collect();

    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

pub fn read(directory: &Path) -> Result<Foreign> {
    let pack = directory.join("mmc-pack.json");
    if !pack.is_file() {
        return Err(Error::NotAnInstance {
            path: directory.to_path_buf(),
        });
    }

    let bytes = std::fs::read(&pack).map_err(|source| Error::Io {
        path: pack.clone(),
        source,
    })?;

    let parsed: Pack = serde_json::from_slice(&bytes).map_err(|source| Error::Corrupt {
        path: pack.clone(),
        source,
    })?;

    let version = parsed
        .component("net.minecraft")
        .map(|component| component.version.clone())
        .ok_or_else(|| Error::NotAnInstance {
            path: directory.to_path_buf(),
        })?;

    let mut loader = None;
    let mut blocked_by = None;

    if let Some(component) = parsed.component("net.fabricmc.fabric-loader") {
        loader = Some(LoaderRequest {
            kind: LoaderKind::Fabric,
            version: Some(component.version.clone()),
        });
    } else if let Some(component) = parsed.component("org.quiltmc.quilt-loader") {
        loader = Some(LoaderRequest {
            kind: LoaderKind::Quilt,
            version: Some(component.version.clone()),
        });
    } else if parsed.component("net.neoforged").is_some() {
        blocked_by = Some("NeoForge".to_string());
    } else if parsed.component("net.minecraftforge").is_some() {
        blocked_by = Some("Forge".to_string());
    }

    let game_dir = ["minecraft", ".minecraft"]
        .iter()
        .map(|name| directory.join(name))
        .find(|candidate| candidate.is_dir())
        .unwrap_or_else(|| directory.join(".minecraft"));

    let settings = settings(directory);

    Ok(Foreign {
        name: settings
            .get("name")
            .cloned()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| folder_name(directory)),
        memory_megabytes: settings
            .get("MaxMemAlloc")
            .and_then(|value| value.parse().ok()),
        jvm_arguments: settings
            .get("JvmArgs")
            .map(|value| split_arguments(value))
            .unwrap_or_default(),
        version,
        loader,
        blocked_by,
        game_dir,
        path: directory.to_path_buf(),
    })
}

fn folder_name(directory: &Path) -> String {
    directory
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Imported".to_string())
}

fn settings(directory: &Path) -> std::collections::HashMap<String, String> {
    let Ok(text) = std::fs::read_to_string(directory.join("instance.cfg")) else {
        return std::collections::HashMap::new();
    };

    text.lines()
        .filter(|line| !line.starts_with('['))
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_string(), unquote(value.trim())))
        .collect()
}

fn unquote(value: &str) -> String {
    let trimmed = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(value);
    trimmed.replace("\\\"", "\"")
}

fn split_arguments(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .map(str::to_string)
        .filter(|argument| !argument.is_empty())
        .collect()
}

pub fn carry_over(from: &Path, into: &Path) -> Result<u64> {
    let entries = match std::fs::read_dir(from) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(source) => {
            return Err(Error::Io {
                path: from.to_path_buf(),
                source,
            })
        }
    };

    std::fs::create_dir_all(into).map_err(|source| Error::Io {
        path: into.to_path_buf(),
        source,
    })?;

    let mut copied = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if MANAGED.contains(&name.to_string_lossy().as_ref()) {
            continue;
        }
        copied += copy_tree(&entry.path(), &into.join(&name))?;
    }

    Ok(copied)
}

fn copy_tree(from: &Path, into: &Path) -> Result<u64> {
    let metadata = std::fs::symlink_metadata(from).map_err(|source| Error::Io {
        path: from.to_path_buf(),
        source,
    })?;

    if metadata.is_symlink() {
        return Ok(0);
    }

    if metadata.is_file() {
        if let Some(parent) = into.parent() {
            std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        if reflink_copy::reflink_or_copy(from, into).is_err() {
            std::fs::copy(from, into).map_err(|source| Error::Io {
                path: into.to_path_buf(),
                source,
            })?;
        }
        return Ok(metadata.len());
    }

    if !metadata.is_dir() {
        return Ok(0);
    }

    std::fs::create_dir_all(into).map_err(|source| Error::Io {
        path: into.to_path_buf(),
        source,
    })?;

    let entries = std::fs::read_dir(from).map_err(|source| Error::Io {
        path: from.to_path_buf(),
        source,
    })?;

    let mut copied = 0;
    for entry in entries.flatten() {
        copied += copy_tree(&entry.path(), &into.join(entry.file_name()))?;
    }
    Ok(copied)
}

#[derive(Debug, serde::Deserialize)]
struct Pack {
    #[serde(default)]
    components: Vec<Component>,
}

#[derive(Debug, serde::Deserialize)]
struct Component {
    uid: String,
    #[serde(default)]
    version: String,
}

impl Pack {
    fn component(&self, uid: &str) -> Option<&Component> {
        self.components.iter().find(|held| held.uid == uid)
    }
}
