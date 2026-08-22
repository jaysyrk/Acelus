use std::path::Path;

use acelus_instance::import::{self, Foreign};
use acelus_meta::LoaderKind;

const FABRIC_PACK: &str = r#"{
    "components": [
        {"cachedName":"LWJGL 3","cachedVersion":"3.3.3","dependencyOnly":true,"uid":"org.lwjgl3","version":"3.3.3"},
        {"cachedName":"Minecraft","cachedVersion":"1.21.11","important":true,"uid":"net.minecraft","version":"1.21.11"},
        {"cachedName":"Intermediary Mappings","dependencyOnly":true,"uid":"net.fabricmc.intermediary","version":"1.21.11"},
        {"cachedName":"Fabric Loader","uid":"net.fabricmc.fabric-loader","version":"0.18.4"}
    ],
    "formatVersion": 1
}"#;

const FORGE_PACK: &str = r#"{
    "components": [
        {"uid":"net.minecraft","version":"1.20.1"},
        {"uid":"net.minecraftforge","version":"47.3.0"}
    ],
    "formatVersion": 1
}"#;

fn instance(root: &Path, folder: &str, pack: &str, name: Option<&str>) -> std::path::PathBuf {
    let directory = root.join(folder);
    std::fs::create_dir_all(directory.join(".minecraft")).unwrap();
    std::fs::write(directory.join("mmc-pack.json"), pack).unwrap();
    if let Some(name) = name {
        std::fs::write(
            directory.join("instance.cfg"),
            format!("InstanceType=OneSix\nname={name}\nJavaPath=/usr/bin/java\n"),
        )
        .unwrap();
    }
    directory
}

#[test]
fn a_fabric_instance_carries_its_version_loader_and_display_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = instance(dir.path(), "main", FABRIC_PACK, Some("My Modded World"));

    let found = import::read(&path).unwrap();

    assert_eq!(found.name, "My Modded World");
    assert_eq!(found.version, "1.21.11");

    let loader = found
        .loader
        .clone()
        .expect("a fabric instance names its loader");
    assert_eq!(loader.kind, LoaderKind::Fabric);
    assert_eq!(
        loader.version.as_deref(),
        Some("0.18.4"),
        "the exact loader build has to come across, or the mods may not load"
    );
    assert!(found.is_importable());
}

#[test]
fn the_folder_name_stands_in_when_no_display_name_is_configured() {
    let dir = tempfile::tempdir().unwrap();
    let path = instance(dir.path(), "unnamed", FABRIC_PACK, None);
    assert_eq!(import::read(&path).unwrap().name, "unnamed");
}

#[test]
fn a_forge_instance_is_refused_by_name_rather_than_half_imported() {
    let dir = tempfile::tempdir().unwrap();
    let path = instance(dir.path(), "forged", FORGE_PACK, Some("Big Pack"));

    let found = import::read(&path).unwrap();
    assert!(!found.is_importable());
    assert_eq!(found.blocked_by.as_deref(), Some("Forge"));
}

#[test]
fn a_directory_that_is_not_an_instance_is_not_mistaken_for_one() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("notes")).unwrap();
    assert!(import::read(&dir.path().join("notes")).is_err());
}

#[test]
fn scanning_finds_every_instance_and_orders_them() {
    let dir = tempfile::tempdir().unwrap();
    instance(dir.path(), "b", FABRIC_PACK, Some("Zebra"));
    instance(dir.path(), "a", FABRIC_PACK, Some("Apple"));
    std::fs::create_dir_all(dir.path().join("stray")).unwrap();

    let found: Vec<String> = import::scan(dir.path())
        .iter()
        .map(|held: &Foreign| held.name.clone())
        .collect();

    assert_eq!(found, vec!["Apple", "Zebra"]);
}

#[test]
fn player_files_come_across_and_managed_directories_do_not() {
    let dir = tempfile::tempdir().unwrap();
    let from = dir.path().join("from");
    let into = dir.path().join("into");

    std::fs::create_dir_all(from.join("saves/My World")).unwrap();
    std::fs::write(from.join("saves/My World/level.dat"), b"world bytes").unwrap();
    std::fs::create_dir_all(from.join("mods")).unwrap();
    std::fs::write(from.join("mods/sodium.jar"), b"mod bytes").unwrap();
    std::fs::write(from.join("options.txt"), b"fov:80").unwrap();

    for managed in ["assets", "libraries", "versions", "logs"] {
        std::fs::create_dir_all(from.join(managed)).unwrap();
        std::fs::write(from.join(managed).join("junk"), b"managed").unwrap();
    }

    let copied = import::carry_over(&from, &into).unwrap();

    assert!(into.join("saves/My World/level.dat").is_file());
    assert!(into.join("mods/sodium.jar").is_file());
    assert!(into.join("options.txt").is_file());
    assert!(copied > 0);

    for managed in ["assets", "libraries", "versions", "logs"] {
        assert!(
            !into.join(managed).exists(),
            "{managed} is Acelus's to manage and must not be copied out of another launcher"
        );
    }

    assert!(
        from.join("saves/My World/level.dat").is_file(),
        "the original instance must survive, so the other launcher still works"
    );
}

const REAL_CONFIG: &str = r#"[General]
AutoCloseConsole=false
ConfigVersion=1.3
InstanceType=OneSix
JavaPath=/home/jaysyrk/.var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher/java/java-runtime-delta/bin/java
JavaVersion=21.0.7
JvmArgs="-Dglfw.library.path=/usr/lib/libglfw.so  -XX:+UseZGC -XX:+ZGenerational"
MaxMemAlloc=6144
MinMemAlloc=6144
OnlineFixes=false
name=main
"#;

#[test]
fn memory_and_jvm_flags_come_across_but_the_other_launchers_java_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let path = instance(dir.path(), "main", FABRIC_PACK, None);
    std::fs::write(path.join("instance.cfg"), REAL_CONFIG).unwrap();

    let found = import::read(&path).unwrap();

    assert_eq!(found.name, "main");
    assert_eq!(found.memory_megabytes, Some(6144));
    assert_eq!(
        found.jvm_arguments,
        vec![
            "-Dglfw.library.path=/usr/lib/libglfw.so",
            "-XX:+UseZGC",
            "-XX:+ZGenerational"
        ],
        "tuning flags are the reason an instance runs well and have to survive the move"
    );

    assert!(
        !found.jvm_arguments.iter().any(|flag| flag.contains("java")),
        "the other launcher's java path must not follow, since Acelus provisions its own"
    );
}
