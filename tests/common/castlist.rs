//! 偽の castlist。試験の中で 127.0.0.1 の空いている port に待ち受け、`POST /v1/resolve` に決まった答えを返す。
//! 受けた要求は、そのまま残す（頭も本文も）。

use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// castlist が知っている送り手（付随する id → アクター）。ほかの付随する id は `actor: null`。
pub const ACTOR_A: &str = "019a0000-0000-7000-8000-00000000000a";
pub const ACTOR_B: &str = "019a0000-0000-7000-8000-00000000000b";
pub const NAME_A: &str = "t-23zs.swe.1";
pub const NAME_B: &str = "pm.1";
/// 偽の castlist が答えの `as_of` に書く時刻。
pub const AS_OF: &str = "2026-11-22T04:00:02.000Z";

fn actor(actor_ref: &str) -> Value {
    match actor_ref {
        "claude.session:a" => json!({ "id": ACTOR_A, "name": NAME_A, "kind": "ai" }),
        "tpm.person:pm" => json!({ "id": ACTOR_B, "name": NAME_B, "kind": "human" }),
        _ => Value::Null,
    }
}

pub struct Fake {
    /// `http://127.0.0.1:<port>`。
    pub url: String,
    /// 受けた要求（頭と本文）。
    pub requests: Arc<Mutex<Vec<String>>>,
}

impl Fake {
    /// すぐ答える。
    pub async fn start() -> Self {
        Self::answering(200, Duration::ZERO).await
    }

    /// `delay` だけ待ってから答える。
    pub async fn slow(delay: Duration) -> Self {
        Self::answering(200, delay).await
    }

    /// `status` で答える（200 のほかは、本文を `{"error":"…"}` にする）。
    pub async fn answering(status: u16, delay: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let kept = requests.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let kept = kept.clone();
                tokio::spawn(async move {
                    let raw = read_request(&mut stream).await;
                    kept.lock().unwrap().push(raw.clone());
                    tokio::time::sleep(delay).await;
                    let body = if status == 200 {
                        answer(&raw).to_string()
                    } else {
                        json!({ "error": "混んでいる" }).to_string()
                    };
                    let response = format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        Self { url, requests }
    }

    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

/// 待ち受けていない castlist の URL（空いている port を取って、すぐ閉じる）。
pub fn closed_url() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    url
}

/// 頭と、`content-length` の分の本文を読む。
async fn read_request(stream: &mut tokio::net::TcpStream) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let text = String::from_utf8_lossy(&buf).into_owned();
        if let Some((head, body)) = text.split_once("\r\n\r\n") {
            let length = head
                .lines()
                .find_map(|l| {
                    let (name, value) = l.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            if body.len() >= length {
                return text;
            }
        }
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return text,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

/// 要求の本文の `refs` ごとに、知っているアクターか null を返す。
fn answer(raw: &str) -> Value {
    let body = raw.split_once("\r\n\r\n").map_or("", |(_, b)| b);
    let asked: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let results: Vec<Value> = asked["refs"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|r| json!({ "ref": r, "actor": actor(r.as_str().unwrap_or("")) }))
        .collect();
    json!({ "as_of": AS_OF, "results": results })
}
