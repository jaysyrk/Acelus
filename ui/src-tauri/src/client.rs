use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{oneshot, Mutex};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("acelusd is not running")]
    NotRunning,

    #[error("the connection to acelusd was lost")]
    Disconnected,

    #[error("could not determine where Acelus keeps its data")]
    NoDataDirectory,

    #[error("{message}")]
    Rpc { code: i32, message: String },

    #[error("acelusd sent a reply Acelus could not read")]
    Malformed,

    #[error("i/o failed talking to acelusd")]
    Io(#[from] std::io::Error),
}

impl serde::Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let code = match self {
            Error::Rpc { code, .. } => *code,
            _ => 0,
        };

        let mut state = serializer.serialize_struct("Error", 2)?;
        state.serialize_field("code", &code)?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

pub type Result<T> = std::result::Result<T, Error>;

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;
type Writer = Arc<Mutex<Option<tokio::net::unix::OwnedWriteHalf>>>;

pub struct Client {
    writer: Writer,
    pending: Pending,
    next_id: AtomicU64,
}

pub fn socket_path() -> Result<PathBuf> {
    if let Ok(explicit) = std::env::var("ACELUS_SOCKET") {
        if !explicit.trim().is_empty() {
            return Ok(PathBuf::from(explicit));
        }
    }
    if let Ok(home) = std::env::var("ACELUS_HOME") {
        if !home.trim().is_empty() {
            return Ok(PathBuf::from(home).join("acelusd.sock"));
        }
    }
    dirs::data_dir()
        .map(|dir| dir.join("acelus").join("acelusd.sock"))
        .ok_or(Error::NoDataDirectory)
}

impl Client {
    pub fn new() -> Self {
        Self {
            writer: Arc::new(Mutex::new(None)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_id: AtomicU64::new(1),
        }
    }

    pub async fn is_connected(&self) -> bool {
        self.writer.lock().await.is_some()
    }

    pub async fn connect(
        &self,
        on_notification: impl Fn(String, Value) + Send + 'static,
    ) -> Result<()> {
        let path = socket_path()?;
        let stream = tokio::net::UnixStream::connect(&path)
            .await
            .map_err(|_| Error::NotRunning)?;

        let (read_half, write_half) = stream.into_split();
        *self.writer.lock().await = Some(write_half);

        let pending = Arc::clone(&self.pending);
        let writer = Arc::clone(&self.writer);
        tokio::spawn(async move {
            let mut lines = BufReader::new(read_half).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Ok(message) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };

                match message.get("id").and_then(numeric_id) {
                    Some(id) => {
                        if let Some(sender) = pending.lock().await.remove(&id) {
                            let _ = sender.send(message);
                        }
                    }
                    None => {
                        if let Some(method) = message.get("method").and_then(Value::as_str) {
                            let params = message.get("params").cloned().unwrap_or(Value::Null);
                            on_notification(method.to_string(), params);
                        }
                    }
                }
            }

            *writer.lock().await = None;
            for (_, sender) in pending.lock().await.drain() {
                let _ = sender.send(Value::Null);
            }
        });

        Ok(())
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().await.insert(id, sender);

        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let mut line = serde_json::to_string(&request).map_err(|_| Error::Malformed)?;
        line.push('\n');

        {
            let mut guard = self.writer.lock().await;
            let writer = guard.as_mut().ok_or(Error::NotRunning)?;
            if writer.write_all(line.as_bytes()).await.is_err() {
                self.pending.lock().await.remove(&id);
                return Err(Error::Disconnected);
            }
            writer.flush().await.map_err(|_| Error::Disconnected)?;
        }

        let reply = receiver.await.map_err(|_| Error::Disconnected)?;
        if reply.is_null() {
            return Err(Error::Disconnected);
        }

        if let Some(error) = reply.get("error") {
            return Err(Error::Rpc {
                code: error.get("code").and_then(Value::as_i64).unwrap_or(0) as i32,
                message: error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("acelusd refused the request")
                    .to_string(),
            });
        }

        Ok(reply.get("result").cloned().unwrap_or(Value::Null))
    }
}

fn numeric_id(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}
