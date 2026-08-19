use std::path::{Path, PathBuf};

use acelus_auth::Account;
use acelus_instance::lock::{Lockfile, Role};
use acelus_instance::paths::{InstanceLayout, Paths};
use acelus_meta::arguments::{substitute, Substitutions, UnresolvedPlaceholders};
use acelus_meta::Os;

pub const LAUNCHER_NAME: &str = "acelus";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(
        "refusing to launch as {name}: this account has no verified Minecraft entitlement. \
         Acelus only launches accounts that genuinely own the game"
    )]
    NotEntitled { name: String },

    #[error("refusing to launch as {name}: the Minecraft session has expired and must be renewed")]
    SessionExpired { name: String },

    #[error(transparent)]
    Unresolved(#[from] UnresolvedPlaceholders),

    #[error("the instance lockfile names no client jar, so there is nothing to run")]
    NoClient,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemorySettings {
    pub minimum_megabytes: u32,
    pub maximum_megabytes: u32,
}

impl Default for MemorySettings {
    fn default() -> Self {
        Self {
            minimum_megabytes: 512,
            maximum_megabytes: 4096,
        }
    }
}

impl MemorySettings {
    pub fn to_arguments(self) -> Vec<String> {
        vec![
            format!("-Xms{}M", self.minimum_megabytes),
            format!("-Xmx{}M", self.maximum_megabytes),
        ]
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuickPlay {
    pub singleplayer: Option<String>,
    pub multiplayer: Option<String>,
    pub realms: Option<String>,
}

impl QuickPlay {
    pub fn is_empty(&self) -> bool {
        self.singleplayer.is_none() && self.multiplayer.is_none() && self.realms.is_none()
    }
}

#[derive(Debug, Clone)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

pub struct LaunchRequest<'a> {
    pub lockfile: &'a Lockfile,
    pub layout: &'a InstanceLayout,
    pub paths: &'a Paths,
    pub account: &'a Account,
    pub java_executable: &'a Path,
    pub os: Os,
    pub memory: MemorySettings,
    pub extra_jvm_arguments: Vec<String>,
    pub quick_play: QuickPlay,
    pub resolution: Option<Resolution>,
    pub client_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub program: PathBuf,
    pub arguments: Vec<String>,
    pub working_directory: PathBuf,
}

impl Invocation {
    pub fn command_line(&self) -> String {
        std::iter::once(self.program.display().to_string())
            .chain(self.arguments.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub fn classpath_separator(os: Os) -> &'static str {
    match os {
        Os::Windows => ";",
        _ => ":",
    }
}

pub fn build(request: &LaunchRequest<'_>) -> Result<Invocation> {
    if !request.account.entitlement_verified {
        return Err(Error::NotEntitled {
            name: request.account.name.clone(),
        });
    }

    if request.account.minecraft_token.is_empty() {
        return Err(Error::SessionExpired {
            name: request.account.name.clone(),
        });
    }

    let classpath = build_classpath(request)?;
    let substitutions = build_substitutions(request, &classpath);

    let mut arguments = substitute(&request.lockfile.arguments.jvm, &substitutions)?;
    arguments.extend(request.memory.to_arguments());
    arguments.extend(request.extra_jvm_arguments.iter().cloned());

    if let Some(logging) = logging_argument(request, &substitutions)? {
        arguments.push(logging);
    }

    arguments.push(request.lockfile.minecraft.main_class.clone());
    arguments.extend(substitute(
        &request.lockfile.arguments.game,
        &substitutions,
    )?);

    Ok(Invocation {
        program: request.java_executable.to_path_buf(),
        arguments,
        working_directory: request.layout.game_dir(),
    })
}

fn build_classpath(request: &LaunchRequest<'_>) -> Result<String> {
    let entries = request.lockfile.classpath();
    if !entries.iter().any(|entry| entry.role == Role::Client) {
        return Err(Error::NoClient);
    }

    let joined = entries
        .iter()
        .map(|entry| request.layout.resolve(&entry.path).display().to_string())
        .collect::<Vec<_>>()
        .join(classpath_separator(request.os));

    Ok(joined)
}

fn logging_argument(
    request: &LaunchRequest<'_>,
    substitutions: &Substitutions,
) -> Result<Option<String>> {
    let Some(config) = request
        .lockfile
        .artifacts_with_role(Role::LoggingConfig)
        .next()
    else {
        return Ok(None);
    };

    let path = request.layout.resolve(&config.path);
    let mut with_path = substitutions.clone();
    with_path.insert("path", path.display().to_string());

    let rendered = substitute(
        &["-Dlog4j.configurationFile=${path}".to_string()],
        &with_path,
    )?;

    Ok(rendered.into_iter().next())
}

fn build_substitutions(request: &LaunchRequest<'_>, classpath: &str) -> Substitutions {
    let account = request.account;
    let lockfile = request.lockfile;

    let mut substitutions = Substitutions::new()
        .set("auth_player_name", &account.name)
        .set("auth_uuid", account.uuid.replace('-', ""))
        .set("auth_access_token", account.minecraft_token.expose())
        .set("auth_xuid", account.xuid.clone().unwrap_or_default())
        .set("auth_session", format!("token:{}", account.uuid))
        .set("clientid", request.client_id.clone().unwrap_or_default())
        .set("user_type", "msa")
        .set("user_properties", "{}")
        .set("version_name", &lockfile.minecraft.id)
        .set("version_type", &lockfile.minecraft.release_type)
        .set(
            "game_directory",
            request.layout.game_dir().display().to_string(),
        )
        .set("assets_root", request.paths.assets().display().to_string())
        .set("game_assets", request.paths.assets().display().to_string())
        .set(
            "assets_index_name",
            lockfile
                .minecraft
                .asset_index_id
                .clone()
                .unwrap_or_default(),
        )
        .set(
            "natives_directory",
            request.layout.natives().display().to_string(),
        )
        .set("launcher_name", LAUNCHER_NAME)
        .set("launcher_version", env!("CARGO_PKG_VERSION"))
        .set("classpath", classpath)
        .set("classpath_separator", classpath_separator(request.os))
        .set(
            "library_directory",
            request.layout.libraries().display().to_string(),
        );

    if let Some(resolution) = &request.resolution {
        substitutions.insert("resolution_width", resolution.width.to_string());
        substitutions.insert("resolution_height", resolution.height.to_string());
    }

    let quick_play = &request.quick_play;
    substitutions.insert(
        "quickPlayPath",
        request
            .layout
            .resolve("quickplay.json")
            .display()
            .to_string(),
    );
    substitutions.insert(
        "quickPlaySingleplayer",
        quick_play.singleplayer.clone().unwrap_or_default(),
    );
    substitutions.insert(
        "quickPlayMultiplayer",
        quick_play.multiplayer.clone().unwrap_or_default(),
    );
    substitutions.insert(
        "quickPlayRealms",
        quick_play.realms.clone().unwrap_or_default(),
    );

    substitutions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_classpath_separator_follows_the_target_platform() {
        assert_eq!(classpath_separator(Os::Windows), ";");
        assert_eq!(classpath_separator(Os::Linux), ":");
        assert_eq!(classpath_separator(Os::MacOs), ":");
    }

    #[test]
    fn memory_settings_render_as_jvm_flags() {
        let memory = MemorySettings {
            minimum_megabytes: 1024,
            maximum_megabytes: 8192,
        };
        assert_eq!(memory.to_arguments(), vec!["-Xms1024M", "-Xmx8192M"]);
    }

    #[test]
    fn quick_play_reports_whether_anything_was_asked_for() {
        assert!(QuickPlay::default().is_empty());
        assert!(!QuickPlay {
            multiplayer: Some("hypixel.net".into()),
            ..QuickPlay::default()
        }
        .is_empty());
    }
}
