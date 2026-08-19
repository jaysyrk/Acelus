use std::path::{Path, PathBuf};

use acelus_auth::{Account, Secret};
use acelus_instance::lock::{
    JavaSource, LockArguments, LockArtifact, LockJava, LockMinecraft, Lockfile, Role, LOCK_VERSION,
};
use acelus_instance::paths::{InstanceLayout, Paths};
use acelus_launch::invocation::{build, Error, LaunchRequest, MemorySettings, QuickPlay};
use acelus_meta::Os;

fn artifact(path: &str, role: Role) -> LockArtifact {
    LockArtifact {
        path: path.into(),
        role,
        size: 1,
        blake3: "0".repeat(64),
        sha1: Some("1".repeat(40)),
        sha512: None,
        origin: vec!["https://example/x".into()],
        extract: None,
    }
}

fn lockfile() -> Lockfile {
    Lockfile {
        lock_version: LOCK_VERSION,
        generated_by: Some("acelus 0.1.0".into()),
        minecraft: LockMinecraft {
            id: "26.2".into(),
            release_type: "release".into(),
            manifest_sha1: "c".repeat(40),
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
        arguments: LockArguments {
            jvm: vec![
                "-Djava.library.path=${natives_directory}".into(),
                "-cp".into(),
                "${classpath}".into(),
            ],
            game: vec![
                "--username".into(),
                "${auth_player_name}".into(),
                "--uuid".into(),
                "${auth_uuid}".into(),
                "--accessToken".into(),
                "${auth_access_token}".into(),
                "--xuid".into(),
                "${auth_xuid}".into(),
                "--assetsDir".into(),
                "${assets_root}".into(),
                "--assetIndex".into(),
                "${assets_index_name}".into(),
                "--versionType".into(),
                "${version_type}".into(),
            ],
        },
        artifacts: vec![
            artifact("client.jar", Role::Client),
            artifact("libraries/a.jar", Role::Library),
            artifact("libraries/b.jar", Role::Library),
            artifact("natives/jars/n.jar", Role::Native),
        ],
    }
}

fn account() -> Account {
    Account {
        uuid: "986dec87-b7ec-47ff-89ff-033fdb95c4b5".into(),
        name: "HowDoesAuthWork".into(),
        minecraft_token: Secret::new("minecraft-access-token"),
        refresh_token: None,
        expires_in: 86400,
        xuid: Some("2535416239104446".into()),
        entitlement_verified: true,
        skin_url: None,
        cape_url: None,
    }
}

struct Fixture {
    lockfile: Lockfile,
    layout: InstanceLayout,
    paths: Paths,
    account: Account,
    java: PathBuf,
}

fn fixture() -> Fixture {
    Fixture {
        lockfile: lockfile(),
        layout: InstanceLayout::new("/data/acelus/instances/survival"),
        paths: Paths::with_root("/data/acelus"),
        account: account(),
        java: PathBuf::from("/data/acelus/runtimes/java-runtime-epsilon/bin/java"),
    }
}

fn request<'a>(fixture: &'a Fixture, os: Os) -> LaunchRequest<'a> {
    LaunchRequest {
        lockfile: &fixture.lockfile,
        layout: &fixture.layout,
        paths: &fixture.paths,
        account: &fixture.account,
        java_executable: &fixture.java,
        os,
        memory: MemorySettings::default(),
        extra_jvm_arguments: Vec::new(),
        quick_play: QuickPlay::default(),
        resolution: None,
        client_id: None,
    }
}

#[test]
fn the_command_line_is_assembled_in_the_order_the_jvm_expects() {
    let fixture = fixture();
    let invocation = build(&request(&fixture, Os::Linux)).unwrap();

    assert_eq!(invocation.program, fixture.java);
    assert_eq!(
        invocation.working_directory,
        Path::new("/data/acelus/instances/survival/minecraft")
    );

    let main_class = invocation
        .arguments
        .iter()
        .position(|a| a == "net.minecraft.client.main.Main")
        .expect("the main class is present");
    let classpath = invocation
        .arguments
        .iter()
        .position(|a| a == "-cp")
        .expect("-cp is present");
    let username = invocation
        .arguments
        .iter()
        .position(|a| a == "--username")
        .expect("--username is present");

    assert!(
        classpath < main_class,
        "jvm arguments must precede the main class"
    );
    assert!(
        main_class < username,
        "game arguments must follow the main class"
    );
}

#[test]
fn the_classpath_is_joined_with_the_separator_for_the_target_platform() {
    let fixture = fixture();

    let linux = build(&request(&fixture, Os::Linux)).unwrap();
    let classpath =
        linux.arguments[linux.arguments.iter().position(|a| a == "-cp").unwrap() + 1].clone();
    assert!(classpath.contains(':'));
    assert!(!classpath.contains(';'));

    let windows = build(&request(&fixture, Os::Windows)).unwrap();
    let classpath =
        windows.arguments[windows.arguments.iter().position(|a| a == "-cp").unwrap() + 1].clone();
    assert!(classpath.contains(';'));
}

#[test]
fn the_classpath_carries_libraries_and_the_client_but_not_natives() {
    let fixture = fixture();
    let invocation = build(&request(&fixture, Os::Linux)).unwrap();

    let index = invocation
        .arguments
        .iter()
        .position(|a| a == "-cp")
        .unwrap();
    let classpath = &invocation.arguments[index + 1];

    assert!(classpath.contains("libraries/a.jar"));
    assert!(classpath.contains("libraries/b.jar"));
    assert!(classpath.contains("client.jar"));
    assert!(
        !classpath.contains("natives/jars/n.jar"),
        "native jars are unpacked, not put on the classpath for this version"
    );

    let client = classpath.find("client.jar").unwrap();
    let library = classpath.find("libraries/a.jar").unwrap();
    assert!(
        library < client,
        "libraries should precede the client jar on the classpath"
    );
}

#[test]
fn account_details_are_substituted_into_the_game_arguments() {
    let fixture = fixture();
    let invocation = build(&request(&fixture, Os::Linux)).unwrap();

    let value_after = |flag: &str| {
        let index = invocation.arguments.iter().position(|a| a == flag).unwrap();
        invocation.arguments[index + 1].clone()
    };

    assert_eq!(value_after("--username"), "HowDoesAuthWork");
    assert_eq!(
        value_after("--uuid"),
        "986dec87b7ec47ff89ff033fdb95c4b5",
        "the game expects the uuid without dashes"
    );
    assert_eq!(value_after("--accessToken"), "minecraft-access-token");
    assert_eq!(value_after("--xuid"), "2535416239104446");
    assert_eq!(value_after("--assetsDir"), "/data/acelus/assets");
    assert_eq!(value_after("--assetIndex"), "32");
    assert_eq!(value_after("--versionType"), "release");
}

#[test]
fn assets_are_taken_from_shared_storage_rather_than_the_instance() {
    let fixture = fixture();
    let invocation = build(&request(&fixture, Os::Linux)).unwrap();

    let index = invocation
        .arguments
        .iter()
        .position(|a| a == "--assetsDir")
        .unwrap();
    let assets = &invocation.arguments[index + 1];

    assert!(
        !assets.contains("instances"),
        "assets must be shared between instances, got {assets}"
    );
}

#[test]
fn memory_settings_reach_the_jvm() {
    let fixture = fixture();
    let mut request = request(&fixture, Os::Linux);
    request.memory = MemorySettings {
        minimum_megabytes: 1024,
        maximum_megabytes: 8192,
    };

    let invocation = build(&request).unwrap();
    assert!(invocation.arguments.contains(&"-Xms1024M".to_string()));
    assert!(invocation.arguments.contains(&"-Xmx8192M".to_string()));
}

#[test]
fn extra_jvm_arguments_are_appended_before_the_main_class() {
    let fixture = fixture();
    let mut request = request(&fixture, Os::Linux);
    request.extra_jvm_arguments = vec!["-XX:+UseZGC".into()];

    let invocation = build(&request).unwrap();
    let flag = invocation
        .arguments
        .iter()
        .position(|a| a == "-XX:+UseZGC")
        .expect("the extra flag is present");
    let main_class = invocation
        .arguments
        .iter()
        .position(|a| a == "net.minecraft.client.main.Main")
        .unwrap();

    assert!(flag < main_class);
}

#[test]
fn an_account_without_a_verified_entitlement_cannot_launch() {
    let fixture = fixture();
    let unverified = Account {
        entitlement_verified: false,
        ..account()
    };

    let mut request = request(&fixture, Os::Linux);
    request.account = &unverified;

    let error = build(&request).unwrap_err();
    assert!(
        matches!(error, Error::NotEntitled { .. }),
        "launching without a verified entitlement is exactly what Acelus refuses to do, got {error:?}"
    );
}

#[test]
fn an_account_without_a_session_token_cannot_launch() {
    let fixture = fixture();
    let expired = Account {
        minecraft_token: Secret::new(""),
        ..account()
    };

    let mut request = request(&fixture, Os::Linux);
    request.account = &expired;

    assert!(matches!(
        build(&request).unwrap_err(),
        Error::SessionExpired { .. }
    ));
}

#[test]
fn a_lockfile_without_a_client_jar_cannot_launch() {
    let mut fixture = fixture();
    fixture
        .lockfile
        .artifacts
        .retain(|a| a.role != Role::Client);

    assert!(matches!(
        build(&request(&fixture, Os::Linux)).unwrap_err(),
        Error::NoClient
    ));
}

#[test]
fn an_unknown_placeholder_stops_the_launch_rather_than_reaching_the_game() {
    let mut fixture = fixture();
    fixture
        .lockfile
        .arguments
        .game
        .push("${something_acelus_does_not_know}".into());

    let error = build(&request(&fixture, Os::Linux)).unwrap_err();
    match error {
        Error::Unresolved(unresolved) => {
            assert_eq!(unresolved.names, vec!["something_acelus_does_not_know"]);
        }
        other => panic!("expected the unresolved placeholder to be named, got {other:?}"),
    }
}

#[test]
fn quick_play_values_are_substituted_when_asked_for() {
    let mut fixture = fixture();
    fixture.lockfile.arguments.game.extend([
        "--quickPlayMultiplayer".into(),
        "${quickPlayMultiplayer}".into(),
    ]);

    let mut request = request(&fixture, Os::Linux);
    request.quick_play = QuickPlay {
        multiplayer: Some("mc.example.net".into()),
        ..QuickPlay::default()
    };

    let invocation = build(&request).unwrap();
    let index = invocation
        .arguments
        .iter()
        .position(|a| a == "--quickPlayMultiplayer")
        .unwrap();
    assert_eq!(invocation.arguments[index + 1], "mc.example.net");
}

#[test]
fn the_logging_configuration_is_passed_when_the_version_ships_one() {
    let mut fixture = fixture();
    fixture
        .lockfile
        .artifacts
        .push(artifact("logging/client.xml", Role::LoggingConfig));

    let invocation = build(&request(&fixture, Os::Linux)).unwrap();

    let logging = invocation
        .arguments
        .iter()
        .find(|a| a.starts_with("-Dlog4j.configurationFile="))
        .expect("the logging configuration should be passed to the jvm");

    assert!(logging.ends_with("instances/survival/logging/client.xml"));
    assert!(!logging.contains("${path}"));
}

#[test]
fn no_logging_argument_is_passed_when_the_version_ships_none() {
    let fixture = fixture();
    let invocation = build(&request(&fixture, Os::Linux)).unwrap();

    assert!(!invocation
        .arguments
        .iter()
        .any(|a| a.starts_with("-Dlog4j.configurationFile=")));
}

#[test]
fn the_access_token_is_not_revealed_by_the_request_debug_output() {
    let fixture = fixture();
    assert!(!format!("{:?}", fixture.account).contains("minecraft-access-token"));
}
