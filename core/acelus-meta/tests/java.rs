use acelus_meta::java::{Platform, RuntimeFiles, RuntimeManifest};
use acelus_meta::{Arch, Os};

const ALL: &[u8] = include_bytes!("fixtures/java_runtime_all.json");
const FILES: &[u8] = include_bytes!("fixtures/java_runtime_files.json");

fn manifest() -> RuntimeManifest {
    RuntimeManifest::parse(ALL).expect("the runtime manifest parses")
}

#[test]
fn a_modern_component_resolves_on_every_platform_mojang_ships() {
    let manifest = manifest();

    for platform in [
        Platform::Linux,
        Platform::MacOs,
        Platform::MacOsArm64,
        Platform::WindowsX64,
        Platform::WindowsArm64,
    ] {
        let entry = manifest
            .resolve(platform, "java-runtime-delta")
            .unwrap_or_else(|| panic!("java-runtime-delta should exist for {platform:?}"));

        assert!(entry.manifest.url.starts_with("https://"));
        assert!(entry.manifest.size > 0);
        assert!(!entry.version.name.is_empty());
    }
}

#[test]
fn the_legacy_component_is_missing_on_the_newer_architectures() {
    let manifest = manifest();

    assert!(manifest.resolve(Platform::Linux, "jre-legacy").is_some());
    assert!(manifest
        .resolve(Platform::WindowsX64, "jre-legacy")
        .is_some());

    assert!(
        manifest
            .resolve(Platform::MacOsArm64, "jre-legacy")
            .is_none(),
        "Mojang ships no legacy runtime for Apple Silicon, so old versions need a fallback"
    );
    assert!(
        manifest
            .resolve(Platform::WindowsArm64, "jre-legacy")
            .is_none(),
        "Mojang ships no legacy runtime for Windows on ARM"
    );
}

#[test]
fn linux_on_arm_has_no_mojang_runtime_at_all() {
    assert!(
        Platform::for_target(Os::Linux, Arch::Arm64).is_none(),
        "Mojang publishes no Linux ARM64 runtime; provisioning must fall back to Adoptium"
    );
}

#[test]
fn an_unknown_component_resolves_to_nothing_rather_than_panicking() {
    assert!(manifest()
        .resolve(Platform::Linux, "java-runtime-omega")
        .is_none());
}

#[test]
fn every_advertised_component_carries_a_verifiable_manifest() {
    let manifest = manifest();

    for platform in [
        Platform::Linux,
        Platform::LinuxI386,
        Platform::MacOs,
        Platform::MacOsArm64,
        Platform::WindowsX64,
        Platform::WindowsX86,
        Platform::WindowsArm64,
    ] {
        for component in manifest.available_components(platform) {
            let entry = manifest
                .resolve(platform, component)
                .expect("an advertised component resolves");
            assert_eq!(
                entry.manifest.sha1.len(),
                40,
                "{component} on {platform:?} has no usable sha1"
            );
        }
    }
}

#[test]
fn a_runtime_file_listing_separates_files_directories_and_links() {
    let files = RuntimeFiles::parse(FILES).expect("the file listing parses");

    assert!(files.directories().count() > 0);
    assert!(files.links().count() > 0);
    assert!(files.regular_files().count() > 0);

    for (path, target) in files.links() {
        assert!(!target.is_empty(), "{path} is a link with no target");
    }

    for (path, downloads, _) in files.regular_files() {
        assert_eq!(downloads.raw.sha1.len(), 40, "{path} has no usable sha1");
        assert!(downloads.raw.url.starts_with("https://"));
    }
}

#[test]
fn the_java_binary_keeps_its_executable_bit() {
    let files = RuntimeFiles::parse(FILES).unwrap();
    let java = files
        .java_executable(Os::Linux)
        .expect("the listing contains a java binary");

    let (_, _, executable) = files
        .regular_files()
        .find(|(path, _, _)| *path == java)
        .expect("the java binary is a regular file");

    assert!(
        executable,
        "a runtime whose java binary loses its executable bit cannot launch anything"
    );
}
