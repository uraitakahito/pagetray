//! castlist（送り手の本当の id と名を持つ、この Mac の中のサーバー）に、送り手を聞く。
//! まだ形だけ（試験を先に書く）。

use pagetray_api::ActorRecord;

/// castlist の居場所（`PAGETRAY_CASTLIST_URL`）。
#[derive(Clone, Debug)]
pub struct Castlist {
    base: String,
}

/// castlist の答え。`actor` が無ければ、castlist に登録の無い送り手。
#[derive(Debug, PartialEq, Eq)]
pub struct Answer {
    pub actor: Option<ActorRecord>,
    /// castlist が答えた時刻。
    pub as_of: String,
}

impl Castlist {
    /// `http://<host>:<port>`（後ろの `/` は無くてよい）。
    pub fn parse(url: &str) -> Result<Self, String> {
        Ok(Self {
            base: url.to_owned(),
        })
    }

    /// 画面から castlist のアクターを開く URL の頭（`http://<host>:<port>`）。
    pub fn base(&self) -> &str {
        &self.base
    }

    /// 付随する id の送り手を聞く。聞けなければ、その訳。
    pub async fn resolve(&self, _issuer: &str, _actor_ref: &str) -> Result<Answer, String> {
        Err("まだ聞けない".to_owned())
    }
}

/// HTTP の答え（頭と本文）を読み、`actor_ref` の答えを取り出す。
fn parse_answer(_raw: &[u8], _actor_ref: &str) -> Result<Answer, String> {
    Err("まだ読めない".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_http_host_and_port_is_taken() {
        for ok in [
            "http://127.0.0.1:8802",
            "http://127.0.0.1:8802/",
            "http://192.168.64.1:8802",
            "http://localhost:8802",
        ] {
            let c = Castlist::parse(ok).unwrap_or_else(|e| panic!("{ok}: {e}"));
            assert_eq!(c.base(), ok.trim_end_matches('/'));
        }
        for ng in [
            "",
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
            assert!(Castlist::parse(ng).is_err(), "{ng}");
        }
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
