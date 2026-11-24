//! castlist（送り手の本当の id と名を持つ、pagetray を動かす機械の中の別のサーバー）に、送り手を聞く。
//!
//! - 聞くのは `ResolveActors`（`POST /api/resolve`）の 1 つ。castlist の定義から作った client（crates/castlist-client/）で呼ぶ。
//! - HTTP の client は、TLS の無い HTTP/1.1（`aws-smithy-http-client` の hyper の作りの `build_http`）。castlist は同じ機械の中の
//!   `http://` だけなので、TLS の crate を build しない。接続は使い回さない（1 度ごとにつなぐ。古い接続で落ちて送り直す、が起きない）。
//! - 待つのは 1 度に [`TIMEOUT`]（つなぐ・送る・答えを読み終えるまでの全部）。送り直さない（今の pagetray と同じ）。
//! - castlist は `Host` が 127.0.0.1・localhost でない要求と、`Origin` の付いた書く要求を断る。だから、つなぐ先がどこでも
//!   `Host: 127.0.0.1` で送り、`Origin` は付けない。
use std::time::Duration;

use castlist_client::Client;
use castlist_client::config::retry::RetryConfig;
use castlist_client::config::timeout::TimeoutConfig;
use castlist_client::config::{BehaviorVersion, Config};
use castlist_client::error::SdkError;

/// 1 度に待つ長さ（つなぐ・送る・答えを読み終えるまでの全部）。過ぎれば聞けなかったとする。
pub const TIMEOUT: Duration = Duration::from_secs(1);

/// 画面の link の頭の既定。本番の castlist を、host の browser から開く所。
pub const DEFAULT_LINK: &str = "http://127.0.0.1:8802";

/// castlist の居場所と、聞く client。聞く先（`PAGETRAY_CASTLIST_URL`）と、画面の link の頭（`PAGETRAY_CASTLIST_LINK_URL`）は別。
/// pagetray は container の中から castlist に聞き、link は Product Manager が host の browser で開くので、届く所が違う。
#[derive(Clone, Debug)]
pub struct Castlist {
    /// 聞く先の `http://<host>:<port>`。
    api: String,
    /// 画面から castlist のアクターを開く URL の頭（`http://<host>:<port>`）。
    link: String,
    client: Client,
}

/// castlist が答えたアクター（形は見てある）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Actor {
    pub id: String,
    pub name: String,
    pub kind: String,
}

/// castlist の答え（登録のある送り手）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub actor: Actor,
    /// castlist が答えた時刻（答えの `asOf`。[`crate::db::stamp`] の字）。
    pub as_of: String,
}

impl Castlist {
    /// 環境変数（`PAGETRAY_CASTLIST_URL`・`PAGETRAY_CASTLIST_LINK_URL`）から作る（[`Castlist::configure`]）。
    pub fn from_env() -> Result<Option<Self>, String> {
        Self::configure(
            std::env::var("PAGETRAY_CASTLIST_URL").ok().as_deref(),
            std::env::var("PAGETRAY_CASTLIST_LINK_URL").ok().as_deref(),
        )
    }

    /// 環境変数の値から作る。`api`（`PAGETRAY_CASTLIST_URL`）が無い・空なら、castlist に聞かない（`None`）。
    /// `link`（`PAGETRAY_CASTLIST_LINK_URL`）が無い・空なら [`DEFAULT_LINK`]。
    /// どちらも `http://<host>:<port>` の形でなければ断る（`api` が無くても、書いた `link` は見る）。
    pub fn configure(api: Option<&str>, link: Option<&str>) -> Result<Option<Self>, String> {
        let link = match link.filter(|v| !v.is_empty()) {
            Some(url) => parse_base(url)
                .map(|addr| format!("http://{addr}"))
                .map_err(|rule| format!("PAGETRAY_CASTLIST_LINK_URL は {rule}"))?,
            None => DEFAULT_LINK.to_owned(),
        };
        let Some(api) = api.filter(|v| !v.is_empty()) else {
            return Ok(None);
        };
        let addr = parse_base(api).map_err(|rule| format!("PAGETRAY_CASTLIST_URL は {rule}"))?;
        let api = format!("http://{addr}");
        let http = aws_smithy_http_client::Builder::new()
            .pool_max_idle_per_host(0)
            .build_http();
        let config = Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .endpoint_url(&api)
            .http_client(http)
            .retry_config(RetryConfig::disabled())
            .timeout_config(TimeoutConfig::builder().operation_timeout(TIMEOUT).build())
            .build();
        Ok(Some(Self {
            api,
            link,
            client: Client::from_conf(config),
        }))
    }

    /// 聞く先（`http://<host>:<port>`）。
    pub fn api(&self) -> &str {
        &self.api
    }

    /// 画面から castlist のアクターを開く URL の頭（`http://<host>:<port>`）。
    pub fn link(&self) -> &str {
        &self.link
    }

    /// 付随する id の送り手を聞く。登録が無ければ `None`。聞けなければ（つながらない・1 秒で答えない・2xx でない・
    /// 答えが読めない・答えの形が合わない）、その訳。
    pub async fn resolve(&self, issuer: &str, actor_ref: &str) -> Result<Option<Answer>, String> {
        let output = self
            .client
            .resolve_actors()
            .issuer(issuer)
            .refs(actor_ref)
            .customize()
            .mutate_request(|request| {
                request.headers_mut().insert("host", "127.0.0.1");
            })
            .send()
            .await
            .map_err(|e| match e {
                SdkError::TimeoutError(_) => "1 秒で答えが無い".to_owned(),
                SdkError::DispatchFailure(e) if e.is_timeout() => "1 秒で答えが無い".to_owned(),
                SdkError::DispatchFailure(_) => "つながらない".to_owned(),
                SdkError::ServiceError(e) if !e.raw().status().is_success() => {
                    format!("castlist が {} を返した", e.raw().status())
                }
                SdkError::ServiceError(_) | SdkError::ResponseError(_) => {
                    "答えが読めない".to_owned()
                }
                e => format!("{}", castlist_client::error::DisplayErrorContext(&e)),
            })?;
        let unreadable = || "答えの形が合わない".to_owned();
        let found = output
            .results()
            .iter()
            .find(|r| r.r#ref() == actor_ref)
            .ok_or_else(|| "答えに、聞いた送り手が無い".to_owned())?;
        let as_of = as_of(output.as_of()).ok_or_else(unreadable)?;
        let Some(actor) = found.actor() else {
            return Ok(None);
        };
        let ok = is_actor_id(actor.id())
            && matches!(actor.kind(), "ai" | "human")
            && (1..=128).contains(&actor.name().chars().count())
            && !actor.name().chars().any(char::is_control);
        if !ok {
            return Err(unreadable());
        }
        Ok(Some(Answer {
            actor: Actor {
                id: actor.id().to_owned(),
                name: actor.name().to_owned(),
                kind: actor.kind().to_owned(),
            },
            as_of,
        }))
    }
}

/// 答えの時刻を、残す字にする。生成した client は、答えに無い `@required` の欄を既定の値で埋める（`asOf` は 1970 年の初め）
/// ので、その時刻は無い物とする。0001〜9999 年の外も読めない物とする。
fn as_of(t: &aws_smithy_types::DateTime) -> Option<String> {
    if t.secs() == 0 && t.subsec_nanos() == 0 {
        return None;
    }
    time::OffsetDateTime::from_unix_timestamp_nanos(t.as_nanos())
        .ok()
        .filter(|t| (1..=9999).contains(&t.year()))
        .map(crate::db::stamp)
}

/// castlist のアクターの id の形（`ai.common#ActorId`: 小文字の 16 進の UUID の字）。画面の link の path に入るので見る。
fn is_actor_id(s: &str) -> bool {
    let groups: Vec<&str> = s.split('-').collect();
    groups.len() == 5
        && groups.iter().zip([8, 4, 4, 4, 12]).all(|(g, len)| {
            g.len() == len && g.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        })
}

/// `http://<host>:<port>`（後ろの `/` は無くてよい）の `<host>:<port>`。path・利用者の名・`https://` は受けない。
fn parse_base(url: &str) -> Result<String, String> {
    let rule = || format!("http://<host>:<port> の形（{url}）");
    let addr = url
        .strip_prefix("http://")
        .map(|rest| rest.strip_suffix('/').unwrap_or(rest))
        .ok_or_else(rule)?;
    let (host, port) = addr.rsplit_once(':').ok_or_else(rule)?;
    let host_ok = !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'));
    let port_ok =
        port.bytes().all(|b| b.is_ascii_digit()) && port.parse::<u16>().is_ok_and(|p| p > 0);
    if !host_ok || !port_ok {
        return Err(rule());
    }
    Ok(addr.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_ids_are_lowercase_uuids() {
        assert!(is_actor_id("019a0000-0000-7000-8000-00000000000a"));
        for ng in [
            "",
            "019A0000-0000-7000-8000-00000000000a",
            "019a0000-0000-7000-8000-00000000000",
            "019a0000-0000-7000-8000-00000000000a-",
            "../0000-0000-7000-8000-00000000000a",
            "019a00000000-7000-8000-00000000000a",
        ] {
            assert!(!is_actor_id(ng), "{ng}");
        }
    }

    #[test]
    fn a_missing_as_of_is_not_a_time() {
        assert_eq!(as_of(&aws_smithy_types::DateTime::from_secs(0)), None);
        assert_eq!(
            as_of(&aws_smithy_types::DateTime::from_secs(1_795_478_400)),
            Some("2026-11-24T00:00:00.000Z".to_owned())
        );
    }
}
