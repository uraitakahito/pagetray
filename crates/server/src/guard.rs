//! 守りの層。生成した道と手の道（ページの本文）の前に、外から順に掛ける（[`crate::app_with`]。castlist の新しい server と同じ作り）:
//!
//! 1. [`stamp`]: どの答えにも型の推し量りを止める header などを付ける（断った答えにも付く）。
//! 2. 問い合わせごとの記録の 1 行（tower-http の TraceLayer。断った物も残る）。
//! 3. [`guard`]: `Host` と `Origin` で断る。`HEAD` は `GET` と同じに答え、本文を捨てる。
//! 4. [`timeout`]: 答えの頭までに時間がかかりすぎれば 503。
//!
//! 本文の上限（413 `PayloadTooLarge`）は、まだ無い（守りの層の PR で足す）。一覧の UI の form が file を上げる要求を
//! `Origin` の断りから外す例外（今の server の `from_ui`）は、file の PR で足す。
//!
//! 断る答えは、生成した層の誤りと同じ restJson1 の形（[`reply`]）。型は定義（ai.common）の `Forbidden`・`Unavailable` で、
//! どの操作の誤りにも入っている（service の errors）。
use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;

use crate::errors::reply;

#[allow(dead_code)]
fn _unused() -> Response {
    reply(StatusCode::OK, "", "")
}

/// 要求の `Host`（port も。守りの層が見て、受けた物）。置いたページの URL を組む。
#[derive(Clone, Debug)]
pub struct RequestHost(pub String);

/// host（pagetray を動かす機械）の中からの要求だけを受ける（今の server の guard と同じ）。
/// - `Host` が 127.0.0.1・localhost でなければ断る（外の site が名前を付け替えて届ける要求を防ぐ）。
/// - `GET`・`HEAD` のほかの要求に `Origin` が付いていれば断る（browser の中のページからは書かせない。roster と `curl` は付けない）。
///
/// `HEAD` は `GET` にして通し、答えの本文を捨てる（状態と header は `GET` と同じ）。生成した骨の道は `GET` だけなので、ここで揃える。
pub async fn guard(request: Request, next: Next) -> Response {
    // 試験を先に書く commit の仮の物（次の commit で作る）。
    next.run(request).await
}

/// `limit` の内に答えの頭を返せなければ、503 で返す（待っている物は捨てる）。
pub async fn timeout(State(_limit): State<Duration>, request: Request, next: Next) -> Response {
    // 試験を先に書く commit の仮の物（次の commit で作る）。
    next.run(request).await
}

/// 長さの字（秒で割り切れれば秒、ほかは ms）。
fn length(d: Duration) -> String {
    if d.subsec_nanos() == 0 {
        format!("{} 秒", d.as_secs())
    } else {
        format!("{} ms", d.as_millis())
    }
}

/// どの答えにも、型の推し量りを止める header と、referrer を送らせない header を付ける。
/// CSP は付けない（ページの本文は本文の道が `sandbox allow-scripts` を付ける。画面の CSP は画面の PR で決める）。
pub async fn stamp(request: Request, next: Next) -> Response {
    // 試験を先に書く commit の仮の物（次の commit で作る）。
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::length;
    use std::time::Duration;

    #[test]
    fn 長さの字() {
        assert_eq!(length(Duration::from_secs(5)), "5 秒");
        assert_eq!(length(Duration::from_millis(200)), "200 ms");
    }
}
