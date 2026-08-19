use std::collections::HashSet;
use std::fs;

use acelus_cas::{Error, Hash, Link, Materialize, Store};

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("cas")).unwrap();
    (dir, store)
}

#[test]
fn insert_is_content_addressed_and_idempotent() {
    let (_dir, store) = store();

    let first = store.insert_bytes(b"minecraft").unwrap();
    let second = store.insert_bytes(b"minecraft").unwrap();

    assert_eq!(first, second);
    assert_eq!(first, Hash::from(blake3::hash(b"minecraft")));
    assert!(store.contains(&first));
    assert_eq!(store.read(&first).unwrap(), b"minecraft");
}

#[test]
fn distinct_content_yields_distinct_objects() {
    let (_dir, store) = store();

    let a = store.insert_bytes(b"fabric").unwrap();
    let b = store.insert_bytes(b"neoforge").unwrap();

    assert_ne!(a, b);
    assert_eq!(store.size_of(&a).unwrap(), 6);
    assert_eq!(store.size_of(&b).unwrap(), 8);
}

#[test]
fn missing_objects_are_reported_not_panicked() {
    let (_dir, store) = store();
    let absent = Hash::from(blake3::hash(b"never inserted"));

    assert!(!store.contains(&absent));
    assert!(matches!(store.read(&absent), Err(Error::Missing { .. })));
    assert!(matches!(store.verify(&absent), Err(Error::Missing { .. })));
    assert!(matches!(store.size_of(&absent), Err(Error::Missing { .. })));
}

#[test]
fn verify_detects_corruption_on_disk() {
    let (_dir, store) = store();
    let hash = store.insert_bytes(b"client.jar").unwrap();
    store.verify(&hash).unwrap();

    let path = store.object_path(&hash);
    let mut perms = fs::metadata(&path).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o644);
    }
    #[cfg(not(unix))]
    perms.set_readonly(false);
    fs::set_permissions(&path, perms).unwrap();
    fs::write(&path, b"tampered").unwrap();

    match store.verify(&hash) {
        Err(Error::Corrupt { expected, actual }) => {
            assert_eq!(expected, hash);
            assert_ne!(actual, hash);
        }
        other => panic!("expected corruption to be detected, got {other:?}"),
    }
}

#[test]
fn many_instances_share_one_physical_copy() {
    let (dir, store) = store();
    let payload = vec![7u8; 64 * 1024];
    let hash = store.insert_bytes(&payload).unwrap();

    let instances = 20;
    let mut links = Vec::new();
    for n in 0..instances {
        let dest = dir
            .path()
            .join(format!("instance-{n}/libraries/shared.jar"));
        links.push(
            store
                .materialize(&hash, &dest, Materialize::Shared)
                .unwrap(),
        );
        assert_eq!(fs::read(&dest).unwrap(), payload);
    }

    assert!(
        links.iter().all(|l| *l != Link::Copy),
        "shared materialization fell back to full copies: {links:?}"
    );

    #[cfg(unix)]
    if links.iter().all(|l| *l == Link::Hardlink) {
        use std::os::unix::fs::MetadataExt;
        let object = fs::metadata(store.object_path(&hash)).unwrap();
        assert_eq!(object.nlink(), instances + 1);
    }
}

#[test]
fn shared_materialization_is_read_only() {
    let (dir, store) = store();
    let hash = store.insert_bytes(b"library").unwrap();
    let dest = dir.path().join("instance/libraries/a.jar");

    store
        .materialize(&hash, &dest, Materialize::Shared)
        .unwrap();

    assert!(fs::metadata(&dest).unwrap().permissions().readonly());
}

#[test]
fn private_materialization_is_writable_and_independent() {
    let (dir, store) = store();
    let hash = store.insert_bytes(b"options:1").unwrap();
    let dest = dir.path().join("instance/options.txt");

    store
        .materialize(&hash, &dest, Materialize::Private)
        .unwrap();
    fs::write(&dest, b"options:2").unwrap();

    assert_eq!(store.read(&hash).unwrap(), b"options:1");
    assert_eq!(fs::read(&dest).unwrap(), b"options:2");
}

#[test]
fn materialize_replaces_an_existing_destination() {
    let (dir, store) = store();
    let old = store.insert_bytes(b"old").unwrap();
    let new = store.insert_bytes(b"new").unwrap();
    let dest = dir.path().join("instance/mod.jar");

    store.materialize(&old, &dest, Materialize::Shared).unwrap();
    store.materialize(&new, &dest, Materialize::Shared).unwrap();

    assert_eq!(fs::read(&dest).unwrap(), b"new");
}

#[test]
fn gc_removes_only_unreferenced_objects() {
    let (_dir, store) = store();
    let keep = store.insert_bytes(b"still referenced").unwrap();
    let drop_a = store.insert_bytes(b"orphan a").unwrap();
    let drop_b = store.insert_bytes(b"orphan b").unwrap();

    let live = HashSet::from([keep]);
    let stats = store.gc(&live).unwrap();

    assert_eq!(stats.retained_objects, 1);
    assert_eq!(stats.removed_objects, 2);
    assert_eq!(stats.removed_bytes, 16);
    assert!(store.contains(&keep));
    assert!(!store.contains(&drop_a));
    assert!(!store.contains(&drop_b));
}

#[test]
fn gc_on_an_empty_store_is_a_no_op() {
    let (_dir, store) = store();
    let stats = store.gc(&HashSet::new()).unwrap();
    assert_eq!(stats, Default::default());
}

#[test]
fn insert_path_matches_insert_bytes() {
    let (dir, store) = store();
    let source = dir.path().join("client.jar");
    fs::write(&source, b"jar contents").unwrap();

    let from_path = store.insert_path(&source).unwrap();
    let from_bytes = store.insert_bytes(b"jar contents").unwrap();

    assert_eq!(from_path, from_bytes);
}

#[test]
fn abandoned_writers_leave_nothing_behind() {
    let (_dir, store) = store();
    let temp = {
        let writer = store.writer().unwrap();
        writer.temp_path().to_path_buf()
    };
    assert!(!temp.exists());
}
