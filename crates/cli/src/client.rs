//! 置き場に HTTP で頼む。どの答えでも API の版（header `Pagetray-Api`）を見て、話せる版と違えば答えを使わずに止まる。

use pagetray_api::{API_HEADER, API_VERSION};
use std::time::Duration;
use ureq::http;

/// 終わりの code: 置き場が断った（4xx・5xx。無い・消したページを含む）。
pub const REFUSED: u8 = 1;
/// 終わりの code: 使い方の誤り（clap が引数の誤りに使う code と同じ）。
pub const USAGE: u8 = 2;
/// 終わりの code: 置き場が応えない。
pub const DOWN: u8 = 3;
/// 終わりの code: API の版が合わない。
pub const API: u8 = 4;

/// ふだんの命令が、置き場の答えを待つ時間。
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// 1 つの答えで読む上限（ページ 1 枚の上限 16 MiB より大きく取る）。
const MAX_REPLY: u64 = 64 * 1024 * 1024;

/// 止まる訳と、終わりの code。
#[derive(Debug)]
pub struct Failure {
    pub code: u8,
    pub message: String,
}

impl Failure {
    pub fn new(code: u8, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(USAGE, message)
    }
}

pub struct Client {
    agent: ureq::Agent,
    /// 置き場の URL（末尾の `/` は無い）。
    pub base: String,
}

/// 置き場の答え。本文は、読むまで取らない（写しや新着のように、流して読む物があるため）。
pub struct Reply {
    pub status: u16,
    body: ureq::Body,
}

impl Client {
    /// `timeout` が `None` なら、答えを待ち続ける（写し・新着）。
    pub fn new(url: &str, timeout: Option<Duration>) -> Result<Self, Failure> {
        let base = url.trim_end_matches('/').to_owned();
        if !base.starts_with("http://") {
            return Err(Failure::usage(format!(
                "置き場の URL は http:// で始める（相手は、この Mac の中の置き場だけ）: {url}"
            )));
        }
        let agent = ureq::Agent::config_builder()
            // 4xx・5xx も答えとして受け取り、置き場の言う訳を出す。
            .http_status_as_error(false)
            // 相手はこの Mac の中。環境変数の proxy を通さない。
            .proxy(None)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .timeout_global(timeout)
            .build()
            .into();
        Ok(Self { agent, base })
    }

    pub fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Reply, Failure> {
        self.send("GET", path, query, None)
    }

    /// `path` は `/` で始まる、置き場の中の path。`body` は、本文と `Content-Type`。
    pub fn send(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, &str)],
        body: Option<(&[u8], &str)>,
    ) -> Result<Reply, Failure> {
        let mut uri = format!("{}{path}", self.base);
        let mut separator = if path.contains('?') { '&' } else { '?' };
        for (name, value) in query {
            uri.push(separator);
            uri.push_str(&encode(name));
            uri.push('=');
            uri.push_str(&encode(value));
            separator = '&';
        }
        let builder = http::Request::builder().method(method).uri(&uri);
        let bad = |e: http::Error| Failure::usage(format!("要求を組めない（{uri}）: {e}"));
        let sent = match body {
            Some((bytes, content_type)) => {
                let request = builder
                    .header(http::header::CONTENT_TYPE, content_type)
                    .body(bytes)
                    .map_err(bad)?;
                self.agent.run(request)
            }
            None => self.agent.run(builder.body(()).map_err(bad)?),
        };
        let response = sent.map_err(|e| match e {
            ureq::Error::BadUri(_) | ureq::Error::Http(_) => {
                Failure::usage(format!("要求を組めない（{uri}）: {e}"))
            }
            e => Failure::new(DOWN, format!("置き場（{}）が応えない: {e}", self.base)),
        })?;
        // header が無い答えは、API の版ができる前の置き場の物。形は API の版 1 と同じなので通す。
        if let Some(theirs) = response.headers().get(API_HEADER) {
            let theirs = theirs.to_str().unwrap_or("?");
            if theirs != API_VERSION.to_string() {
                return Err(Failure::new(
                    API,
                    format!(
                        "置き場（{}）の API の版は {theirs} で、この pagetray（{}）が話せるのは {API_VERSION}。pagetray か置き場のどちらかを上げて、版を合わせる",
                        self.base,
                        env!("CARGO_PKG_VERSION")
                    ),
                ));
            }
        }
        let (parts, body) = response.into_parts();
        Ok(Reply {
            status: parts.status.as_u16(),
            body,
        })
    }
}

impl Reply {
    /// 2xx ならそのまま返す。違えば、置き場の言う訳（`{"error": "…"}`）を付けて止まる。
    pub fn ok(self, what: &str) -> Result<Self, Failure> {
        if (200..300).contains(&self.status) {
            return Ok(self);
        }
        let status = self.status;
        let body = self.bytes().unwrap_or_default();
        let reason = serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["error"].as_str().map(str::to_owned))
            .unwrap_or_else(|| String::from_utf8_lossy(&body).trim().to_owned());
        Err(Failure::new(
            REFUSED,
            format!("{what}（{status}）: {reason}"),
        ))
    }

    pub fn bytes(mut self) -> Result<Vec<u8>, Failure> {
        self.body
            .with_config()
            .limit(MAX_REPLY)
            .read_to_vec()
            .map_err(|e| Failure::new(DOWN, format!("置き場の答えを読めない: {e}")))
    }

    /// 本文を、流して読む（DB の写し・新着）。
    pub fn reader(self) -> impl std::io::Read {
        self.body.into_reader()
    }

    pub fn json<T: serde::de::DeserializeOwned>(self) -> Result<T, Failure> {
        serde_json::from_slice(&self.bytes()?).map_err(|e| {
            Failure::new(
                DOWN,
                format!("置き場の答えが、API の仕様の形でない（置き場の URL が違う？）: {e}"),
            )
        })
    }
}

/// query の名と値を、URL に書ける字にする（英数字と `-._~` のほかは `%XX`）。
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::encode;

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(encode("AI-22_a.b~"), "AI-22_a.b~");
        assert_eq!(encode("a b&c=d"), "a%20b%26c%3Dd");
        assert_eq!(encode("計画"), "%E8%A8%88%E7%94%BB");
        assert_eq!(encode("workspace:1"), "workspace%3A1");
    }
}
