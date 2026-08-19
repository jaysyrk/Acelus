use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const RESOURCES_BASE_URL: &str = "https://resources.download.minecraft.net";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetIndex {
    pub objects: BTreeMap<String, AssetObject>,

    #[serde(default, rename = "virtual", skip_serializing_if = "is_false")]
    pub is_virtual: bool,

    #[serde(default, skip_serializing_if = "is_false")]
    pub map_to_resources: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

impl AssetObject {
    pub fn shard(&self) -> &str {
        &self.hash[..2.min(self.hash.len())]
    }

    pub fn store_path(&self) -> String {
        format!("{}/{}", self.shard(), self.hash)
    }

    pub fn url(&self) -> String {
        format!("{RESOURCES_BASE_URL}/{}", self.store_path())
    }
}

impl AssetIndex {
    pub fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    pub fn total_size(&self) -> u64 {
        self.objects.values().map(|object| object.size).sum()
    }

    pub fn unique_objects(&self) -> usize {
        self.objects
            .values()
            .map(|object| object.hash.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len()
    }

    pub fn needs_named_copies(&self) -> bool {
        self.is_virtual || self.map_to_resources
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(json: &str) -> AssetIndex {
        AssetIndex::parse(json.as_bytes()).unwrap()
    }

    #[test]
    fn objects_resolve_to_sharded_paths_and_urls() {
        let index = index(
            r#"{"objects":{"icons/icon_128x128.png":{"hash":"b62ca8ec10d07e6bf5ac8dae0c8c1d2e6a1e3356","size":9101}}}"#,
        );

        let object = &index.objects["icons/icon_128x128.png"];
        assert_eq!(object.shard(), "b6");
        assert_eq!(
            object.store_path(),
            "b6/b62ca8ec10d07e6bf5ac8dae0c8c1d2e6a1e3356"
        );
        assert_eq!(
            object.url(),
            "https://resources.download.minecraft.net/b6/b62ca8ec10d07e6bf5ac8dae0c8c1d2e6a1e3356"
        );
    }

    #[test]
    fn a_modern_index_needs_no_named_copies() {
        let index = index(r#"{"objects":{}}"#);
        assert!(!index.needs_named_copies());
    }

    #[test]
    fn a_virtual_index_needs_named_copies() {
        let index = index(r#"{"objects":{},"virtual":true}"#);
        assert!(index.needs_named_copies());
    }

    #[test]
    fn an_index_mapping_to_resources_needs_named_copies() {
        let index = index(r#"{"objects":{},"map_to_resources":true}"#);
        assert!(index.needs_named_copies());
    }

    #[test]
    fn sizes_and_duplicate_objects_are_counted_correctly() {
        let index = index(
            r#"{"objects":{
                "a":{"hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":10},
                "b":{"hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":10},
                "c":{"hash":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","size":5}
            }}"#,
        );

        assert_eq!(index.objects.len(), 3);
        assert_eq!(index.unique_objects(), 2);
        assert_eq!(index.total_size(), 25);
    }
}
