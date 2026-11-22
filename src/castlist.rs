//! castlist（送り手の本当の id と名を持つ、pagetray を動かす機械の中の別のサーバー）に、送り手を聞く。
//! 聞くのは `POST /v1/resolve` の 1 つだけなので、HTTP/1.1 を TCP の上で直に話す（重い依存を入れない）。
//! castlist は `Host` が 127.0.0.1・localhost でない要求と、`Origin` の付いた書く要求を断る。だから、つなぐ先が
//! どこでも `Host: 127.0.0.1` で送り、`Origin` は付けない。

use pagetray_api::{ActorRecord, is_actor_id, is_name, is_timestamp};
use serde::Deserialize;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// 1 度に待つ長さ（つなぐ・送る・答えを読み終えるまでの全部）。過ぎれば聞けなかったとする。
pub const TIMEOUT: Duration = Duration::from_secs(1);
/// 読む答えの大きさの上限。
const MAX_ANSWER: usize = 64 * 1024;

/// castlist の居場所。聞く先（`PAGETRAY_CASTLIST_URL`）と、画面の link の頭（`PAGETRAY_CASTLIST_LINK_URL`）は別。
/// pagetray は container の中から castlist に聞き、link は Product Manager が host の browser で開くので、届く所が違う。
#[derive(Clone, Debug)]
pub struct Castlist {
    /// 聞く先（`<host>:<port>`）。
    addr: String,
    /// 聞く先の `http://<host>:<port>`。
    api: String,
    /// 画面から castlist のアクターを開く URL の頭（`http://<host>:<port>`）。
    link: String,
}

/// castlist の答え。`actor` が無ければ、castlist に登録の無い送り手。
#[derive(Debug, PartialEq, Eq)]
pub struct Answer {
    pub actor: Option<ActorRecord>,
    /// castlist が答えた時刻。
    pub as_of: String,
}

/// 画面の link の頭の既定。本番の castlist を、host の browser から開く所。
pub const DEFAULT_LINK: &str = "http://127.0.0.1:8802";

impl Castlist {
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
        Ok(Some(Self {
            api: format!("http://{addr}"),
            addr,
            link,
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

    /// 付随する id の送り手を聞く。聞けなければ（つながらない・1 秒で答えない・200 でない・答えが読めない）、その訳。
    pub async fn resolve(&self, issuer: &str, actor_ref: &str) -> Result<Answer, String> {
        let body = serde_json::json!({ "issuer": issuer, "refs": [actor_ref] }).to_string();
        let request = format!(
            "POST /v1/resolve HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let raw = tokio::time::timeout(TIMEOUT, self.exchange(request.as_bytes()))
            .await
            .map_err(|_| "1 秒で答えが無い".to_owned())??;
        parse_answer(&raw, actor_ref)
    }

    /// 送って、答えを読み終えるまで（`Content-Length` の分か、castlist が閉じるまで）。
    async fn exchange(&self, request: &[u8]) -> Result<Vec<u8>, String> {
        let mut stream = TcpStream::connect(&self.addr)
            .await
            .map_err(|e| format!("つながらない: {e}"))?;
        stream
            .write_all(request)
            .await
            .map_err(|e| format!("送れない: {e}"))?;
        let mut raw = Vec::new();
        let mut chunk = [0u8; 8192];
        while !complete(&raw) {
            let n = stream
                .read(&mut chunk)
                .await
                .map_err(|e| format!("読めない: {e}"))?;
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&chunk[..n]);
            if raw.len() > MAX_ANSWER {
                return Err("答えが大きすぎる".to_owned());
            }
        }
        Ok(raw)
    }
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

/// 頭と本文に分ける。
fn split(raw: &[u8]) -> Option<(&str, &[u8])> {
    let at = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&raw[..at]).ok()?;
    Some((head, &raw[at + 4..]))
}

/// 頭の欄の値（名の大文字・小文字は問わない）。
fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.split("\r\n").skip(1).find_map(|line| {
        let (n, v) = line.split_once(':')?;
        n.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// `Content-Length` の分を読み終えたか（無ければ、閉じるまで読む）。
fn complete(raw: &[u8]) -> bool {
    split(raw).is_some_and(|(head, body)| {
        header(head, "content-length")
            .and_then(|v| v.parse::<usize>().ok())
            .is_some_and(|length| body.len() >= length)
    })
}

/// `Transfer-Encoding: chunked` の本文をつなぐ。
fn dechunk(mut body: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let end = body.windows(2).position(|w| w == b"\r\n")?;
        let size = std::str::from_utf8(&body[..end]).ok()?;
        let size = usize::from_str_radix(size.split(';').next()?.trim(), 16).ok()?;
        body = &body[end + 2..];
        if size == 0 {
            return Some(out);
        }
        out.extend_from_slice(body.get(..size)?);
        body = body.get(size + 2..)?;
    }
}

#[derive(Deserialize)]
struct Resolved {
    as_of: String,
    results: Vec<Resolution>,
}

#[derive(Deserialize)]
struct Resolution {
    #[serde(rename = "ref")]
    actor_ref: String,
    actor: Option<ActorRecord>,
}

/// HTTP の答え（頭と本文）を読み、`actor_ref` の答えを取り出す。
fn parse_answer(raw: &[u8], actor_ref: &str) -> Result<Answer, String> {
    let unreadable = || "答えが読めない".to_owned();
    let (head, body) = split(raw).ok_or_else(unreadable)?;
    let status = head.split_whitespace().nth(1).unwrap_or("");
    if status != "200" {
        return Err(format!("castlist が {status} を返した"));
    }
    let body =
        if header(head, "transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
            dechunk(body).ok_or_else(unreadable)?
        } else {
            let length = header(head, "content-length").and_then(|v| v.parse::<usize>().ok());
            body[..length.unwrap_or(body.len()).min(body.len())].to_vec()
        };
    let resolved: Resolved = serde_json::from_slice(&body).map_err(|_| unreadable())?;
    let found = resolved
        .results
        .into_iter()
        .find(|r| r.actor_ref == actor_ref)
        .ok_or_else(|| "答えに、聞いた送り手が無い".to_owned())?;
    let actor_ok = found.actor.as_ref().is_none_or(|a| {
        is_actor_id(&a.id)
            && is_name(&a.kind)
            && (1..=128).contains(&a.name.chars().count())
            && !a.name.chars().any(char::is_control)
    });
    if !actor_ok || !is_timestamp(&resolved.as_of) {
        return Err(unreadable());
    }
    Ok(Answer {
        actor: found.actor,
        as_of: resolved.as_of,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const API: &str = "http://castlist.box:8080";

    #[test]
    fn only_http_host_and_port_is_taken() {
        for ok in [
            "http://127.0.0.1:8802",
            "http://127.0.0.1:8802/",
            "http://192.168.64.1:8802",
            "http://localhost:8802",
            "http://castlist.box:8080",
        ] {
            // castlist の URL も、画面の link の頭も、同じ決まりで受ける。
            let c = Castlist::configure(Some(ok), Some(ok))
                .unwrap_or_else(|e| panic!("{ok}: {e}"))
                .unwrap();
            assert_eq!(c.api(), ok.trim_end_matches('/'));
            assert_eq!(c.link(), ok.trim_end_matches('/'));
        }
        for ng in [
            "127.0.0.1:8802",
            "https://127.0.0.1:8802",
            "http://127.0.0.1",
            "http://127.0.0.1:",
            "http://127.0.0.1:x",
            "http://127.0.0.1:8802/v1",
            "http://a@127.0.0.1:8802",
            "http://127.0.0.1:8802?x",
            "http://127.0.0.1:99999",
            "http://:8802",
        ] {
            let api = Castlist::configure(Some(ng), None).map(|_| ());
            assert!(
                api.as_ref()
                    .is_err_and(|e| e.contains("PAGETRAY_CASTLIST_URL") && e.contains(ng)),
                "{ng}: {api:?}"
            );
            let link = Castlist::configure(Some(API), Some(ng)).map(|_| ());
            assert!(
                link.as_ref()
                    .is_err_and(|e| e.contains("PAGETRAY_CASTLIST_LINK_URL") && e.contains(ng)),
                "{ng}: {link:?}"
            );
        }
    }

    /// 聞く先（container の中から届く所）と、画面の link の頭（Product Manager の browser から開く所）は別に決める。
    #[test]
    fn the_link_base_is_separate_from_the_api() {
        let c = Castlist::configure(Some(API), Some("http://localhost:18802/"))
            .unwrap()
            .unwrap();
        assert_eq!(c.api(), API);
        assert_eq!(c.link(), "http://localhost:18802");
    }

    /// 画面の link の頭を書かなければ（空でも）、本番の castlist を host から開く `http://127.0.0.1:8802`。
    #[test]
    fn the_link_base_defaults_to_127_0_0_1_8802() {
        assert_eq!(DEFAULT_LINK, "http://127.0.0.1:8802");
        for link in [None, Some("")] {
            let c = Castlist::configure(Some(API), link).unwrap().unwrap();
            assert_eq!(c.api(), API);
            assert_eq!(c.link(), DEFAULT_LINK, "{link:?}");
        }
    }

    /// castlist の URL が無ければ（空でも）、castlist に聞かず、link も無い。
    #[test]
    fn without_the_api_url_there_is_no_castlist() {
        for api in [None, Some("")] {
            for link in [None, Some("http://localhost:18802")] {
                assert!(
                    Castlist::configure(api, link).unwrap().is_none(),
                    "{api:?} {link:?}"
                );
            }
        }
        // 形の違う link の頭は、castlist の URL が無くても、起きる前に断る（書き間違いを黙って捨てない）。
        assert!(Castlist::configure(None, Some("https://127.0.0.1:8802")).is_err());
    }

    const BODY: &str = r#"{"as_of":"2026-11-22T04:00:02.000Z","results":[{"ref":"claude.session:a","actor":{"id":"019a0000-0000-7000-8000-00000000000a","name":"t-23zs.swe.1","kind":"ai"}}]}"#;

    fn actor() -> Option<ActorRecord> {
        Some(ActorRecord {
            id: "019a0000-0000-7000-8000-00000000000a".into(),
            name: "t-23zs.swe.1".into(),
            kind: "ai".into(),
        })
    }

    #[test]
    fn an_answer_with_a_length_is_read() {
        let raw = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{BODY}",
            BODY.len()
        );
        assert_eq!(
            parse_answer(raw.as_bytes(), "claude.session:a"),
            Ok(Answer {
                actor: actor(),
                as_of: "2026-11-22T04:00:02.000Z".into()
            })
        );
    }

    #[test]
    fn a_chunked_answer_is_read() {
        let (a, b) = BODY.split_at(10);
        let raw = format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n",
            a.len(),
            b.len()
        );
        assert_eq!(
            parse_answer(raw.as_bytes(), "claude.session:a").map(|a| a.actor),
            Ok(actor())
        );
    }

    #[test]
    fn an_unknown_sender_is_an_answer_without_an_actor() {
        let body = r#"{"as_of":"2026-11-22T04:00:02.000Z","results":[{"ref":"claude.session:x","actor":null}]}"#;
        let raw = format!(
            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        );
        assert_eq!(
            parse_answer(raw.as_bytes(), "claude.session:x").map(|a| a.actor),
            Ok(None)
        );
    }

    #[test]
    fn errors_and_odd_answers_are_not_answers() {
        for raw in [
            "HTTP/1.1 503 Service Unavailable\r\ncontent-length: 2\r\n\r\n{}".to_owned(),
            "HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\n{}".to_owned(),
            "HTTP/1.1 200 OK\r\n\r\nnot json".to_owned(),
            "garbage".to_owned(),
            // 聞いていない送り手の答え。
            format!("HTTP/1.1 200 OK\r\n\r\n{}", BODY.replace(":a\"", ":b\"")),
            // 時刻の無い答え。
            format!(
                "HTTP/1.1 200 OK\r\n\r\n{}",
                BODY.replace("2026-11-22T04:00:02.000Z", "")
            ),
            // id に、URL を壊す字がある。
            format!(
                "HTTP/1.1 200 OK\r\n\r\n{}",
                BODY.replace("019a0000-", "../")
            ),
        ] {
            assert!(
                parse_answer(raw.as_bytes(), "claude.session:a").is_err(),
                "{raw}"
            );
        }
    }
}
