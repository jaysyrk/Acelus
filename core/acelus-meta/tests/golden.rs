use acelus_meta::rules::Features;
use acelus_meta::version::LEGACY_JVM_ARGUMENTS;
use acelus_meta::{Arch, Environment, Os, ReleaseType, VersionJson, VersionManifest};

const MODERN: &[u8] = include_bytes!("fixtures/26.2.json");
const LEGACY: &[u8] = include_bytes!("fixtures/1.12.2.json");
const MANIFEST: &[u8] = include_bytes!("fixtures/version_manifest_v2.json");

fn modern() -> VersionJson {
    VersionJson::parse(MODERN).expect("modern manifest parses")
}

fn legacy() -> VersionJson {
    VersionJson::parse(LEGACY).expect("legacy manifest parses")
}

fn env(os: Os, arch: Arch) -> Environment {
    Environment::new(os, arch)
}

#[test]
fn parses_a_modern_version_manifest() {
    let version = modern();
    assert_eq!(version.id, "26.2");
    assert_eq!(version.release_type, ReleaseType::Release);
    assert_eq!(version.main_class, "net.minecraft.client.main.Main");
    assert_eq!(version.assets.as_deref(), Some("32"));

    let java = version
        .java_version
        .expect("modern versions declare a runtime");
    assert_eq!(java.component, "java-runtime-epsilon");
    assert_eq!(java.major_version, 25);

    let logging = version
        .logging
        .and_then(|l| l.client)
        .expect("logging config");
    assert_eq!(logging.argument, "-Dlog4j.configurationFile=${path}");
}

#[test]
fn parses_a_legacy_version_manifest() {
    let version = legacy();
    assert_eq!(version.id, "1.12.2");
    assert!(version.arguments.is_none());
    assert!(version.minecraft_arguments.is_some());

    let java = version
        .java_version
        .expect("legacy versions still declare a runtime");
    assert_eq!(java.component, "jre-legacy");
    assert_eq!(java.major_version, 8);
}

#[test]
fn modern_natives_are_classifier_named_libraries_on_the_classpath() {
    let version = modern();

    let linux = version.resolve_libraries(&env(Os::Linux, Arch::X86_64));
    let windows = version.resolve_libraries(&env(Os::Windows, Arch::X86_64));

    assert!(
        linux.natives.is_empty(),
        "modern versions ship no extractable natives"
    );

    let carries_linux_native = |libs: &acelus_meta::ResolvedLibraries| {
        libs.classpath
            .iter()
            .any(|a| a.name == "com.mojang:jtracy:1.0.37:natives-linux")
    };
    assert!(
        carries_linux_native(&linux),
        "the linux native jar belongs on the linux classpath"
    );
    assert!(
        !carries_linux_native(&windows),
        "the linux native jar must not appear on the windows classpath"
    );
}

#[test]
fn platform_specific_libraries_are_selected_by_their_rules() {
    let version = modern();
    let objc = |os, arch| {
        version
            .resolve_libraries(&env(os, arch))
            .classpath
            .iter()
            .any(|a| a.name.starts_with("ca.weblite:java-objc-bridge"))
    };

    assert!(
        objc(Os::MacOs, Arch::Arm64),
        "the objc bridge is macOS only"
    );
    assert!(!objc(Os::Linux, Arch::X86_64));
    assert!(!objc(Os::Windows, Arch::X86_64));
}

#[test]
fn modern_classpath_differs_per_platform() {
    let version = modern();
    assert_eq!(
        version
            .resolve_libraries(&env(Os::Linux, Arch::X86_64))
            .classpath
            .len(),
        68
    );
    assert_eq!(
        version
            .resolve_libraries(&env(Os::Windows, Arch::X86_64))
            .classpath
            .len(),
        88
    );
    assert_eq!(
        version
            .resolve_libraries(&env(Os::MacOs, Arch::Arm64))
            .classpath
            .len(),
        83
    );
}

#[test]
fn legacy_natives_are_extracted_from_classifiers() {
    let version = legacy();
    let linux = version.resolve_libraries(&env(Os::Linux, Arch::X86_64));

    assert_eq!(linux.natives.len(), 3);
    assert_eq!(linux.classpath.len(), 32);

    for native in &linux.natives {
        assert!(
            native.artifact.path.contains("natives-linux"),
            "expected a linux classifier, got {}",
            native.artifact.path
        );
        assert_eq!(native.exclude, vec!["META-INF/"]);
    }
}

#[test]
fn legacy_natives_follow_the_target_platform() {
    let version = legacy();

    for (os, marker) in [
        (Os::Linux, "natives-linux"),
        (Os::Windows, "natives-windows"),
        (Os::MacOs, "natives-osx"),
    ] {
        let resolved = version.resolve_libraries(&env(os, Arch::X86_64));
        assert!(
            resolved
                .natives
                .iter()
                .all(|n| n.artifact.path.contains(marker)),
            "{os:?} natives should all be {marker}"
        );
    }
}

#[test]
fn every_resolved_artifact_carries_a_verifiable_digest() {
    for version in [modern(), legacy()] {
        let resolved = version.resolve_libraries(&env(Os::Linux, Arch::X86_64));
        for artifact in resolved
            .classpath
            .iter()
            .chain(resolved.natives.iter().map(|n| &n.artifact))
        {
            assert!(
                artifact.sha1.is_some(),
                "{} has no sha1 to verify against",
                artifact.name
            );
            assert!(artifact.size.is_some(), "{} has no size", artifact.name);
            assert!(
                artifact.url.starts_with("https://"),
                "{} is not served over https",
                artifact.name
            );
        }
    }
}

#[test]
fn modern_jvm_arguments_are_selected_per_platform() {
    let version = modern();

    let macos = version.resolve_arguments(&env(Os::MacOs, Arch::Arm64));
    assert!(macos.jvm.iter().any(|a| a == "-XstartOnFirstThread"));

    let linux = version.resolve_arguments(&env(Os::Linux, Arch::X86_64));
    assert!(!linux.jvm.iter().any(|a| a == "-XstartOnFirstThread"));

    let thirty_two_bit = version.resolve_arguments(&env(Os::Windows, Arch::X86));
    assert!(thirty_two_bit.jvm.iter().any(|a| a == "-Xss1M"));

    let sixty_four_bit = version.resolve_arguments(&env(Os::Windows, Arch::X86_64));
    assert!(!sixty_four_bit.jvm.iter().any(|a| a == "-Xss1M"));
}

#[test]
fn modern_jvm_arguments_always_include_the_classpath() {
    let resolved = modern().resolve_arguments(&env(Os::Linux, Arch::X86_64));
    let index = resolved
        .jvm
        .iter()
        .position(|a| a == "-cp")
        .expect("-cp is present");
    assert_eq!(resolved.jvm[index + 1], "${classpath}");
}

#[test]
fn quick_play_arguments_appear_only_when_the_feature_is_enabled() {
    let version = modern();

    let plain = version.resolve_arguments(&env(Os::Linux, Arch::X86_64));
    assert!(!plain.game.iter().any(|a| a == "--quickPlaySingleplayer"));

    let quick = version.resolve_arguments(
        &env(Os::Linux, Arch::X86_64)
            .with_features(Features::new().with("is_quick_play_singleplayer", true)),
    );
    assert!(quick.game.iter().any(|a| a == "--quickPlaySingleplayer"));
}

#[test]
fn demo_argument_appears_only_for_demo_users() {
    let version = modern();
    let demo = version.resolve_arguments(
        &env(Os::Linux, Arch::X86_64).with_features(Features::new().with("is_demo_user", true)),
    );
    assert!(demo.game.iter().any(|a| a == "--demo"));
}

#[test]
fn legacy_arguments_come_from_the_flat_string_with_synthesised_jvm_arguments() {
    let resolved = legacy().resolve_arguments(&env(Os::Linux, Arch::X86_64));

    assert_eq!(resolved.game[0], "--username");
    assert_eq!(resolved.game[1], "${auth_player_name}");
    assert!(resolved.game.iter().any(|a| a == "${auth_access_token}"));
    assert_eq!(resolved.jvm, LEGACY_JVM_ARGUMENTS);
}

#[test]
fn parses_the_version_manifest_and_finds_versions() {
    let manifest = VersionManifest::parse(MANIFEST).expect("manifest parses");

    let latest = manifest
        .latest_release()
        .expect("the latest release is listed");
    assert_eq!(latest.release_type, ReleaseType::Release);
    assert!(latest.sha1.is_some());

    let legacy = manifest.find("1.12.2").expect("1.12.2 is listed");
    assert_eq!(legacy.release_type, ReleaseType::Release);

    assert!(manifest.find("this-version-does-not-exist").is_none());
    assert!(manifest.of_type(ReleaseType::Snapshot).count() > 0);
}

#[test]
fn maven_coordinates_map_to_repository_paths() {
    use acelus_meta::Coordinates;

    let plain = Coordinates::parse("com.mojang:jtracy:1.0.37").unwrap();
    assert_eq!(
        plain.to_path(),
        "com/mojang/jtracy/1.0.37/jtracy-1.0.37.jar"
    );

    let classified = Coordinates::parse("com.mojang:jtracy:1.0.37:natives-linux").unwrap();
    assert_eq!(
        classified.to_path(),
        "com/mojang/jtracy/1.0.37/jtracy-1.0.37-natives-linux.jar"
    );

    let extended = Coordinates::parse("net.fabricmc:intermediary:1.21@zip").unwrap();
    assert_eq!(
        extended.to_path(),
        "net/fabricmc/intermediary/1.21/intermediary-1.21.zip"
    );

    assert!(Coordinates::parse("not-a-coordinate").is_none());
}

#[test]
fn resolved_library_paths_match_their_declared_maven_layout() {
    let version = modern();
    let resolved = version.resolve_libraries(&env(Os::Linux, Arch::X86_64));

    for artifact in &resolved.classpath {
        let coordinates = acelus_meta::Coordinates::parse(&artifact.name)
            .unwrap_or_else(|| panic!("{} is not a maven coordinate", artifact.name));
        assert_eq!(
            artifact.path,
            coordinates.to_path(),
            "declared path for {} disagrees with its coordinates",
            artifact.name
        );
    }
}
