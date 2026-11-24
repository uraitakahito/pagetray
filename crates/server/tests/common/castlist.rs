//! 偽の castlist。試験の中で 127.0.0.1 の空いている port に待ち受け、新しい castlist の `POST /api/resolve`
//! （restJson1。`{issuer, refs}` → `{asOf, results:[{ref, actor?}]}`）に決まった答えを返す。
//! 受けた要求は、そのまま残す（頭も本文も）。HTTP は TCP の上で直に話す（client が送った頭を、そのまま見るため）。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// castlist が知っている送り手（付随する id → アクター）。ほかの付随する id は、答えに `actor` が無い。
pub const ACTOR_A: &str = "019a0000-0000-7000-8000-00000000000a";
pub const ACTOR_B: &str = "019a0000-0000-7000-8000-00000000000b";
pub const NAME_A: &str = "t-23zs.swe.1";
pub const NAME_B: &str = "pm.1";
/// 偽の castlist が答えの `asOf` に書く時刻。
pub const AS_OF: &str = "2026-11-22T04:00:02Z";
/// `AS_OF` を pagetray が残す字（UTC・ミリ秒まで・24 字）。
pub const AS_OF_STORED: &str = "2026-11-22T04:00:02.000Z";

/// 知っている送り手のアクター（知らなければ無い）。
pub fn actor(actor_ref: &str) -> Option<Value> {
    match actor_ref {
        "claude.session:a" => Some(json!({ "id": ACTOR_A, "name": NAME_A, "kind": "ai" })),
        "tpm.person:pm" => Some(json!({ "id": ACTOR_B, "name": NAME_B, "kind": "human" })),
        _ => None,
    }
}

/// 要求の本文の `refs` ごとに、知っているアクターを入れた答え（知らない物は `actor` が無い）。
pub fn answer(asked: &Value) -> Value {
    let results: Vec<Value> = asked["refs"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|r| match actor(r.as_str().unwrap_or("")) {
            Some(actor) => json!({ "ref": r, "actor": actor }),
            None => json!({ "ref": r }),
        })
        .collect();
    json!({ "asOf": AS_OF, "results": results })
}

/// 偽の castlist の答え方。
pub enum Reply {
    /// 状態の数と本文（`content-length` を付ける）。
    Status(u16, String),
    /// 200 で、本文を 2 つの chunk に分けて送る（`transfer-encoding: chunked`）。
    Chunked(String),
}

type Respond = dyn Fn(&Value) -> Reply + Send + Sync;

pub struct Fake {
    /// `http://127.0.0.1:<port>`。
    pub url: String,
    /// 待ち受けている port。
    pub port: u16,
    /// 受けた要求（頭と本文）。
    requests: Arc<Mutex<Vec<String>>>,
}

impl Fake {
    /// すぐ、知っている送り手を答える。
    pub async fn start() -> Self {
        Self::slow(Duration::ZERO).await
    }

    /// `delay` だけ待ってから、知っている送り手を答える。
    pub async fn slow(delay: Duration) -> Self {
        Self::with(delay, |asked| Reply::Status(200, answer(asked).to_string())).await
    }

    /// `status` で答える（本文は castlist の誤りの形）。
    pub async fn answering(status: u16) -> Self {
        Self::with(Duration::ZERO, move |_| {
            Reply::Status(status, json!({ "message": "混んでいる" }).to_string())
        })
        .await
    }

    /// 200 で、決まった本文を答える（読めない答えを試す）。
    pub async fn body(body: impl Into<String>) -> Self {
        let body = body.into();
        Self::with(Duration::ZERO, move |_| Reply::Status(200, body.clone())).await
    }

    /// 要求の本文（JSON。読めなければ Null）から答えを作る `respond` で、`delay` だけ待ってから答える。
    pub async fn with(
        delay: Duration,
        respond: impl Fn(&Value) -> Reply + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{port}");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let kept = requests.clone();
        let respond: Arc<Respond> = Arc::new(respond);
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let kept = kept.clone();
                let respond = respond.clone();
                tokio::spawn(async move {
                    let raw = read_request(&mut stream).await;
                    if raw.is_empty() {
                        return;
                    }
                    kept.lock().unwrap().push(raw.clone());
                    tokio::time::sleep(delay).await;
                    let body = raw.split_once("\r\n\r\n").map_or("", |(_, b)| b);
                    let asked: Value = serde_json::from_str(body).unwrap_or(Value::Null);
                    let response = match respond(&asked) {
                        Reply::Status(status, body) => format!(
                            "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        ),
                        Reply::Chunked(body) => {
                            let (a, b) = body.split_at(body.len() / 2);
                            format!(
                                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n{:x}\r\n{a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n",
                                a.len(),
                                b.len()
                            )
                        }
                    };
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        Self {
            url,
            port,
            requests,
        }
    }

    /// 受けた要求（頭と本文）。
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
