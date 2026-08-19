use acelus_instance::lock::{
    JavaSource, LoaderKind, LockArtifact, LockExtract, LockJava, LockLoader, LockMinecraft,
    Lockfile, Role, LOCK_VERSION,
};

const SCHEMA: &str = include_str!("../../../schema/acelus.lock.schema.json");

fn schema() -> jsonschema::Validator {
    let document: serde_json::Value =
        serde_json::from_str(SCHEMA).expect("the schema is valid json");
    jsonschema::validator_for(&document).expect("the schema itself is a valid json schema")
}

fn assert_valid(lockfile: &Lockfile) {
    let value = serde_json::to_value(lockfile).expect("the lockfile serialises");
    let validator = schema();

    let errors: Vec<String> = validator
        .iter_errors(&value)
        .map(|error| format!("{} at {}", error, error.instance_path))
        .collect();

    assert!(
        errors.is_empty(),
        "the generated lockfile does not satisfy schema/acelus.lock.schema.json:\n{}",
        errors.join("\n")
    );
}

fn minimal() -> Lockfile {
    Lockfile {
        lock_version: LOCK_VERSION,
        generated_by: Some("acelus 0.1.0".into()),
        minecraft: LockMinecraft {
            id: "26.2".into(),
            release_type: "release".into(),
            manifest_sha1: "c75d82e7fa6eca5a043dab0c6cf77cb8317644f4".into(),
            asset_index_id: Some("32".into()),
            main_class: "net.minecraft.client.main.Main".into(),
            compliance_level: Some(1),
        },
        loader: None,
        java: LockJava {
            component: Some("java-runtime-epsilon".into()),
            major_version: 25,
            source: JavaSource::Mojang,
        },
        artifacts: Vec::new(),
    }
}

fn artifact(path: &str, role: Role) -> LockArtifact {
    LockArtifact {
        path: path.into(),
        role,
        size: 39193383,
        blake3: "0".repeat(64),
        sha1: Some("2dc72797acbc1b63fc16a11c4ac393605f453754".into()),
        sha512: None,
        origin: vec!["https://piston-data.mojang.com/v1/objects/x/client.jar".into()],
        extract: None,
    }
}

#[test]
fn a_minimal_lockfile_satisfies_the_published_schema() {
    assert_valid(&minimal());
}

#[test]
fn every_role_the_code_can_emit_is_accepted_by_the_schema() {
    let mut lockfile = minimal();
    lockfile.artifacts = [
        Role::Client,
        Role::Library,
        Role::Native,
        Role::Asset,
        Role::LoggingConfig,
        Role::Mod,
        Role::Runtime,
        Role::Override,
    ]
    .into_iter()
    .enumerate()
    .map(|(n, role)| artifact(&format!("file-{n}"), role))
    .collect();

    assert_valid(&lockfile);
}

#[test]
fn every_loader_the_code_can_emit_is_accepted_by_the_schema() {
    for kind in [
        LoaderKind::Fabric,
        LoaderKind::Quilt,
        LoaderKind::NeoForge,
        LoaderKind::Forge,
    ] {
        let mut lockfile = minimal();
        lockfile.loader = Some(LockLoader {
            kind,
            version: "0.16.9".into(),
            intermediary_version: Some("1.21".into()),
        });
        assert_valid(&lockfile);
    }
}

#[test]
fn every_java_source_the_code_can_emit_is_accepted_by_the_schema() {
    for source in [JavaSource::Mojang, JavaSource::Adoptium, JavaSource::System] {
        let mut lockfile = minimal();
        lockfile.java.source = source;
        assert_valid(&lockfile);
    }
}

#[test]
fn extraction_directives_are_accepted_by_the_schema() {
    let mut lockfile = minimal();
    let mut native = artifact("natives/lwjgl.jar", Role::Native);
    native.extract = Some(LockExtract {
        exclude: vec!["META-INF/".into()],
    });
    lockfile.artifacts = vec![native];

    assert_valid(&lockfile);
}

#[test]
fn a_modrinth_style_sha512_is_accepted_by_the_schema() {
    let mut lockfile = minimal();
    let mut modfile = artifact("minecraft/mods/sodium.jar", Role::Mod);
    modfile.sha1 = None;
    modfile.sha512 = Some("f".repeat(128));
    modfile.origin = vec!["https://cdn.modrinth.com/data/x/sodium.jar".into()];
    lockfile.artifacts = vec![modfile];

    assert_valid(&lockfile);
}

#[test]
fn the_schema_rejects_a_lockfile_with_a_malformed_digest() {
    let mut lockfile = minimal();
    let mut broken = artifact("client.jar", Role::Client);
    broken.blake3 = "too-short".into();
    lockfile.artifacts = vec![broken];

    let value = serde_json::to_value(&lockfile).unwrap();
    assert!(
        schema().iter_errors(&value).next().is_some(),
        "the schema should reject a digest that is not 64 hex characters"
    );
}

#[test]
fn the_schema_rejects_an_artifact_with_no_origin() {
    let mut lockfile = minimal();
    let mut orphan = artifact("client.jar", Role::Client);
    orphan.origin = Vec::new();
    lockfile.artifacts = vec![orphan];

    let value = serde_json::to_value(&lockfile).unwrap();
    assert!(
        schema().iter_errors(&value).next().is_some(),
        "an artifact with nowhere to be fetched from should be rejected"
    );
}
