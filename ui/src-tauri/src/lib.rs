mod client;

use std::sync::Arc;

use client::{Client, Error, Result};
use serde_json::Value;
use tauri::{Emitter, Manager};

const DAEMON_EVENT: &str = "daemon";

struct Bridge {
    client: Arc<Client>,
}

fn daemon_binary() -> String {
    if let Ok(explicit) = std::env::var("ACELUSD_BINARY") {
        if !explicit.trim().is_empty() {
            return explicit;
        }
    }

    if let Ok(self_path) = std::env::current_exe() {
        let beside = self_path.with_file_name(if cfg!(windows) {
            "acelusd.exe"
        } else {
            "acelusd"
        });
        if beside.is_file() {
            return beside.to_string_lossy().into_owned();
        }
    }

    "acelusd".to_string()
}

async fn start_daemon() -> Result<()> {
    let mut command = std::process::Command::new(daemon_binary());
    command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command.spawn().map_err(|_| Error::NotRunning)?;
    Ok(())
}

#[tauri::command]
async fn connect(app: tauri::AppHandle, bridge: tauri::State<'_, Bridge>) -> Result<bool> {
    if bridge.client.is_connected().await {
        return Ok(true);
    }

    let forward = {
        let app = app.clone();
        move |method: String, params: Value| {
            let _ = app.emit(
                DAEMON_EVENT,
                serde_json::json!({"method": method, "params": params}),
            );
        }
    };

    match bridge.client.connect(forward).await {
        Ok(()) => Ok(true),
        Err(Error::NotRunning) => {
            start_daemon().await?;
            for _ in 0..40 {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let forward = {
                    let app = app.clone();
                    move |method: String, params: Value| {
                        let _ = app.emit(
                            DAEMON_EVENT,
                            serde_json::json!({"method": method, "params": params}),
                        );
                    }
                };
                if bridge.client.connect(forward).await.is_ok() {
                    return Ok(true);
                }
            }
            Err(Error::NotRunning)
        }
        Err(other) => Err(other),
    }
}

#[tauri::command]
async fn rpc(method: String, params: Value, bridge: tauri::State<'_, Bridge>) -> Result<Value> {
    bridge.client.call(&method, params).await
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            app.manage(Bridge {
                client: Arc::new(Client::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![connect, rpc])
        .run(tauri::generate_context!())
        .expect("the Acelus window could not be created");
}
