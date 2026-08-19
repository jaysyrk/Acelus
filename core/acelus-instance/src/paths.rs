use std::path::{Path, PathBuf};

pub const STORE_DIR: &str = "store";
pub const ASSETS_DIR: &str = "assets";
pub const RUNTIMES_DIR: &str = "runtimes";
pub const INSTANCES_DIR: &str = "instances";
pub const CACHE_DIR: &str = "cache";
pub const LOCKFILE_NAME: &str = "acelus.lock";
pub const INSTANCE_FILE_NAME: &str = "instance.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    root: PathBuf,
}

impl Paths {
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn discover() -> Option<Self> {
        Self::from_home_override(std::env::var("ACELUS_HOME").ok().as_deref())
    }

    pub fn from_home_override(explicit: Option<&str>) -> Option<Self> {
        match explicit {
            Some(value) if !value.trim().is_empty() => Some(Self::with_root(value)),
            _ => dirs::data_dir().map(|dir| Self::with_root(dir.join("acelus"))),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn store(&self) -> PathBuf {
        self.root.join(STORE_DIR)
    }

    pub fn assets(&self) -> PathBuf {
        self.root.join(ASSETS_DIR)
    }

    pub fn asset_objects(&self) -> PathBuf {
        self.assets().join("objects")
    }

    pub fn asset_indexes(&self) -> PathBuf {
        self.assets().join("indexes")
    }

    pub fn runtimes(&self) -> PathBuf {
        self.root.join(RUNTIMES_DIR)
    }

    pub fn runtime(&self, component: &str) -> PathBuf {
        self.runtimes().join(component)
    }

    pub fn instances(&self) -> PathBuf {
        self.root.join(INSTANCES_DIR)
    }

    pub fn instance(&self, id: &str) -> PathBuf {
        self.instances().join(id)
    }

    pub fn cache(&self) -> PathBuf {
        self.root.join(CACHE_DIR)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceLayout {
    root: PathBuf,
}

impl InstanceLayout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn lockfile(&self) -> PathBuf {
        self.root.join(LOCKFILE_NAME)
    }

    pub fn descriptor(&self) -> PathBuf {
        self.root.join(INSTANCE_FILE_NAME)
    }

    pub fn game_dir(&self) -> PathBuf {
        self.root.join("minecraft")
    }

    pub fn libraries(&self) -> PathBuf {
        self.root.join("libraries")
    }

    pub fn natives(&self) -> PathBuf {
        self.root.join("natives")
    }

    pub fn client_jar(&self) -> PathBuf {
        self.root.join("client.jar")
    }

    pub fn logging_config(&self) -> PathBuf {
        self.root.join("logging")
    }

    pub fn resolve(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_storage_lives_outside_any_instance() {
        let paths = Paths::with_root("/data/acelus");

        assert_eq!(paths.store(), Path::new("/data/acelus/store"));
        assert_eq!(
            paths.asset_objects(),
            Path::new("/data/acelus/assets/objects")
        );
        assert_eq!(
            paths.runtime("java-runtime-delta"),
            Path::new("/data/acelus/runtimes/java-runtime-delta")
        );

        assert!(
            !paths.store().starts_with(paths.instances()),
            "shared objects must not live inside an instance directory"
        );
        assert!(!paths.assets().starts_with(paths.instances()));
        assert!(!paths.runtimes().starts_with(paths.instances()));
    }

    #[test]
    fn instances_are_addressed_by_id() {
        let paths = Paths::with_root("/data/acelus");
        assert_eq!(
            paths.instance("survival"),
            Path::new("/data/acelus/instances/survival")
        );
    }

    #[test]
    fn an_instance_keeps_game_data_separate_from_managed_files() {
        let layout = InstanceLayout::new("/data/acelus/instances/survival");

        assert_eq!(
            layout.lockfile(),
            Path::new("/data/acelus/instances/survival/acelus.lock")
        );
        assert_eq!(
            layout.game_dir(),
            Path::new("/data/acelus/instances/survival/minecraft")
        );

        for managed in [layout.libraries(), layout.natives(), layout.client_jar()] {
            assert!(
                !managed.starts_with(layout.game_dir()),
                "{managed:?} is managed by Acelus and must not sit inside the game directory"
            );
        }
    }

    #[test]
    fn an_explicit_home_overrides_the_platform_default() {
        let paths = Paths::from_home_override(Some("/tmp/acelus-test")).unwrap();
        assert_eq!(paths.root(), Path::new("/tmp/acelus-test"));
    }

    #[test]
    fn a_blank_home_override_falls_back_to_the_platform_default() {
        for blank in [None, Some(""), Some("   ")] {
            let paths = Paths::from_home_override(blank);
            if let Some(paths) = paths {
                assert!(
                    paths.root().ends_with("acelus"),
                    "the fallback should be a platform data directory, got {:?}",
                    paths.root()
                );
            }
        }
    }
}
