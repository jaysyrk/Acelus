use acelus_meta::arguments::ResolvedArguments;
use acelus_meta::assets::AssetIndex;

use crate::lock::{JavaSource, LockMinecraft, Role};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedArtifact {
    pub path: String,
    pub role: Role,
    pub size: Option<u64>,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
    pub origin: Vec<String>,
    pub extract: Option<Vec<String>>,
}

impl PlannedArtifact {
    pub fn new(path: impl Into<String>, role: Role, url: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            role,
            size: None,
            sha1: None,
            sha512: None,
            origin: vec![url.into()],
            extract: None,
        }
    }

    pub fn with_sha1(mut self, sha1: impl Into<String>) -> Self {
        self.sha1 = Some(sha1.into());
        self
    }

    pub fn with_size(mut self, size: u64) -> Self {
        self.size = Some(size);
        self
    }

    pub fn with_extract(mut self, exclude: Vec<String>) -> Self {
        self.extract = Some(exclude);
        self
    }

    pub fn integrity(&self) -> acelus_net::Integrity {
        let mut integrity = acelus_net::Integrity::none();
        integrity.sha1 = self.sha1.clone();
        integrity.sha512 = self.sha512.clone();
        integrity.size = self.size;
        integrity
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedJava {
    pub component: Option<String>,
    pub major_version: u32,
    pub source: JavaSource,
    pub manifest_url: Option<String>,
    pub manifest_sha1: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Plan {
    pub minecraft: LockMinecraft,
    pub java: PlannedJava,
    pub artifacts: Vec<PlannedArtifact>,
    pub arguments: ResolvedArguments,
    pub asset_index: Option<AssetIndex>,
}

impl Plan {
    pub fn total_download_size(&self) -> u64 {
        let artifacts: u64 = self.artifacts.iter().filter_map(|a| a.size).sum();
        let assets = self
            .asset_index
            .as_ref()
            .map(|index| index.total_size())
            .unwrap_or(0);
        artifacts + assets
    }

    pub fn artifacts_with_role(&self, role: Role) -> impl Iterator<Item = &PlannedArtifact> {
        self.artifacts.iter().filter(move |a| a.role == role)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_artifact_carries_its_vendor_digest_into_the_integrity_check() {
        let artifact =
            PlannedArtifact::new("client.jar", Role::Client, "https://example/client.jar")
                .with_sha1("2dc72797acbc1b63fc16a11c4ac393605f453754")
                .with_size(39193383);

        let integrity = artifact.integrity();
        assert_eq!(
            integrity.sha1.as_deref(),
            Some("2dc72797acbc1b63fc16a11c4ac393605f453754")
        );
        assert_eq!(integrity.size, Some(39193383));
        assert!(integrity.sha512.is_none());
    }

    #[test]
    fn an_artifact_without_a_digest_still_produces_a_usable_integrity() {
        let artifact = PlannedArtifact::new("a.jar", Role::Library, "https://example/a.jar");
        assert!(artifact.integrity().is_empty());
    }

    #[test]
    fn native_extraction_directives_survive_planning() {
        let artifact = PlannedArtifact::new("n.jar", Role::Native, "https://example/n.jar")
            .with_extract(vec!["META-INF/".into()]);
        assert_eq!(
            artifact.extract.as_deref(),
            Some(&["META-INF/".to_string()][..])
        );
    }
}
