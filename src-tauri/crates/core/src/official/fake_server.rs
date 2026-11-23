//! A local fake official server for tests: the request line decides the reply. Compiled only in tests.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// One reply. With `length: false` it sends no Content-Length and reads until the connection closes
/// (simulates a chunked / length-less response).
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
    pub length: bool,
}

impl Reply {
    pub fn json(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into().into_bytes(),
            length: true,
        }
    }
}

/// Starts a fake server and returns the base URL and the request counter. `route` gets the
/// "path?query" from the request line.
pub async fn serve(
    route: impl Fn(&str) -> Reply + Send + Sync + 'static,
) -> (String, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let route = Arc::new(route);
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let counter = counter.clone();
            let route = route.clone();
            tokio::spawn(async move {
                let mut buffer = vec![0u8; 16 * 1024];
                let n = socket.read(&mut buffer).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buffer[..n]).to_string();
                let target = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                counter.fetch_add(1, Ordering::SeqCst);
                let reply = route(&target);
                let mut header = format!(
                    "HTTP/1.1 {} X\r\ncontent-type: application/json\r\nconnection: close\r\n",
                    reply.status
                );
                if reply.length {
                    header.push_str(&format!("content-length: {}\r\n", reply.body.len()));
                }
                header.push_str("\r\n");
                let _ = socket.write_all(header.as_bytes()).await;
                let _ = socket.write_all(&reply.body).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    (base, hits)
}

/// The value of one parameter in the query string.
pub fn query_param(target: &str, key: &str) -> Option<String> {
    let query = target.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then(|| value.to_string())
    })
}
