//! JSON の口: 置く・探す・消す・DB の写し。

use crate::db::{Filter, NewPage, PageMeta, PutError, Target};
use crate::{AppState, error, internal, text};
use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
pub use pagetray_api::{is_name, is_segment, page_path};
use pagetray_api::{is_timestamp, is_workspace};
use serde::{Deserialize, Serialize};
use tokio_util::io::ReaderStream;

const DEFAULT_LIMIT: i64 = 100;
const MAX_LIMIT: i64 = 1000;

#[derive(Deserialize)]
pub struct PutParams {
    project: String,
    key: String,
    kind: String,
    role: String,
    #[serde(default)]
    workspace: String,
    n: Option<i64>,
    created_at: Option<String>,
}

#[derive(Serialize)]
pub struct Page {
    #[serde(flatten)]
    meta: PageMeta,
    /// このサーバーの中の path。`<置き場の URL><path>` で開ける。
    path: String,
}

impl From<PageMeta> for Page {
    fn from(meta: PageMeta) -> Self {
        let path = page_path(&meta.project, &meta.key, &meta.kind, meta.n);
        Self { meta, path }
    }
}

pub async fn put(
    State(state): State<AppState>,
    Query(p): Query<PutParams>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let checks = [
        (
            is_segment(&p.project) && p.project != "api",
            "project は英数字と . _ - で 255 字まで",
        ),
        (is_segment(&p.key), "key は英数字と . _ - で 255 字まで"),
        (
            is_name(&p.kind),
            "kind は英小文字で始まり、英小文字・数字・- で 32 字まで",
        ),
        (
            is_name(&p.role),
            "role は英小文字で始まり、英小文字・数字・- で 32 字まで",
        ),
        (
            is_workspace(&p.workspace),
            "workspace は英数字と : . _ - で 64 字まで",
        ),
        (p.n.is_none_or(|n| n >= 1), "n は 1 以上"),
        (
            p.created_at.as_deref().is_none_or(is_timestamp),
            "created_at は 2026-01-02T03:04:05.678Z の形（UTC）",
        ),
        (!body.is_empty(), "本文が空"),
    ];
    if let Some((_, message)) = checks.iter().find(|(ok, _)| !ok) {
        return error(StatusCode::BAD_REQUEST, message);
    }
    let html = String::from_utf8_lossy(&body);
    let (title, plain) = (text::title(&html), text::plain(&html));
    let placed = state
        .db
        .put(NewPage {
            project: &p.project,
            key: &p.key,
            kind: &p.kind,
            role: &p.role,
            workspace: &p.workspace,
            title: &title,
            text: &plain,
            body: &body,
            n: p.n,
            created_at: p.created_at.as_deref(),
        })
        .await;
    match placed {
        Ok(meta) => {
            let page = Page::from(meta);
            println!(
                "put {} role={} workspace={} size={}",
                page.path, page.meta.role, page.meta.workspace, page.meta.size
            );
            // 置いた側がそのまま開けるよう、要求の Host で URL を組む。
            let host = headers
                .get(header::HOST)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("127.0.0.1");
            let url = format!("http://{host}{}", page.path);
            let mut value = serde_json::to_value(&page).unwrap_or_default();
            // 聞いている UI が無ければ送り先が無いだけなので、失敗は見ない。
            let _ = state.events.send(value.to_string());
            value["url"] = url.clone().into();
            (StatusCode::CREATED, [(header::LOCATION, url)], Json(value)).into_response()
        }
        Err(PutError::Taken) => error(StatusCode::CONFLICT, "その番号は、もう使われている"),
        Err(PutError::Db(e)) => internal(e),
    }
}

#[derive(Deserialize)]
pub struct ListParams {
    project: Option<String>,
    key: Option<String>,
    kind: Option<String>,
    role: Option<String>,
    workspace: Option<String>,
    q: Option<String>,
    sort: Option<String>,
    limit: Option<i64>,
    before: Option<String>,
    after: Option<String>,
}

pub async fn list(State(state): State<AppState>, Query(p): Query<ListParams>) -> Response {
    let oldest_first = match p.sort.as_deref() {
        None | Some("-created_at") => false,
        Some("created_at") => true,
        Some(_) => {
            return error(StatusCode::BAD_REQUEST, "sort は created_at か -created_at");
        }
    };
    let filter = Filter {
        project: p.project,
        key: p.key,
        kind: p.kind,
        role: p.role,
        workspace: p.workspace,
        q: p.q,
        before: p.before,
        after: p.after,
        oldest_first,
        limit: p.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT),
    };
    match state.db.list(&filter).await {
        Ok(rows) => {
            let pages: Vec<Page> = rows.into_iter().map(Page::from).collect();
            Json(serde_json::json!({ "pages": pages })).into_response()
        }
        Err(e) => internal(e),
    }
}

#[derive(Deserialize)]
pub struct RemoveParams {
    project: String,
    key: Option<String>,
    kind: Option<String>,
    n: Option<i64>,
}

/// 合うページを消す（`DELETE /api/pages?project=…[&key=…][&kind=…[&n=…]]`）。戻せない。
/// 消した番号は残り、同じ番号は 2 度使わない。消したページの path の一覧を返す（合う物が無ければ空）。
/// 消してよいのは TPM 担当だけという決まりは、cli（tpm page delete）と guard が持つ。ここでは人を見分けない。
pub async fn remove(State(state): State<AppState>, Query(p): Query<RemoveParams>) -> Response {
    let checks = [
        (
            is_segment(&p.project),
            "project は英数字と . _ - で 255 字まで",
        ),
        (
            p.key.as_deref().is_none_or(is_segment),
            "key は英数字と . _ - で 255 字まで",
        ),
        (
            p.kind.as_deref().is_none_or(is_name),
            "kind は英小文字で始まり、英小文字・数字・- で 32 字まで",
        ),
        (p.n.is_none_or(|n| n >= 1), "n は 1 以上"),
        (
            p.n.is_none() || (p.key.is_some() && p.kind.is_some()),
            "n を書くときは key と kind も書く",
        ),
    ];
    if let Some((_, message)) = checks.iter().find(|(ok, _)| !ok) {
        return error(StatusCode::BAD_REQUEST, message);
    }
    let target = Target {
        project: p.project,
        key: p.key,
        kind: p.kind,
        n: p.n,
    };
    match state.db.remove(&target).await {
        Ok(removed) => {
            for path in &removed {
                println!("remove {path}");
            }
            Json(serde_json::json!({ "removed": removed })).into_response()
        }
        Err(e) => internal(e),
    }
}

/// DB の写しを 1 つの file で返す（`curl -o pages.db <URL>/api/backup`）。
pub async fn backup(State(state): State<AppState>) -> Response {
    let path = match state.db.backup().await {
        Ok(path) => path,
        Err(e) => return internal(e),
    };
    let file = tokio::fs::File::open(&path).await;
    // 開いた後は名が要らない。消しても、読み終えるまで中身は残る。
    let _ = tokio::fs::remove_file(&path).await;
    match file {
        Ok(file) => (
            [
                (header::CONTENT_TYPE, "application/vnd.sqlite3"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"pages.db\"",
                ),
            ],
            Body::from_stream(ReaderStream::new(file)),
        )
            .into_response(),
        Err(e) => internal(e),
    }
}
