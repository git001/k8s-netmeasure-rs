//! Control-port client of `run` (design §7): one HTTP/1.1 keep-alive connection per
//! target on a `current_thread` runtime owned by the main thread, used via `block_on`
//! between cycles only. Every request has a 2 s timeout; a failed request is retried
//! once on a new connection. Bodies are parsed with `deny_unknown_fields`.

use std::collections::HashMap;
use std::net::SocketAddrV4;
use std::time::Duration;

use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::client::conn::http1::{self, SendRequest};
use hyper::{Request, StatusCode, header};
use hyper_util::rt::TokioIo;
use serde::de::DeserializeOwned;

const TIMEOUT: Duration = Duration::from_secs(2);

pub struct Client {
    runtime: tokio::runtime::Runtime,
    connections: HashMap<SocketAddrV4, SendRequest<Empty<Bytes>>>,
}

impl Client {
    pub fn new() -> std::io::Result<Client> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()?;
        Ok(Client {
            runtime,
            connections: HashMap::new(),
        })
    }

    /// `GET path` on `addr`, parsed as `T`.
    pub fn get<T: DeserializeOwned>(
        &mut self,
        addr: SocketAddrV4,
        path: &str,
    ) -> Result<T, String> {
        let first = self.try_get(addr, path);
        let body = match first {
            Ok(b) => b,
            Err(e) => {
                tracing::debug!(addr = %addr, path, error = %e, "control request failed, reconnecting");
                self.connections.remove(&addr);
                self.try_get(addr, path)?
            }
        };
        serde_json::from_slice(&body).map_err(|e| format!("{addr}{path}: unexpected body: {e}"))
    }

    fn try_get(&mut self, addr: SocketAddrV4, path: &str) -> Result<Bytes, String> {
        let existing = self.connections.remove(&addr);
        let request = Request::get(path)
            .header(header::HOST, addr.to_string())
            .body(Empty::new())
            .map_err(|e| e.to_string())?;
        let runtime = &self.runtime;
        let (sender, body) = runtime.block_on(async {
            tokio::time::timeout(TIMEOUT, async {
                let mut sender = match existing {
                    Some(s) if !s.is_closed() => s,
                    _ => connect(addr).await?,
                };
                sender.ready().await.map_err(|e| e.to_string())?;
                let response = sender
                    .send_request(request)
                    .await
                    .map_err(|e| e.to_string())?;
                let status = response.status();
                let body = response
                    .into_body()
                    .collect()
                    .await
                    .map_err(|e| e.to_string())?;
                if status != StatusCode::OK {
                    return Err(format!("{addr}{path}: HTTP {status}"));
                }
                Ok((sender, body.to_bytes()))
            })
            .await
            .map_err(|_| format!("{addr}{path}: no answer within {TIMEOUT:?}"))?
        })?;
        self.connections.insert(addr, sender);
        Ok(body)
    }
}

async fn connect(addr: SocketAddrV4) -> Result<SendRequest<Empty<Bytes>>, String> {
    let stream = tokio::net::TcpStream::connect(addr)
        .await
        .map_err(|e| format!("{addr}: {e}"))?;
    let (sender, connection) = http1::handshake(TokioIo::new(stream))
        .await
        .map_err(|e| e.to_string())?;
    tokio::spawn(connection);
    Ok(sender)
}
