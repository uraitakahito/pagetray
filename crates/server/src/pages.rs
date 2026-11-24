//! ページを置く操作（CreatePage）と、ページの本文を返す手の道（`GET /{project}/{key}/{kind}-{n}.html`。今の server と同じ URL）。
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use pagetray_sdk::error::CreatePageError;
use pagetray_sdk::input::CreatePageInput;
use pagetray_sdk::output::CreatePageOutput;
use pagetray_sdk::server::request::extension::Extension;

use crate::db::Db;
use crate::errors::{Failure, PutFailure, internal, reply};
use crate::guard::RequestHost;

/// ページの本文の path。
pub fn page_path(project: &str, key: &str, kind: &str, n: i64) -> String {
    format!("/{project}/{key}/{kind}-{n}.html")
}

/// URL の 1 段になる字（project・key）。形（英数字と `.` `_` `-` で 1〜255 字）は定義が見るので、ここでは `.` と `..` だけを断る。
fn is_dot(s: &str) -> bool {
    s == "." || s == ".."
}

/// URL の 1 段になる字（project・key）の形（本文の道は生成した層を通らないので、ここで見る）。
fn is_segment(s: &str) -> bool {
    (1..=255).contains(&s.len())
        && !is_dot(s)
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// 種類と役割の名の形（本文の道で見る。定義の `Name` と同じ）。
fn is_name(s: &str) -> bool {
    (1..=32).contains(&s.len())
        && s.as_bytes()[0].is_ascii_lowercase()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// `<kind>-<n>.html` を分ける。番号は、頭に 0 の無い 1 以上の数だけ（1 枚に URL は 1 つ）。
fn parse_file(file: &str) -> Option<(&str, i64)> {
    let (kind, n) = file.strip_suffix(".html")?.rsplit_once('-')?;
    let ok = is_name(kind) && !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit());
    ok.then(|| n.parse().ok()).flatten().map(|n| (kind, n))
}

/// 置く引数の、定義の形では言えない決まりを見る。合わなければ、その訳。空の本文は、生成した層が先に断る（`/body` が無い）。
fn check(input: &CreatePageInput) -> Result<(Option<i64>, Option<String>), String> {
    if is_dot(&input.project) || input.project == "api" {
        return Err("project に .・..・api は使えない".to_owned());
    }
    if is_dot(&input.key) {
        return Err("key に . と .. は使えない".to_owned());
    }
    let n = match &input.n {
        None => None,
        // 形（頭に 0 の無い 18 桁まで）は定義が見てある。
        Some(n) => Some(n.parse::<i64>().map_err(|_| "n は 1 以上の数".to_owned())?),
    };
    let created_at = match &input.created_at {
        None => None,
        Some(t) => Some(created_at(t)?),
    };
    if input.actor_issuer.is_some() || input.actor_ref.is_some() {
        return Err("actorIssuer と actorRef（送り手）は、まだ受けない".to_owned());
    }
    Ok((n, created_at))
}

/// 取り込みの時刻を、残す字（UTC・ミリ秒まで・24 字）にする。ミリ秒より細かい時刻と、0001〜9999 年の外は断る
/// （0000 年は生成した層が断る。答えの時刻に書けないため）。
fn created_at(t: &aws_smithy_types::DateTime) -> Result<String, String> {
    if !t.subsec_nanos().is_multiple_of(1_000_000) {
        return Err("createdAt はミリ秒まで".to_owned());
    }
    time::OffsetDateTime::from_unix_timestamp_nanos(t.as_nanos())
        .ok()
        .filter(|t| (1..=9999).contains(&t.year()))
        .map(crate::db::stamp)
        .ok_or_else(|| "createdAt は 0001 年から 9999 年までの時刻".to_owned())
}

/// ページを 1 枚置く（試験を先に書く commit の仮の物。次の commit で作る）。
pub async fn create_page(
    _db: Arc<Db>,
    _input: CreatePageInput,
    _host: Extension<RequestHost>,
) -> Result<CreatePageOutput, CreatePageError> {
    Err(PutFailure::Internal(internal("CreatePage", &"まだ作っていない")).into())
}

/// ページの本文を、置いたままの byte で返す（試験を先に書く commit の仮の物。次の commit で作る）。
pub async fn show(
    State(_db): State<Arc<Db>>,
    Path((_project, _key, _file)): Path<(String, String, String)>,
) -> Response {
    Failure::Internal(internal("page", &"まだ作っていない")).reply()
}

fn not_found() -> Response {
    reply(StatusCode::NOT_FOUND, "NotFound", "そのページは無い")
}
