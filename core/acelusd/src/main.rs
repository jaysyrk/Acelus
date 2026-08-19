mod handler;
mod state;

use std::sync::Arc;

use acelus_instance::Paths;

fn socket_path(paths: &Paths) -> std::path::PathBuf {
    if let Ok(explicit) = std::env::var("ACELUS_SOCKET") {
        if !explicit.trim().is_empty() {
            return std::path::PathBuf::from(explicit);
        }
    }
    paths.root().join("acelusd.sock")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("ACELUS_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let paths = Paths::discover()
        .ok_or_else(|| anyhow::anyhow!("could not determine a data directory for Acelus"))?;

    std::fs::create_dir_all(paths.root())?;

    let daemon = state::Daemon::open(paths.clone())?;
    let rpc = Arc::new(handler::Rpc::new(daemon));

    let socket = socket_path(&paths);
    tracing::info!(socket = %socket.display(), "acelusd listening");

    #[cfg(unix)]
    acelus_rpc::transport::listen_unix(&socket, rpc).await?;

    #[cfg(windows)]
    acelus_rpc::transport::listen_pipe(r"\\.\pipe\acelusd", rpc).await?;

    Ok(())
}
