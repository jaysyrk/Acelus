use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::rules::{Arch, Os};

pub const JAVA_RUNTIME_MANIFEST_URL: &str =
    "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Platform {
    Linux,
    LinuxI386,
    MacOs,
    MacOsArm64,
    WindowsX64,
    WindowsX86,
    WindowsArm64,
}

impl Platform {
    pub const fn as_manifest_key(self) -> &'static str {
        match self {
            Platform::Linux => "linux",
            Platform::LinuxI386 => "linux-i386",
            Platform::MacOs => "mac-os",
            Platform::MacOsArm64 => "mac-os-arm64",
            Platform::WindowsX64 => "windows-x64",
            Platform::WindowsX86 => "windows-x86",
            Platform::WindowsArm64 => "windows-arm64",
        }
    }

    pub fn for_target(os: Os, arch: Arch) -> Option<Self> {
        match (os, arch) {
            (Os::Linux, Arch::X86_64) => Some(Platform::Linux),
            (Os::Linux, Arch::X86) => Some(Platform::LinuxI386),
            (Os::Linux, Arch::Arm64 | Arch::Arm32) => None,
            (Os::MacOs, Arch::X86_64) => Some(Platform::MacOs),
            (Os::MacOs, Arch::Arm64) => Some(Platform::MacOsArm64),
            (Os::MacOs, _) => None,
            (Os::Windows, Arch::X86_64) => Some(Platform::WindowsX64),
            (Os::Windows, Arch::X86) => Some(Platform::WindowsX86),
            (Os::Windows, Arch::Arm64) => Some(Platform::WindowsArm64),
            (Os::Windows, Arch::Arm32) => None,
        }
    }

    pub fn current() -> Option<Self> {
        Self::for_target(Os::current(), Arch::current())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuntimeManifest {
    platforms: BTreeMap<String, BTreeMap<String, Vec<RuntimeEntry>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeEntry {
    pub manifest: ManifestRef,
    pub version: RuntimeVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability: Option<Availability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestRef {
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeVersion {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub released: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Availability {
    #[serde(default)]
    pub group: u32,
    #[serde(default)]
    pub progress: u32,
}

impl RuntimeManifest {
    pub fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    pub fn components_for(
        &self,
        platform: Platform,
    ) -> Option<&BTreeMap<String, Vec<RuntimeEntry>>> {
        self.platforms.get(platform.as_manifest_key())
    }

    pub fn resolve(&self, platform: Platform, component: &str) -> Option<&RuntimeEntry> {
        self.components_for(platform)?.get(component)?.first()
    }

    pub fn available_components(&self, platform: Platform) -> Vec<&str> {
        self.components_for(platform)
            .map(|components| {
                components
                    .iter()
                    .filter(|(_, entries)| !entries.is_empty())
                    .map(|(name, _)| name.as_str())
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeFiles {
    pub files: BTreeMap<String, RuntimeFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RuntimeFile {
    Directory,
    Link {
        target: String,
    },
    File {
        downloads: RuntimeDownloads,
        #[serde(default)]
        executable: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeDownloads {
    pub raw: RuntimeArtifact,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lzma: Option<RuntimeArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeArtifact {
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

impl RuntimeFiles {
    pub fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    pub fn regular_files(&self) -> impl Iterator<Item = (&str, &RuntimeDownloads, bool)> {
        self.files.iter().filter_map(|(path, entry)| match entry {
            RuntimeFile::File {
                downloads,
                executable,
            } => Some((path.as_str(), downloads, *executable)),
            _ => None,
        })
    }

    pub fn directories(&self) -> impl Iterator<Item = &str> {
        self.files.iter().filter_map(|(path, entry)| match entry {
            RuntimeFile::Directory => Some(path.as_str()),
            _ => None,
        })
    }

    pub fn links(&self) -> impl Iterator<Item = (&str, &str)> {
        self.files.iter().filter_map(|(path, entry)| match entry {
            RuntimeFile::Link { target } => Some((path.as_str(), target.as_str())),
            _ => None,
        })
    }

    pub fn total_size(&self) -> u64 {
        self.regular_files()
            .map(|(_, downloads, _)| downloads.raw.size)
            .sum()
    }

    pub fn java_executable(&self, os: Os) -> Option<&str> {
        let wanted = match os {
            Os::Windows => "bin/java.exe",
            Os::MacOs => "jre.bundle/Contents/Home/bin/java",
            Os::Linux => "bin/java",
        };

        if self.files.contains_key(wanted) {
            return Some(wanted);
        }

        self.files
            .keys()
            .find(|path| path.ends_with("bin/java") || path.ends_with("bin/java.exe"))
            .map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_keys_match_mojangs_spelling() {
        assert_eq!(
            Platform::for_target(Os::Linux, Arch::X86_64)
                .unwrap()
                .as_manifest_key(),
            "linux"
        );
        assert_eq!(
            Platform::for_target(Os::MacOs, Arch::Arm64)
                .unwrap()
                .as_manifest_key(),
            "mac-os-arm64"
        );
        assert_eq!(
            Platform::for_target(Os::Windows, Arch::X86_64)
                .unwrap()
                .as_manifest_key(),
            "windows-x64"
        );
        assert_eq!(
            Platform::for_target(Os::Windows, Arch::Arm64)
                .unwrap()
                .as_manifest_key(),
            "windows-arm64"
        );
    }

    #[test]
    fn mojang_publishes_no_runtime_for_linux_on_arm() {
        assert!(Platform::for_target(Os::Linux, Arch::Arm64).is_none());
        assert!(Platform::for_target(Os::Linux, Arch::Arm32).is_none());
    }

    #[test]
    fn runtime_entries_are_classified_by_type() {
        let files = RuntimeFiles::parse(
            br#"{"files":{
                "bin": {"type":"directory"},
                "bin/java": {"type":"file","executable":true,"downloads":{
                    "raw":{"sha1":"aa","size":10,"url":"https://example/raw"},
                    "lzma":{"sha1":"bb","size":4,"url":"https://example/lzma"}}},
                "legal/x": {"type":"link","target":"../y"}
            }}"#,
        )
        .unwrap();

        assert_eq!(files.directories().collect::<Vec<_>>(), vec!["bin"]);
        assert_eq!(files.links().collect::<Vec<_>>(), vec![("legal/x", "../y")]);

        let regular: Vec<_> = files.regular_files().collect();
        assert_eq!(regular.len(), 1);
        assert_eq!(regular[0].0, "bin/java");
        assert!(regular[0].2, "bin/java must keep its executable bit");
        assert_eq!(regular[0].1.raw.url, "https://example/raw");
        assert!(regular[0].1.lzma.is_some());
        assert_eq!(files.total_size(), 10);
    }

    #[test]
    fn the_java_executable_is_found_per_platform() {
        let unix = RuntimeFiles::parse(
            br#"{"files":{"bin/java":{"type":"file","executable":true,"downloads":{"raw":{"sha1":"a","size":1,"url":"https://e"}}}}}"#,
        )
        .unwrap();
        assert_eq!(unix.java_executable(Os::Linux), Some("bin/java"));

        let windows = RuntimeFiles::parse(
            br#"{"files":{"bin/java.exe":{"type":"file","executable":true,"downloads":{"raw":{"sha1":"a","size":1,"url":"https://e"}}}}}"#,
        )
        .unwrap();
        assert_eq!(windows.java_executable(Os::Windows), Some("bin/java.exe"));
    }

    #[test]
    fn a_runtime_without_java_reports_none() {
        let files = RuntimeFiles::parse(br#"{"files":{"README":{"type":"directory"}}}"#).unwrap();
        assert!(files.java_executable(Os::Linux).is_none());
    }
}
