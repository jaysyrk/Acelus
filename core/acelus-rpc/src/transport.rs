use std::sync::Arc;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use crate::protocol::{decode, encode, Notification, Response, RpcError};

#[async_trait::async_trait]
pub trait Handler: Send + Sync + 'static {
    async fn call(
        &self,
        method: &str,
        params: Value,
        notifier: &Notifier,
    ) -> std::result::Result<Value, RpcError>;
}

#[derive(Debug, Clone)]
pub struct Notifier {
    outbound: mpsc::UnboundedSender<String>,
}

impl Notifier {
    pub fn send(&self, notification: Notification) {
        let _ = self.outbound.send(encode(&notification));
    }

    pub fn notify(&self, method: impl Into<String>, params: Value) {
        self.send(Notification::new(method, params));
    }
}

pub async fn serve_connection<S>(stream: S, handler: Arc<dyn Handler>) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (reader, mut writer) = tokio::io::split(stream);
    let (outbound, mut inbox) = mpsc::unbounded_channel::<String>();
    let notifier = Notifier {
        outbound: outbound.clone(),
    };

    let pump = tokio::spawn(async move {
        while let Some(frame) = inbox.recv().await {
            if writer.write_all(frame.as_bytes()).await.is_err() {
                break;
            }
            if writer.flush().await.is_err() {
                break;
            }
        }
    });

    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let request = match decode(&line) {
            Ok(request) => request,
            Err(error) => {
                let _ = outbound.send(encode(&Response::failure(Value::Null, error)));
                continue;
            }
        };

        let id = request.id.clone();
        let outcome = handler
            .call(&request.method, request.params, &notifier)
            .await;

        let Some(id) = id else {
            continue;
        };

        let response = match outcome {
            Ok(result) => Response::success(id, result),
            Err(error) => Response::failure(id, error),
        };

        if outbound.send(encode(&response)).is_err() {
            break;
        }
    }

    drop(outbound);
    drop(notifier);
    let _ = pump.await;

    Ok(())
}

#[cfg(unix)]
pub async fn listen_unix(path: &std::path::Path, handler: Arc<dyn Handler>) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let _ = tokio::fs::remove_file(path).await;

    let listener = tokio::net::UnixListener::bind(path)?;

    loop {
        let (stream, _) = listener.accept().await?;
        let handler = handler.clone();
        tokio::spawn(async move {
            if let Err(error) = serve_connection(stream, handler).await {
                tracing::warn!(%error, "a client connection ended badly");
            }
        });
    }
}

#[cfg(windows)]
pub async fn listen_pipe(name: &str, handler: Arc<dyn Handler>) -> std::io::Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;

    loop {
        let server = ServerOptions::new().create(name)?;
        server.connect().await?;

        let handler = handler.clone();
        tokio::spawn(async move {
            if let Err(error) = serve_connection(server, handler).await {
                tracing::warn!(%error, "a client connection ended badly");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::AsyncBufReadExt;

    struct Echo;

    #[async_trait::async_trait]
    impl Handler for Echo {
        async fn call(
            &self,
            method: &str,
            params: Value,
            notifier: &Notifier,
        ) -> std::result::Result<Value, RpcError> {
            match method {
                "echo" => Ok(params),
                "announce" => {
                    notifier.notify("something.happened", json!({"seen": true}));
                    Ok(json!(null))
                }
                "explode" => Err(RpcError::internal("deliberate failure")),
                other => Err(RpcError::method_not_found(other)),
            }
        }
    }

    async fn roundtrip(requests: &str) -> Vec<Value> {
        let (client, server) = tokio::io::duplex(8192);
        tokio::spawn(serve_connection(server, Arc::new(Echo)));

        let (reader, mut writer) = tokio::io::split(client);
        writer.write_all(requests.as_bytes()).await.unwrap();
        writer.shutdown().await.unwrap();

        let mut lines = BufReader::new(reader).lines();
        let mut received = Vec::new();
        while let Some(line) = lines.next_line().await.unwrap() {
            received.push(serde_json::from_str(&line).unwrap());
        }
        received
    }

    #[tokio::test]
    async fn a_request_receives_a_response_carrying_its_id() {
        let replies =
            roundtrip("{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"echo\",\"params\":{\"a\":1}}\n")
                .await;

        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0]["id"], 7);
        assert_eq!(replies[0]["result"]["a"], 1);
    }

    #[tokio::test]
    async fn several_requests_are_answered_in_order() {
        let replies = roundtrip(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"echo\",\"params\":1}\n\
             {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"echo\",\"params\":2}\n",
        )
        .await;

        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0]["id"], 1);
        assert_eq!(replies[1]["id"], 2);
    }

    #[tokio::test]
    async fn a_notification_receives_no_response() {
        let replies = roundtrip("{\"jsonrpc\":\"2.0\",\"method\":\"echo\",\"params\":1}\n").await;
        assert!(replies.is_empty());
    }

    #[tokio::test]
    async fn an_unknown_method_is_reported_without_closing_the_connection() {
        let replies = roundtrip(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"nope\"}\n\
             {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"echo\",\"params\":true}\n",
        )
        .await;

        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0]["error"]["code"], crate::codes::METHOD_NOT_FOUND);
        assert_eq!(replies[1]["result"], true);
    }

    #[tokio::test]
    async fn a_malformed_line_is_reported_and_the_connection_survives() {
        let replies = roundtrip(
            "not json\n{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"echo\",\"params\":\"ok\"}\n",
        )
        .await;

        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0]["error"]["code"], crate::codes::PARSE_ERROR);
        assert_eq!(replies[1]["id"], 9);
    }

    #[tokio::test]
    async fn a_handler_error_becomes_an_error_response() {
        let replies = roundtrip("{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"explode\"}\n").await;

        assert_eq!(replies[0]["error"]["code"], crate::codes::INTERNAL);
        assert_eq!(replies[0]["error"]["message"], "deliberate failure");
        assert!(replies[0].get("result").is_none());
    }

    #[tokio::test]
    async fn notifications_from_a_handler_reach_the_client() {
        let replies = roundtrip("{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"announce\"}\n").await;

        let notification = replies
            .iter()
            .find(|reply| reply.get("method").is_some())
            .expect("the notification was delivered");
        assert_eq!(notification["method"], "something.happened");
        assert_eq!(notification["params"]["seen"], true);

        assert!(replies.iter().any(|reply| reply["id"] == 1));
    }

    #[tokio::test]
    async fn blank_lines_are_ignored() {
        let replies =
            roundtrip("\n\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"echo\",\"params\":0}\n")
                .await;
        assert_eq!(replies.len(), 1);
    }
}
