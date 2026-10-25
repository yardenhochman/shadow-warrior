//! Shared test helpers (compiled only for tests).
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

pub type Routes = Arc<Mutex<HashMap<String, (u16, String)>>>;
pub type Seen = Arc<Mutex<Vec<String>>>;

/// Mock HTTP device (WLED, Tasmota...): routes keyed by "METHOD /path"; the key
/// "*" matches any request not otherwise routed. Unknown routes give 404.
/// Routes can be changed while running to simulate a device coming up.
pub async fn mock_http(routes: &[(&str, u16, &str)]) -> (String, Routes, Seen) {
    let table: Routes = Arc::new(Mutex::new(
        routes.iter().map(|(k, c, b)| (k.to_string(), (*c, b.to_string()))).collect(),
    ));
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let (t, s) = (table.clone(), seen.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let (t, s) = (t.clone(), s.clone());
            tokio::spawn(async move {
                let mut buf = vec![0u8; 16384];
                let mut total = 0;
                loop {
                    let n = sock.read(&mut buf[total..]).await.unwrap_or(0);
                    total += n;
                    let text = String::from_utf8_lossy(&buf[..total]).to_string();
                    if let Some(h) = text.find("\r\n\r\n") {
                        let len = text
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if total >= h + 4 + len || n == 0 {
                            break;
                        }
                    } else if n == 0 {
                        break;
                    }
                }
                let req = String::from_utf8_lossy(&buf[..total]).to_string();
                let mut parts = req.split_whitespace();
                let key = format!("{} {}", parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                s.lock().await.push(req.clone());
                let (code, body) = {
                    let t = t.lock().await;
                    t.get(&key).or_else(|| t.get("*")).cloned().unwrap_or((404, String::new()))
                };
                let resp = format!(
                    "HTTP/1.1 {} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    code,
                    body.len(),
                    body
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    (addr, table, seen)
}

/// An address nothing listens on (connection refused).
pub async fn dead_addr() -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    l.local_addr().unwrap().to_string()
}
