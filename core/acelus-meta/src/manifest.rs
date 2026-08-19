use serde::{Deserialize, Serialize};

use crate::version::ReleaseType;

pub const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifest {
    pub latest: Latest,
    pub versions: Vec<VersionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Latest {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSummary {
    pub id: String,
    #[serde(rename = "type")]
    pub release_type: ReleaseType,
    pub url: String,
    pub time: String,
    pub release_time: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compliance_level: Option<u32>,
}

impl VersionManifest {
    pub fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    pub fn find(&self, id: &str) -> Option<&VersionSummary> {
        self.versions.iter().find(|v| v.id == id)
    }

    pub fn latest_release(&self) -> Option<&VersionSummary> {
        self.find(&self.latest.release)
    }

    pub fn latest_snapshot(&self) -> Option<&VersionSummary> {
        self.find(&self.latest.snapshot)
    }

    pub fn of_type(&self, release_type: ReleaseType) -> impl Iterator<Item = &VersionSummary> {
        self.versions
            .iter()
            .filter(move |v| v.release_type == release_type)
    }
}
