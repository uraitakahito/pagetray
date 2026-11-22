//! JSON の API: ページと file を置く・探す・消す・DB の写し。

use crate::castlist::Answer;
use crate::db::{
    FileFilter, FileTarget, Filter, NewFile, NewPage, PageMeta, PutError, Reply, Target,
};
use crate::events::Note;
use crate::{AppState, FromUi, UI_ROLE, error, internal, text};
use axum::body::{Body, Bytes};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pagetray_api::{
    FILE_TYPES, SenderRecord, file_type, is_actor_ref, is_issuer, is_timestamp, is_workspace,
    sniff, split_file_name,
};
pub use pagetray_api::{is_name, is_segment, page_path};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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
    /// 付随する id を出した物と、付随する id（両方書くか、両方書かない）。
    actor_issuer: Option<String>,
    actor_ref: Option<String>,
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
    let actor = match actor_params(p.actor_issuer, p.actor_ref) {
        Ok(actor) => actor,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    // castlist には、書く取引を始める前に聞く（待つ間、書く接続を握らない）。
    let sender = match ask(&state, actor).await {
        Ok(sender) => sender,
        Err((status, message)) => return error(status, &message),
    };
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
            sender: sender.as_ref(),
        })
        .await;
    match placed {
        Ok(meta) => {
            let page = Page::from(meta);
            tracing::info!(
                "put {} role={} workspace={} size={}",
                page.path,
                page.meta.role,
                page.meta.workspace,
                page.meta.size
            );
            let url = url_of(&headers, &page.path);
            let mut value = serde_json::to_value(&page).unwrap_or_default();
            // 聞いている UI が無ければ送り先が無いだけなので、失敗は見ない。
            let _ = state.events.send(Note::Page(value.to_string()));
            value["url"] = url.clone().into();
            (StatusCode::CREATED, [(header::LOCATION, url)], Json(value)).into_response()
        }
        Err(PutError::Taken) => error(StatusCode::CONFLICT, "その番号は、もう使われている"),
        Err(PutError::Db(e)) => internal(e),
    }
}

const ACTOR_ISSUER_RULE: &str = "actor_issuer は英小文字・数字・- . で 64 字まで";
const ACTOR_REF_RULE: &str = "actor_ref は <種類>:<値>。種類は英小文字・数字・- . で 64 字まで、値は英数字と : . _ - で 256 字まで";

/// 付随する id の引数（`actor_issuer`・`actor_ref`）を見る。両方あれば、その組。両方無ければ無い。
fn actor_params(
    issuer: Option<String>,
    actor_ref: Option<String>,
) -> Result<Option<(String, String)>, &'static str> {
    match (issuer, actor_ref) {
        (None, None) => Ok(None),
        (Some(issuer), Some(actor_ref)) => {
            if !is_issuer(&issuer) {
                Err(ACTOR_ISSUER_RULE)
            } else if !is_actor_ref(&actor_ref) {
                Err(ACTOR_REF_RULE)
            } else {
                Ok(Some((issuer, actor_ref)))
            }
        }
        _ => Err("actor_issuer と actor_ref は、両方書くか、両方書かない"),
    }
}

/// 付随する id があれば、送り手の記録を作る。castlist の URL があれば castlist に聞き、その答えを入れる
/// （無ければ、付随する id だけ）。聞けない時は 503、castlist に登録の無い送り手は 422 と、その文を返す。
async fn ask(
    state: &AppState,
    actor: Option<(String, String)>,
) -> Result<Option<SenderRecord>, (StatusCode, String)> {
    let Some((issuer, actor_ref)) = actor else {
        return Ok(None);
    };
    let Some(castlist) = &state.castlist else {
        return Ok(Some(SenderRecord {
            issuer,
            actor_ref,
            actor: None,
            resolved_at: None,
        }));
    };
    match castlist.resolve(&issuer, &actor_ref).await {
        Ok(Answer {
            actor: Some(actor),
            as_of,
        }) => Ok(Some(SenderRecord {
            issuer,
            actor_ref,
            actor: Some(actor),
            resolved_at: Some(as_of),
        })),
        Ok(Answer { actor: None, .. }) => Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("castlist に登録の無い送り手（{actor_ref}）"),
        )),
        Err(why) => {
            tracing::warn!("castlist: {why} ref={actor_ref}");
            Err((
                StatusCode::SERVICE_UNAVAILABLE,
                format!("castlist に聞けない（{why}）。少し待ってから送り直す"),
            ))
        }
    }
}

/// 置いた側がそのまま開けるよう、要求の Host で URL を組む。
fn url_of(headers: &HeaderMap, path: &str) -> String {
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("127.0.0.1");
    format!("http://{host}{path}")
}

#[derive(Deserialize)]
pub struct ListParams {
    project: Option<String>,
    key: Option<String>,
    kind: Option<String>,
    role: Option<String>,
    workspace: Option<String>,
    q: Option<String>,
    /// 返事の状態（`waiting`・`answered`）。
    reply: Option<String>,
    /// 送り手（castlist のアクターの id）。
    actor: Option<String>,
    sort: Option<String>,
    limit: Option<i64>,
    before: Option<String>,
    after: Option<String>,
}

const REPLY_RULE: &str = "reply は waiting か answered";

pub async fn list(State(state): State<AppState>, Query(p): Query<ListParams>) -> Response {
    let oldest_first = match p.sort.as_deref() {
        None | Some("-created_at") => false,
        Some("created_at") => true,
        Some(_) => {
            return error(StatusCode::BAD_REQUEST, "sort は created_at か -created_at");
        }
    };
    let reply = match p.reply.as_deref().map(Reply::parse) {
        None => None,
        Some(Some(reply)) => Some(reply),
        Some(None) => return error(StatusCode::BAD_REQUEST, REPLY_RULE),
    };
    let filter = Filter {
        project: p.project,
        key: p.key,
        kind: p.kind,
        role: p.role,
        workspace: p.workspace,
        q: p.q,
        reply,
        actor: p.actor,
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
pub struct ReplyParams {
    project: String,
    key: String,
    kind: String,
    n: i64,
    state: String,
}

/// ページ 1 枚の返事の状態を書く（`POST /api/pages/reply?project=…&key=…&kind=…&n=…&state=waiting|answered`）。
/// 本文は読まない。答えは、そのページの記録。同じ状態を 2 度書いても同じ答え（時刻は初めの物を保つ）。
/// だれが書いてよいかは、呼ぶ側（tpm ask pm・tpm answer）が持つ。ここでは人を見分けない。
/// browser の中のページからは書けない（`Origin` の付いた要求は guard が断る）。
pub async fn reply(State(state): State<AppState>, Query(p): Query<ReplyParams>) -> Response {
    let Some(wanted) = Reply::parse(&p.state) else {
        return error(StatusCode::BAD_REQUEST, "state は waiting か answered");
    };
    let checks = [
        (
            is_segment(&p.project),
            "project は英数字と . _ - で 255 字まで",
        ),
        (is_segment(&p.key), "key は英数字と . _ - で 255 字まで"),
        (
            is_name(&p.kind),
            "kind は英小文字で始まり、英小文字・数字・- で 32 字まで",
        ),
        (p.n >= 1, "n は 1 以上"),
    ];
    if let Some((_, message)) = checks.iter().find(|(ok, _)| !ok) {
        return error(StatusCode::BAD_REQUEST, message);
    }
    match state
        .db
        .set_reply(&p.project, &p.key, &p.kind, p.n, wanted)
        .await
    {
        Ok(Some(meta)) => {
            let page = Page::from(meta);
            tracing::info!("reply {} state={}", page.path, wanted.name());
            let value = serde_json::to_value(&page).unwrap_or_default();
            // 聞いている UI が無ければ送り先が無いだけなので、失敗は見ない。
            let _ = state.events.send(Note::Reply(value.to_string()));
            Json(value).into_response()
        }
        Ok(None) => error(StatusCode::NOT_FOUND, "そのページは無い"),
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
                tracing::info!("remove {path}");
            }
            Json(serde_json::json!({ "removed": removed })).into_response()
        }
        Err(e) => internal(e),
    }
}

#[derive(Deserialize)]
pub struct PutFileParams {
    project: String,
    key: String,
    /// `<語>.<拡張子>`。番号はサーバーが付ける。
    name: String,
    /// 一覧の UI からの要求（[`FromUi`]）では読まない。
    #[serde(default)]
    role: String,
    #[serde(default)]
    workspace: String,
    n: Option<i64>,
    created_at: Option<String>,
    /// 付随する id を出した物と、付随する id（両方書くか、両方書かない）。一覧の UI からの要求では読まない。
    actor_issuer: Option<String>,
    actor_ref: Option<String>,
}

const FILE_NAME_RULE: &str =
    "name は <語>.<拡張子>。語は英小文字・数字・- _ で 64 字まで（英小文字か数字で始まる）";

/// file を置く（`POST /api/files?project=…&key=…&name=<語>.<拡張子>&role=…`）。本文は中身そのもの。
/// 一覧の UI から上げた物は、引数に何が書いてあっても、役割 `product-manager`・workspace 空で記録する
/// （browser から担当を名乗らせない）。番号と時刻も、置き場が決める。付随する id も読まない（送り手を記録しない）。
pub async fn put_file(
    State(state): State<AppState>,
    ui: Option<Extension<FromUi>>,
    Query(mut p): Query<PutFileParams>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if ui.is_some() {
        p.role = UI_ROLE.to_owned();
        p.workspace = String::new();
        p.n = None;
        p.created_at = None;
        p.actor_issuer = None;
        p.actor_ref = None;
    }
    let checks = [
        (
            is_segment(&p.project) && p.project != "api",
            "project は英数字と . _ - で 255 字まで",
        ),
        (is_segment(&p.key), "key は英数字と . _ - で 255 字まで"),
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
    let Some((name, ext)) = split_file_name(&p.name) else {
        return error(StatusCode::BAD_REQUEST, FILE_NAME_RULE);
    };
    let actor = match actor_params(p.actor_issuer.take(), p.actor_ref.take()) {
        Ok(actor) => actor,
        Err(message) => return error(StatusCode::BAD_REQUEST, message),
    };
    let Some(content_type) = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
    else {
        return error(StatusCode::BAD_REQUEST, "Content-Type が要る");
    };
    let Some((mime, kind)) = file_type(ext) else {
        let taken: Vec<&str> = FILE_TYPES.iter().map(|(ext, _, _)| *ext).collect();
        let message = format!("その拡張子は受けない（受けるのは {}）", taken.join("・"));
        return error(StatusCode::UNSUPPORTED_MEDIA_TYPE, &message);
    };
    if !same_media_type(content_type, mime) {
        return error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            &format!(".{ext} の Content-Type は {mime}"),
        );
    }
    if !sniff(kind, &body) {
        return error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            &format!("中身が .{ext} の形でない"),
        );
    }
    let sha256: String = Sha256::digest(&body)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    // castlist には、断る物を見終えてから、書く取引を始める前に聞く（待つ間、書く接続を握らない）。
    let sender = match ask(&state, actor).await {
        Ok(sender) => sender,
        Err((status, message)) => return error(status, &message),
    };
    let placed = state
        .db
        .put_file(NewFile {
            project: &p.project,
            key: &p.key,
            name,
            ext,
            r#type: mime,
            sha256: &sha256,
            role: &p.role,
            workspace: &p.workspace,
            body: &body,
            n: p.n,
            created_at: p.created_at.as_deref(),
            sender: sender.as_ref(),
        })
        .await;
    match placed {
        Ok(mut file) => {
            tracing::info!(
                "put {} role={} workspace={} size={}",
                file.path,
                file.role,
                file.workspace,
                file.size
            );
            // 聞いている UI が無ければ送り先が無いだけなので、失敗は見ない。
            let json = serde_json::to_string(&file).unwrap_or_default();
            let _ = state.events.send(Note::File(json));
            let url = url_of(&headers, &file.path);
            file.url = Some(url.clone());
            (StatusCode::CREATED, [(header::LOCATION, url)], Json(file)).into_response()
        }
        Err(PutError::Taken) => error(StatusCode::CONFLICT, "その番号は、もう使われている"),
        Err(PutError::Db(e)) => internal(e),
    }
}

/// Content-Type の種類の所（`;` の前）が同じか。大文字・小文字と、後ろの引数（charset など）は問わない。
fn same_media_type(given: &str, want: &str) -> bool {
    let media = |s: &str| s.split(';').next().unwrap_or("").trim().to_owned();
    media(given).eq_ignore_ascii_case(&media(want))
}

#[derive(Deserialize)]
pub struct ListFilesParams {
    project: Option<String>,
    key: Option<String>,
    role: Option<String>,
    workspace: Option<String>,
    q: Option<String>,
    /// 送り手（castlist のアクターの id）。
    actor: Option<String>,
    sort: Option<String>,
    limit: Option<i64>,
    before: Option<String>,
    after: Option<String>,
}

pub async fn list_files(
    State(state): State<AppState>,
    Query(p): Query<ListFilesParams>,
) -> Response {
    let oldest_first = match p.sort.as_deref() {
        None | Some("-created_at") => false,
        Some("created_at") => true,
        Some(_) => {
            return error(StatusCode::BAD_REQUEST, "sort は created_at か -created_at");
        }
    };
    let filter = FileFilter {
        project: p.project,
        key: p.key,
        role: p.role,
        workspace: p.workspace,
        q: p.q,
        actor: p.actor,
        before: p.before,
        after: p.after,
        oldest_first,
        limit: p.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT),
    };
    match state.db.list_files(&filter).await {
        Ok(files) => Json(serde_json::json!({ "files": files })).into_response(),
        Err(e) => internal(e),
    }
}

#[derive(Deserialize)]
pub struct RemoveFilesParams {
    project: String,
    key: Option<String>,
    /// `<語>.<拡張子>`。
    name: Option<String>,
    n: Option<i64>,
}

/// 合う file を消す（`DELETE /api/files?project=…[&key=…][&name=<語>.<拡張子>[&n=…]]`）。戻せない。
/// 消した番号は残り、同じ番号は 2 度使わない。消した file の path の一覧を返す（合う物が無ければ空）。
pub async fn remove_files(
    State(state): State<AppState>,
    Query(p): Query<RemoveFilesParams>,
) -> Response {
    let parts = p.name.as_deref().map(split_file_name);
    let checks = [
        (
            is_segment(&p.project),
            "project は英数字と . _ - で 255 字まで",
        ),
        (
            p.key.as_deref().is_none_or(is_segment),
            "key は英数字と . _ - で 255 字まで",
        ),
        (parts.is_none_or(|parts| parts.is_some()), FILE_NAME_RULE),
        (p.n.is_none_or(|n| n >= 1), "n は 1 以上"),
        (
            p.n.is_none() || (p.key.is_some() && p.name.is_some()),
            "n を書くときは key と name も書く",
        ),
    ];
    if let Some((_, message)) = checks.iter().find(|(ok, _)| !ok) {
        return error(StatusCode::BAD_REQUEST, message);
    }
    let (name, ext) = parts.flatten().unzip();
    let target = FileTarget {
        project: p.project,
        key: p.key,
        name: name.map(str::to_owned),
        ext: ext.map(str::to_owned),
        n: p.n,
    };
    match state.db.remove_files(&target).await {
        Ok(removed) => {
            for path in &removed {
                tracing::info!("remove {path}");
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

/// 残した記録（終わり方の印と、時刻つきの行）を、古い順の字で返す（`curl <URL>/api/logs`）。
/// 落ちた後に、道具を止めずに読むための道。多くて 4 MiB と 64 KiB。
pub async fn logs(State(state): State<AppState>) -> Response {
    let Some(runlog) = state.runlog else {
        return error(StatusCode::NOT_FOUND, "記録の置き場が無い");
    };
    match tokio::task::spawn_blocking(move || runlog.read_all()).await {
        Ok(text) => ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response(),
        Err(e) => internal(e),
    }
}
