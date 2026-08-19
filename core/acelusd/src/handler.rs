use std::sync::Arc;

use acelus_auth::store::now_seconds;
use acelus_instance::lock::Lockfile;
use acelus_instance::{InstanceLayout, Progress, Role};
use acelus_launch::invocation::{self, LaunchRequest, MemorySettings, QuickPlay};
use acelus_launch::session::DEFAULT_LOG_CAPACITY;
use acelus_launch::Session;
use acelus_meta::ReleaseType;
use acelus_rpc::protocol::{codes, RpcError};
use acelus_rpc::{Handler, Notifier};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::Daemon;

pub struct Rpc {
    daemon: Arc<Daemon>,
}

impl Rpc {
    pub fn new(daemon: Arc<Daemon>) -> Self {
        Self { daemon }
    }
}

fn params<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, RpcError> {
    serde_json::from_value(value).map_err(|error| RpcError::invalid_params(error.to_string()))
}

fn internal(error: impl std::fmt::Display) -> RpcError {
    RpcError::internal(error.to_string())
}

fn not_found(error: impl std::fmt::Display) -> RpcError {
    RpcError::new(codes::NOT_FOUND, error.to_string())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionListParams {
    #[serde(default)]
    channels: Option<Vec<String>>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateParams {
    name: String,
    version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdParams {
    id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteParams {
    id: String,
    #[serde(default)]
    keep_user_data: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UuidParams {
    uuid: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LaunchParams {
    id: String,
    #[serde(default)]
    account_uuid: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionParams {
    session_id: String,
}

#[async_trait::async_trait]
impl Handler for Rpc {
    async fn call(
        &self,
        method: &str,
        params_value: Value,
        notifier: &Notifier,
    ) -> Result<Value, RpcError> {
        match method {
            "daemon.info" => self.daemon_info(),
            "version.list" => self.version_list(params(params_value)?).await,
            "instance.create" => self.instance_create(params(params_value)?),
            "instance.list" => self.instance_list(),
            "instance.delete" => self.instance_delete(params(params_value)?),
            "install.run" => self.install_run(params(params_value)?, notifier).await,
            "verify.run" => self.verify_run(params(params_value)?),
            "account.beginLogin" => self.account_begin_login(notifier.clone()).await,
            "account.list" => self.account_list(),
            "account.select" => self.account_select(params(params_value)?),
            "account.remove" => self.account_remove(params(params_value)?),
            "launch.start" => self.launch_start(params(params_value)?).await,
            "launch.status" => self.launch_status().await,
            "launch.stop" => self.launch_stop(params(params_value)?).await,
            "log.tail" => self.log_tail(params(params_value)?).await,
            other => Err(RpcError::method_not_found(other)),
        }
    }
}

impl Rpc {
    fn daemon_info(&self) -> Result<Value, RpcError> {
        Ok(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "rpcVersion": 1,
            "dataDir": self.daemon.paths.root().display().to_string(),
        }))
    }

    async fn version_list(&self, request: VersionListParams) -> Result<Value, RpcError> {
        let manifest = self
            .daemon
            .resolver
            .version_manifest()
            .await
            .map_err(|error| RpcError::new(codes::NETWORK, error.to_string()))?;

        let wanted: Option<Vec<ReleaseType>> = request.channels.map(|channels| {
            channels
                .iter()
                .filter_map(|channel| match channel.as_str() {
                    "release" => Some(ReleaseType::Release),
                    "snapshot" => Some(ReleaseType::Snapshot),
                    "old_beta" => Some(ReleaseType::OldBeta),
                    "old_alpha" => Some(ReleaseType::OldAlpha),
                    _ => None,
                })
                .collect()
        });

        let versions: Vec<Value> = manifest
            .versions
            .iter()
            .filter(|summary| {
                wanted
                    .as_ref()
                    .map(|kinds| kinds.contains(&summary.release_type))
                    .unwrap_or(true)
            })
            .take(request.limit.unwrap_or(usize::MAX))
            .map(|summary| {
                json!({
                    "id": summary.id,
                    "type": summary.release_type,
                    "releaseTime": summary.release_time,
                    "complianceLevel": summary.compliance_level,
                })
            })
            .collect();

        Ok(json!({ "versions": versions, "latest": {
            "release": manifest.latest.release,
            "snapshot": manifest.latest.snapshot,
        }}))
    }

    fn instance_create(&self, request: CreateParams) -> Result<Value, RpcError> {
        let descriptor = self
            .daemon
            .instances
            .create(&request.name, &request.version)
            .map_err(|error| match error {
                acelus_instance::descriptor::Error::AlreadyExists { .. } => {
                    RpcError::new(codes::ALREADY_EXISTS, error.to_string())
                }
                acelus_instance::descriptor::Error::InvalidName { .. } => {
                    RpcError::invalid_params(error.to_string())
                }
                other => internal(other),
            })?;

        Ok(json!({ "instance": self.describe(&descriptor) }))
    }

    fn instance_list(&self) -> Result<Value, RpcError> {
        let instances: Vec<Value> = self
            .daemon
            .instances
            .list()
            .map_err(internal)?
            .iter()
            .map(|descriptor| self.describe(descriptor))
            .collect();

        Ok(json!({ "instances": instances }))
    }

    fn instance_delete(&self, request: DeleteParams) -> Result<Value, RpcError> {
        self.daemon
            .instances
            .delete(&request.id, request.keep_user_data)
            .map_err(not_found)?;
        Ok(json!({}))
    }

    fn describe(&self, descriptor: &acelus_instance::Descriptor) -> Value {
        let layout = self.daemon.instances.layout(&descriptor.id);
        json!({
            "id": descriptor.id,
            "name": descriptor.name,
            "version": descriptor.version,
            "path": layout.root().display().to_string(),
            "installed": layout.lockfile().is_file(),
            "lastPlayed": descriptor.last_played,
        })
    }

    async fn install_run(&self, request: IdParams, notifier: &Notifier) -> Result<Value, RpcError> {
        let descriptor = self.daemon.instances.get(&request.id).map_err(not_found)?;
        let layout = self.daemon.instances.layout(&descriptor.id);

        let plan = self
            .daemon
            .resolver
            .resolve(&descriptor.version, &self.daemon.environment)
            .await
            .map_err(|error| RpcError::new(codes::NETWORK, error.to_string()))?;

        let report = |progress: &Progress| {
            notifier.notify(
                "install.progress",
                json!({
                    "jobId": descriptor.id,
                    "phase": format!("{:?}", progress.phase).to_lowercase(),
                    "completedBytes": progress.completed_bytes,
                    "totalBytes": progress.total_bytes,
                    "completedFiles": progress.completed_files,
                    "totalFiles": progress.total_files,
                    "reusedBytes": progress.reused_bytes,
                    "current": progress.current,
                }),
            );
        };

        let lockfile = self
            .daemon
            .installer
            .install(&plan, &layout, &report)
            .await
            .map_err(|error| RpcError::new(codes::INTEGRITY_FAILED, error.to_string()))?;

        self.daemon
            .java
            .provision(&plan.java, self.daemon.environment.os)
            .await
            .map_err(|error| RpcError::new(codes::JAVA_UNAVAILABLE, error.to_string()))?;

        write_lockfile(&layout, &lockfile)?;

        notifier.notify(
            "install.complete",
            json!({"jobId": descriptor.id, "ok": true}),
        );

        Ok(json!({
            "jobId": descriptor.id,
            "artifacts": lockfile.artifacts.len(),
            "bytes": lockfile.total_size(),
        }))
    }

    fn verify_run(&self, request: IdParams) -> Result<Value, RpcError> {
        let layout = self.daemon.instances.layout(&request.id);
        let lockfile = read_lockfile(&layout)?;

        let mut problems = Vec::new();
        for artifact in &lockfile.artifacts {
            let path = match artifact.role {
                Role::Asset => self.daemon.paths.root().join(&artifact.path),
                _ => layout.resolve(&artifact.path),
            };

            match std::fs::metadata(&path) {
                Err(_) => problems.push(json!({"path": artifact.path, "kind": "missing"})),
                Ok(metadata) if metadata.len() != artifact.size => problems.push(json!({
                    "path": artifact.path,
                    "kind": "size-mismatch",
                    "expected": artifact.size.to_string(),
                    "actual": metadata.len().to_string(),
                })),
                Ok(_) => {}
            }
        }

        Ok(json!({"ok": problems.is_empty(), "problems": problems}))
    }

    async fn account_begin_login(&self, notifier: Notifier) -> Result<Value, RpcError> {
        let authenticator = self.daemon.authenticator.as_ref().ok_or_else(|| {
            RpcError::new(
                codes::AZURE_APP_UNAPPROVED,
                crate::state::AccountProblem::NoClientId.message(),
            )
        })?;

        let code = authenticator
            .begin_device_code()
            .await
            .map_err(|error| RpcError::new(codes::AUTH_REQUIRED, error.to_string()))?;

        let job_id = uuid::Uuid::new_v4().to_string();
        self.daemon
            .pending_logins
            .lock()
            .await
            .insert(job_id.clone(), code.clone());

        let reply = json!({
            "jobId": job_id,
            "userCode": code.user_code,
            "verificationUri": code.verification_uri,
            "expiresIn": code.expires_in,
            "interval": code.interval,
        });

        let daemon = self.daemon.clone();
        let waiting_job = job_id.clone();
        tokio::spawn(async move {
            let authenticator = daemon
                .authenticator
                .as_ref()
                .expect("a login only starts when an authenticator exists");

            let outcome = authenticator.login_with_device_code(&code).await;
            daemon.pending_logins.lock().await.remove(&waiting_job);

            match outcome {
                Ok(account) => {
                    let stored = daemon.accounts.remember(&account, now_seconds());
                    notifier.notify(
                        "account.loginComplete",
                        json!({
                            "jobId": waiting_job,
                            "account": stored.ok().map(|stored| json!({
                                "uuid": stored.uuid,
                                "name": stored.name,
                                "entitlementVerified": stored.entitlement_verified,
                            })),
                        }),
                    );
                }
                Err(error) => notifier.notify(
                    "account.loginComplete",
                    json!({
                        "jobId": waiting_job,
                        "error": {"code": codes::AUTH_REQUIRED, "message": error.to_string()},
                    }),
                ),
            }
        });

        Ok(reply)
    }

    fn account_list(&self) -> Result<Value, RpcError> {
        let registry = self.daemon.accounts.load().map_err(internal)?;
        let now = now_seconds();

        let accounts: Vec<Value> = registry
            .accounts
            .iter()
            .map(|account| {
                json!({
                    "uuid": account.uuid,
                    "name": account.name,
                    "skinUrl": account.skin_url,
                    "capeUrl": account.cape_url,
                    "entitlementVerified": account.entitlement_verified,
                    "expired": account.is_expired(now),
                })
            })
            .collect();

        Ok(json!({"active": registry.active, "accounts": accounts}))
    }

    fn account_select(&self, request: UuidParams) -> Result<Value, RpcError> {
        self.daemon
            .accounts
            .set_active(&request.uuid)
            .map_err(not_found)?;
        Ok(json!({}))
    }

    fn account_remove(&self, request: UuidParams) -> Result<Value, RpcError> {
        self.daemon
            .accounts
            .forget(&request.uuid)
            .map_err(not_found)?;
        Ok(json!({}))
    }

    async fn launch_start(&self, request: LaunchParams) -> Result<Value, RpcError> {
        let descriptor = self.daemon.instances.get(&request.id).map_err(not_found)?;
        let layout = self.daemon.instances.layout(&descriptor.id);
        let lockfile = read_lockfile(&layout)?;

        let account = self
            .daemon
            .usable_account(request.account_uuid.as_deref())
            .await
            .map_err(|problem| RpcError::new(codes::AUTH_REQUIRED, problem.message()))?;

        let runtime = self
            .daemon
            .java
            .provision(
                &acelus_instance::PlannedJava {
                    component: lockfile.java.component.clone(),
                    major_version: lockfile.java.major_version,
                    source: lockfile.java.source,
                    manifest_url: None,
                    manifest_sha1: None,
                },
                self.daemon.environment.os,
            )
            .await
            .or_else(|_| acelus_instance::java::discover_system_java(lockfile.java.major_version))
            .map_err(|error| RpcError::new(codes::JAVA_UNAVAILABLE, error.to_string()))?;

        let memory = descriptor
            .memory_megabytes
            .map(|maximum| MemorySettings {
                minimum_megabytes: 512,
                maximum_megabytes: maximum,
            })
            .unwrap_or_default();

        let invocation = invocation::build(&LaunchRequest {
            lockfile: &lockfile,
            layout: &layout,
            paths: &self.daemon.paths,
            account: &account,
            java_executable: &runtime.java_executable,
            os: self.daemon.environment.os,
            memory,
            extra_jvm_arguments: descriptor.extra_jvm_arguments.clone(),
            quick_play: QuickPlay::default(),
            resolution: None,
            client_id: None,
        })
        .map_err(|error| match error {
            invocation::Error::NotEntitled { .. } => {
                RpcError::new(codes::ENTITLEMENT_MISSING, error.to_string())
            }
            invocation::Error::SessionExpired { .. } => {
                RpcError::new(codes::AUTH_EXPIRED, error.to_string())
            }
            other => internal(other),
        })?;

        let session = Session::spawn(&invocation, DEFAULT_LOG_CAPACITY)
            .await
            .map_err(internal)?;

        let reply = json!({"sessionId": session.id(), "pid": session.pid()});
        self.daemon
            .sessions
            .lock()
            .await
            .insert(session.id().to_string(), session);

        let mut played = descriptor.clone();
        played.last_played = Some(now_seconds());
        let _ = self.daemon.instances.save(&played);

        Ok(reply)
    }

    async fn launch_status(&self) -> Result<Value, RpcError> {
        let sessions = self.daemon.sessions.lock().await;
        let running: Vec<Value> = sessions
            .values()
            .map(|session| json!({"sessionId": session.id(), "pid": session.pid()}))
            .collect();

        Ok(json!({"sessions": running}))
    }

    async fn launch_stop(&self, request: SessionParams) -> Result<Value, RpcError> {
        let sessions = self.daemon.sessions.lock().await;
        let session = sessions
            .get(&request.session_id)
            .ok_or_else(|| not_found(format!("no session {}", request.session_id)))?;

        session.terminate().map_err(internal)?;
        Ok(json!({}))
    }

    async fn log_tail(&self, request: SessionParams) -> Result<Value, RpcError> {
        let sessions = self.daemon.sessions.lock().await;
        let session = sessions
            .get(&request.session_id)
            .ok_or_else(|| not_found(format!("no session {}", request.session_id)))?;

        let lines: Vec<Value> = session
            .log()
            .snapshot()
            .iter()
            .map(|line| {
                json!({
                    "stream": match line.stream {
                        acelus_launch::Stream::Stdout => "stdout",
                        acelus_launch::Stream::Stderr => "stderr",
                    },
                    "line": line.text,
                })
            })
            .collect();

        Ok(json!({"lines": lines}))
    }
}

fn write_lockfile(layout: &InstanceLayout, lockfile: &Lockfile) -> Result<(), RpcError> {
    let json = lockfile.to_json().map_err(internal)?;
    std::fs::write(layout.lockfile(), json).map_err(internal)
}

fn read_lockfile(layout: &InstanceLayout) -> Result<Lockfile, RpcError> {
    let bytes = std::fs::read(layout.lockfile()).map_err(|_| {
        RpcError::new(
            codes::NOT_FOUND,
            "this instance is not installed yet; run acelus install first",
        )
    })?;

    Lockfile::parse(&bytes).map_err(internal)
}
