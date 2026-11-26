//! 置き場（pagetray の server）に HTTP で頼む。
//!
//! - 生成した操作（`/healthz`・`/api/…` の定義の道）は、定義から作った client（`pagetray_client`）で呼ぶ。
//! - 手の道（ページの本文・file の中身・新着の知らせ・`api` の命令）は、同じ HTTP の client（aws-smithy-http-client の
//!   hyper の作りの connector）で直に呼ぶ。
//! - どちらも、TLS の無い HTTP/1.1・接続を使い回さない・送り直さない・環境変数の proxy を使わない・`Host: 127.0.0.1:<port>`
//!   （置き場は Host が 127.0.0.1・localhost でない要求を断る。つなぐ先の名がどうでも届くように。port は `--url` の物で、
//!   置き場が答えの `url`・`Location` を Host で組むので、開ける link になる）・`Origin` を付けない。
//! - 生成した層が知らない道に返す 404 `UnknownOperationException`（定義より古い置き場）は code 4。

use aws_smithy_http_client::Connector;
use aws_smithy_runtime_api::client::http::HttpConnector;
use aws_smithy_runtime_api::client::interceptors::Intercept;
use aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextMut;
use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
use aws_smithy_runtime_api::client::result::SdkError;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_runtime_api::http::{Headers, Request};
use aws_smithy_types::body::SdkBody;
use aws_smithy_types::config_bag::ConfigBag;
use aws_smithy_types::error::display::DisplayErrorContext;
use aws_smithy_types::error::metadata::ProvideErrorMetadata;
use http_body_util::BodyExt;
use pagetray_client::config::retry::RetryConfig;
use pagetray_client::config::timeout::TimeoutConfig;
use pagetray_client::config::{BehaviorVersion, Config};
use std::time::Duration;

/// 終わりの code: 置き場が断った（4xx・5xx。無い・消したページを含む）。
pub const REFUSED: u8 = 1;
/// 終わりの code: 使い方の誤り（clap が引数の誤りに使う code と同じ）。
pub const USAGE: u8 = 2;
/// 終わりの code: 置き場が応えない。
pub const DOWN: u8 = 3;
/// 終わりの code: 置き場の API が、この CLI の話す形でない（定義より古い置き場）。
pub const API: u8 = 4;

/// ふだんの命令が、置き場の答えを待つ時間（つなぐ・送る・答えを読み終えるまでの全部）。
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// 1 つの答えで読む上限（ページ 1 枚の上限 16 MiB より大きく取る）。
const MAX_REPLY: usize = 64 * 1024 * 1024;
/// 生成した層が、知らない道に返す誤りの種類。
const UNKNOWN_OPERATION: &str = "UnknownOperationException";

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

/// 置き場。生成した client と、手の道を呼ぶ connector。
pub struct Store {
    pub api: pagetray_client::Client,
    http: Connector,
    /// 置き場の URL（末尾の `/` は無い）。
    pub base: String,
    /// 送る `Host`（`127.0.0.1` に、`--url` の port があれば付けた物）。
    host: String,
}

/// 手の道の答えの、生成した層の知らない道の 404 `UnknownOperationException` の見方。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// CLI が自分で組んだ道。知らない道は、定義より古い置き場（code 4）。
    Own,
    /// 人が書いた道（`api` の命令）。知らない道は打ち間違いが多いので、ほかの断りと同じく答えのまま返す。
    Typed,
}

/// 手の道の答え。
pub struct Reply {
    pub status: u16,
    /// 誤りの種類（header `X-Amzn-Errortype`）。
    pub error_type: Option<String>,
    body: SdkBody,
    /// 本文を読み終える期限（頼んでからの [`TIMEOUT`]）。`None` なら待ち続ける。
    deadline: Option<tokio::time::Instant>,
}

impl Store {
    pub fn new(url: &str) -> Result<Self, Failure> {
        let base = url.trim_end_matches('/').to_owned();
        if !base.starts_with("http://") {
            return Err(Failure::usage(format!(
                "置き場の URL は http:// で始める（相手は、この Mac の中の置き場だけ）: {url}"
            )));
        }
        let host = host_of(&base);
        let config = Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .endpoint_url(&base)
            .http_client(
                aws_smithy_http_client::Builder::new()
                    .pool_max_idle_per_host(0)
                    .build_http(),
            )
            .retry_config(RetryConfig::disabled())
            .timeout_config(TimeoutConfig::builder().operation_timeout(TIMEOUT).build())
            .interceptor(Wire { host: host.clone() })
            .build();
        let http = Connector::builder().pool_max_idle_per_host(0).build_http();
        Ok(Self {
            api: pagetray_client::Client::from_conf(config),
            http,
            base,
            host,
        })
    }

    /// 生成した操作の誤りを、終わりの code と訳にする。`what` は、何をしようとしたか。
    pub fn failure<E>(&self, what: &str, error: SdkError<E, HttpResponse>) -> Failure
    where
        E: ProvideErrorMetadata + std::error::Error + 'static,
    {
        match &error {
            SdkError::ServiceError(e) => {
                let status = e.raw().status().as_u16();
                let kind = e.err().code().unwrap_or("?");
                if kind == UNKNOWN_OPERATION {
                    return unknown_operation(&self.base);
                }
                let message = e.err().message().unwrap_or_default();
                Failure::new(REFUSED, format!("{what}（{status} {kind}）: {message}"))
            }
            SdkError::TimeoutError(_) => self.down("10 秒で答えが無い"),
            SdkError::DispatchFailure(e) if e.is_timeout() => self.down("10 秒で答えが無い"),
            SdkError::DispatchFailure(_) => self.down(&DisplayErrorContext(&error).to_string()),
            SdkError::ConstructionFailure(_) => {
                Failure::usage(format!("要求を組めない: {}", DisplayErrorContext(&error)))
            }
            _ => Failure::new(
                DOWN,
                format!(
                    "置き場（{}）の答えが、API の形でない（置き場の URL が違う？）: {}",
                    self.base,
                    DisplayErrorContext(&error)
                ),
            ),
        }
    }

    fn down(&self, why: &str) -> Failure {
        Failure::new(DOWN, format!("置き場（{}）が応えない: {why}", self.base))
    }

    /// 手の道を呼ぶ。`path` は `/` で始まる、置き場の中の path。`body` は、本文と `Content-Type`。
    /// 待つのは、頼んでから本文を読み終えるまでで `timeout`。`None` なら待ち続ける（新着の知らせ）。
    pub async fn send(
        &self,
        route: Route,
        method: &str,
        path: &str,
        query: &[(&str, &str)],
        body: Option<(Vec<u8>, &str)>,
        timeout: Option<Duration>,
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
        let bad =
            |e: &dyn std::fmt::Display| Failure::usage(format!("要求を組めない（{uri}）: {e}"));
        let mut request = match body {
            Some((bytes, content_type)) => {
                let mut request = Request::new(SdkBody::from(bytes));
                // header にできない値（改行など）は、使い方の誤り（insert は panic する）。
                request
                    .headers_mut()
                    .try_insert("content-type", content_type.to_owned())
                    .map_err(|e| {
                        Failure::usage(format!(
                            "本文の種類（--content-type）を header にできない（{content_type:?}）: {e}"
                        ))
                    })?;
                request
            }
            None => Request::new(SdkBody::empty()),
        };
        request.set_method(method).map_err(|e| bad(&e))?;
        request.set_uri(uri.as_str()).map_err(|e| bad(&e))?;
        request.headers_mut().insert("host", self.host.clone());
        let deadline = timeout.map(|limit| tokio::time::Instant::now() + limit);
        let call = self.http.call(request);
        let response = match deadline {
            Some(at) => tokio::time::timeout_at(at, call)
                .await
                .map_err(|_| self.down("10 秒で答えが無い"))?,
            None => call.await,
        }
        .map_err(|e| self.down(&DisplayErrorContext(&e).to_string()))?;
        let error_type = error_type(response.headers());
        if route == Route::Own && error_type.as_deref() == Some(UNKNOWN_OPERATION) {
            return Err(unknown_operation(&self.base));
        }
        Ok(Reply {
            status: response.status().as_u16(),
            error_type,
            body: response.into_body(),
            deadline,
        })
    }
}

impl Reply {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// 2xx ならそのまま返す。違えば、置き場の言う訳（restJson1 の `message`）を付けて止まる。
    pub async fn ok(self, what: &str) -> Result<Self, Failure> {
        if self.is_success() {
            return Ok(self);
        }
        let status = self.status;
        let kind = self.error_type.clone().unwrap_or_else(|| "?".to_owned());
        let body = self.bytes().await.unwrap_or_default();
        let reason = serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["message"].as_str().map(str::to_owned))
            .unwrap_or_else(|| String::from_utf8_lossy(&body).trim().to_owned());
        Err(Failure::new(
            REFUSED,
            format!("{what}（{status} {kind}）: {reason}"),
        ))
    }

    /// 本文を読み終える（[`MAX_REPLY`] まで。頼んだ時の期限まで）。
    pub async fn bytes(self) -> Result<Vec<u8>, Failure> {
        let collect = http_body_util::Limited::new(self.body, MAX_REPLY).collect();
        let collected = match self.deadline {
            Some(at) => tokio::time::timeout_at(at, collect)
                .await
                .map_err(|_| Failure::new(DOWN, "置き場の答えを読めない: 10 秒で読み終えない"))?,
            None => collect.await,
        };
        collected
            .map(|c| c.to_bytes().to_vec())
            .map_err(|e| Failure::new(DOWN, format!("置き場の答えを読めない: {e}")))
    }

    /// 本文を、届いた分ずつ読む。`idle` のあいだ何も届かなければ、止まった置き場として code 3。終われば `None`。
    /// 頼んだ時の期限（本文を読み終えるまで）は見ない（大きな DB の写しは、送り終えるまで長くかかってよい）。
    pub async fn chunk_within(&mut self, idle: Duration) -> Result<Option<Vec<u8>>, Failure> {
        let seconds = idle.as_secs();
        loop {
            let frame = tokio::time::timeout(idle, self.body.frame())
                .await
                .map_err(|_| {
                    Failure::new(
                        DOWN,
                        format!("置き場の答えが {seconds} 秒届かない（途中で止まった）"),
                    )
                })?;
            let Some(frame) = frame else {
                return Ok(None);
            };
            let frame =
                frame.map_err(|e| Failure::new(DOWN, format!("置き場の答えを読めない: {e}")))?;
            if let Ok(data) = frame.into_data() {
                return Ok(Some(data.to_vec()));
            }
        }
    }

    /// 本文を、届いた分ずつ読む（新着の知らせ）。置き場がきれいに閉じれば `None`。途中で切れた（置き場が落ちた・
    /// 接続が切れた）時は code 3。
    pub async fn chunk(&mut self) -> Result<Option<Vec<u8>>, Failure> {
        loop {
            let Some(frame) = self.body.frame().await else {
                return Ok(None);
            };
            let frame = frame
                .map_err(|e| Failure::new(DOWN, format!("置き場の答えが途中で切れた: {e}")))?;
            if let Ok(data) = frame.into_data() {
                return Ok(Some(data.to_vec()));
            }
        }
    }
}

/// 生成した client の要求に掛ける決まり（`Host` を 127.0.0.1 と `--url` の port にする）。
#[derive(Debug)]
struct Wire {
    host: String,
}

impl Intercept for Wire {
    fn name(&self) -> &'static str {
        "pagetray の決まり"
    }

    fn modify_before_transmit(
        &self,
        context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _: &RuntimeComponents,
        _: &mut ConfigBag,
    ) -> Result<(), aws_smithy_runtime_api::box_error::BoxError> {
        context
            .request_mut()
            .headers_mut()
            .insert("host", self.host.clone());
        Ok(())
    }
}

fn unknown_operation(base: &str) -> Failure {
    Failure::new(
        API,
        format!(
            "置き場（{base}）が、この CLI（pagetray {}）の呼ぶ操作を知らない（置き場の版が古い）。置き場を上げて、版を合わせる",
            env!("CARGO_PKG_VERSION")
        ),
    )
}

/// 送る `Host`: `127.0.0.1` に、`base`（`http://<名>[:<port>]`）の port があれば付けた物。
fn host_of(base: &str) -> String {
    let authority = base
        .strip_prefix("http://")
        .unwrap_or(base)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("");
    let port = authority
        .rsplit_once(':')
        .map(|(_, port)| port)
        .filter(|port| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()));
    match port {
        Some(port) => format!("127.0.0.1:{port}"),
        None => "127.0.0.1".to_owned(),
    }
}

fn error_type(headers: &Headers) -> Option<String> {
    headers
        .get("x-amzn-errortype")
        // restJson1 の種類は、`:` の後ろに URL などが付くことがある。
        .map(|v| v.split(':').next().unwrap_or(v).to_owned())
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
    use super::{encode, host_of};

    #[test]
    fn the_host_carries_the_port_of_the_url() {
        assert_eq!(host_of("http://127.0.0.1:8801"), "127.0.0.1:8801");
        assert_eq!(host_of("http://pagetray.box:8080/x"), "127.0.0.1:8080");
        assert_eq!(host_of("http://localhost"), "127.0.0.1");
        assert_eq!(host_of("http://[::1]"), "127.0.0.1");
    }

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(encode("AI-22_a.b~"), "AI-22_a.b~");
        assert_eq!(encode("a b&c=d"), "a%20b%26c%3Dd");
        assert_eq!(encode("計画"), "%E8%A8%88%E7%94%BB");
        assert_eq!(encode("workspace:1"), "workspace%3A1");
    }
}
