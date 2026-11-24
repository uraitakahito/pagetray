//! 守りの層。生成した道と手の道（ページと file の本文）の前に、外から順に掛ける（[`crate::app_with`]。castlist の新しい server と同じ作り）:
//!
//! 1. [`stamp`]: どの答えにも型の推し量りを止める header などを付ける（断った答えにも付く）。
//! 2. 問い合わせごとの記録の 1 行（tower-http の TraceLayer。断った物も残る）。
//! 3. [`guard`]: `Host` と `Origin` で断る（一覧の UI の form が file を上げる要求だけは通し、[`FromUi`] の印を付ける）。
//!    `HEAD` は `GET` と同じに答え、本文を捨てる。
//! 4. [`timeout`]: 答えの頭までに時間がかかりすぎれば 503。
//! 5. [`limit`]: 本文が [`crate::MAX_BODY`]（16 MiB）を超えれば 413。
//!
//! 断る答えは、生成した層の誤りと同じ restJson1 の形（[`reply`]）。型は定義（ai.common）の `Forbidden`・`PayloadTooLarge`・
//! `Unavailable` で、どの操作の誤りにも入っている（service の errors）。
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;
use http_body_util::BodyExt;

use crate::errors::reply;

/// 要求の `Host`（port も。守りの層が見て、受けた物）。置いたページの URL を組む。
#[derive(Clone, Debug)]
pub struct RequestHost(pub String);

/// 一覧の UI の form から上げた file の印（[`guard`] が付ける）。付いた要求は、役割と workspace を置き場が決め、
/// 番号・時刻・付随する id を読まない（[`crate::files::create_file`]）。
#[derive(Clone, Copy, Debug)]
pub struct FromUi;

/// host（pagetray を動かす機械）の中からの要求だけを受ける（今の server の guard と同じ）。
/// - `Host` が 127.0.0.1・localhost でなければ断る（外の site が名前を付け替えて届ける要求を防ぐ）。
/// - `GET`・`HEAD` のほかの要求に `Origin` が付いていれば断る（browser の中のページからは書かせない。roster と `curl` は付けない）。
///   例外は 1 つだけ: 一覧の UI の form が file を上げる要求（[`from_ui`]）。印 [`FromUi`] を付けて通す。
///
/// `HEAD` は `GET` にして通し、答えの本文を捨てる（状態と header は `GET` と同じ）。生成した骨の道は `GET` だけなので、ここで揃える。
pub async fn guard(mut request: Request, next: Next) -> Response {
    let full = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let host = full
        .as_deref()
        .map(|v| v.rsplit_once(':').map_or(v, |(name, _port)| name));
    if !matches!(host, Some("127.0.0.1" | "localhost")) {
        return reply(
            StatusCode::FORBIDDEN,
            "Forbidden",
            "Host は 127.0.0.1 か localhost だけを受ける",
        );
    }
    if request.method() != Method::GET
        && request.method() != Method::HEAD
        && request.headers().contains_key(header::ORIGIN)
    {
        if !from_ui(&request) {
            return reply(
                StatusCode::FORBIDDEN,
                "Forbidden",
                "browser の中のページからは書けない",
            );
        }
        request.extensions_mut().insert(FromUi);
    }
    if let Some(full) = full {
        request.extensions_mut().insert(RequestHost(full));
    }
    let head = request.method() == Method::HEAD;
    if head {
        *request.method_mut() = Method::GET;
    }
    let response = next.run(request).await;
    if head {
        // 本文を捨てる前に、本文の大きさが決まっていれば Content-Length を付ける（GET と同じ header にする。
        // 付けずに捨てると、外の層が空の本文の 0 を付ける）。流す本文（大きさが決まらない物）は付けない。
        let (mut parts, body) = response.into_parts();
        if !parts.headers.contains_key(header::CONTENT_LENGTH)
            && let Some(len) = http_body::Body::size_hint(&body).exact()
        {
            parts
                .headers
                .insert(header::CONTENT_LENGTH, HeaderValue::from(len));
        }
        Response::from_parts(parts, Body::empty())
    } else {
        response
    }
}

/// 一覧の UI の form が file を上げる要求か（今の server の `from_ui` と同じ決まりを、CreateFile の道で）:
/// `POST /api/projects/{project}/keys/{key}/files` で、`Origin` が `http://<Host（port も）>` とそのまま同じで、
/// `Sec-Fetch-Site` が無いか `same-origin`。本文の枠の中のページ（`Origin: null`）や、ほかの site・ほかの道からは当たらない。
fn from_ui(request: &Request) -> bool {
    let headers = request.headers();
    let text = |name| {
        headers
            .get(name)
            .and_then(|v: &HeaderValue| v.to_str().ok())
    };
    let same_origin = match (text(header::ORIGIN), text(header::HOST)) {
        (Some(origin), Some(host)) => origin.strip_prefix("http://") == Some(host),
        _ => false,
    };
    request.method() == Method::POST
        && is_create_file(request.uri().path())
        && same_origin
        && matches!(
            text(HeaderName::from_static("sec-fetch-site")),
            None | Some("same-origin")
        )
}

/// CreateFile の道（`/api/projects/{project}/keys/{key}/files`。後ろに何も付かない物）か。
fn is_create_file(path: &str) -> bool {
    let Some(rest) = path.strip_prefix("/api/projects/") else {
        return false;
    };
    let parts: Vec<&str> = rest.split('/').collect();
    matches!(parts[..], [project, "keys", key, "files"] if !project.is_empty() && !key.is_empty())
}

/// 本文を [`crate::MAX_BODY`] まで読んでから渡す。超えれば 413 `PayloadTooLarge`（何も置かない）。
/// `Content-Length` で上限を超えると言う要求は、読まずに断る。大きさを言わずに流す要求は、上限まで読んで断る。
/// 生成した層は本文をみな読んでから引数にするので、ここで読んでおいても変わらない。
pub async fn limit(request: Request, next: Next) -> Response {
    let too_large = || {
        reply(
            StatusCode::PAYLOAD_TOO_LARGE,
            "PayloadTooLarge",
            "本文が大きすぎる（16 MiB まで）",
        )
    };
    let declared = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if declared.is_some_and(|len| len > crate::MAX_BODY as u64) {
        return too_large();
    }
    let (parts, body) = request.into_parts();
    let bytes = match http_body_util::Limited::new(body, crate::MAX_BODY)
        .collect()
        .await
    {
        Ok(collected) => collected.to_bytes(),
        Err(e) if e.is::<http_body_util::LengthLimitError>() => return too_large(),
        Err(e) => {
            tracing::warn!(error = %e, "本文が読めない");
            return reply(
                StatusCode::BAD_REQUEST,
                "SerializationException",
                "本文が読めない",
            );
        }
    };
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

/// `limit` の内に答えの頭を返せなければ、503 で返す（待っている物は捨てる）。
pub async fn timeout(State(limit): State<Duration>, request: Request, next: Next) -> Response {
    match tokio::time::timeout(limit, next.run(request)).await {
        Ok(response) => response,
        Err(_) => reply(
            StatusCode::SERVICE_UNAVAILABLE,
            "Unavailable",
            &format!(
                "時間がかかりすぎた（{}）。少し待ってから送り直す",
                length(limit)
            ),
        ),
    }
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
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::{is_create_file, length};
    use std::time::Duration;

    #[test]
    fn 長さの字() {
        assert_eq!(length(Duration::from_secs(5)), "5 秒");
        assert_eq!(length(Duration::from_millis(200)), "200 ms");
    }

    #[test]
    fn the_create_file_path_is_only_the_exact_shape() {
        assert!(is_create_file("/api/projects/p/keys/K-1/files"));
        for ng in [
            "/api/files",
            "/api/projects/p/keys/K-1/files/",
            "/api/projects/p/keys/K-1/files/x",
            "/api/projects/p/keys/K-1/kinds/files/pages",
            "/api/projects//keys/K-1/files",
            "/api/projects/p/keys//files",
            "/api/projects/p/key/K-1/files",
            "/x/api/projects/p/keys/K-1/files",
        ] {
            assert!(!is_create_file(ng), "{ng}");
        }
    }
}
