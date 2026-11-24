//! ページを置く操作（CreatePage）と、ページの本文を返す手の道（`GET /{project}/{key}/{kind}-{n}.html`。今の server と同じ URL）。
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use pagetray_sdk::error::CreatePageError;
use pagetray_sdk::input::CreatePageInput;
use pagetray_sdk::model::{ActorSummary, Sender};
use pagetray_sdk::output::CreatePageOutput;
use pagetray_sdk::server::request::extension::Extension;

use crate::castlist::Castlist;
use crate::db::{Db, NewPage, NewSender};
use crate::errors::{Failure, PutFailure, internal, reply};
use crate::guard::RequestHost;
use crate::text;

/// ページの本文の path。
pub fn page_path(project: &str, key: &str, kind: &str, n: i64) -> String {
    format!("/{project}/{key}/{kind}-{n}.html")
}

/// URL の 1 段になる字（project・key）。形（英数字と `.` `_` `-` で 1〜255 字）は定義が見るので、ここでは `.` と `..` だけを断る。
pub(crate) fn is_dot(s: &str) -> bool {
    s == "." || s == ".."
}

/// URL の 1 段になる字（project・key）の形（本文の道は生成した層を通らないので、ここで見る）。
pub(crate) fn is_segment(s: &str) -> bool {
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
pub(crate) fn parse_file(file: &str) -> Option<(&str, i64)> {
    let (kind, n) = file.strip_suffix(".html")?.rsplit_once('-')?;
    let ok = is_name(kind) && !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit());
    ok.then(|| n.parse().ok()).flatten().map(|n| (kind, n))
}

/// 見終えた置く引数。
struct Checked {
    n: Option<i64>,
    created_at: Option<String>,
    /// 送り手の付随する id（出した物と参照）。
    actor: Option<(String, String)>,
}

/// 置く引数の、定義の形では言えない決まりを見る。合わなければ、その訳。空の本文は、生成した層が先に断る（`/body` が無い）。
/// 付随する id の形（`Issuer`・`ActorRef`）は、定義が見てある。
fn check(input: &CreatePageInput) -> Result<Checked, String> {
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
    let actor = match (&input.actor_issuer, &input.actor_ref) {
        (None, None) => None,
        (Some(issuer), Some(actor_ref)) => Some((issuer.clone(), actor_ref.clone())),
        _ => {
            return Err("actorIssuer と actorRef（送り手）は、両方書くか、両方書かない".to_owned());
        }
    };
    Ok(Checked {
        n,
        created_at,
        actor,
    })
}

/// 付随する id があれば、足す送り手の行を作る。castlist があれば聞いて答えを入れ（無ければ付随する id だけ）、
/// castlist に登録の無い送り手は 400、聞けなければ 503。書く取引を始める前に呼ぶ（待つ間、書く接続を握らない）。
async fn ask(
    castlist: Option<&Castlist>,
    actor: Option<(String, String)>,
) -> Result<Option<NewSender>, PutFailure> {
    let Some((issuer, actor_ref)) = actor else {
        return Ok(None);
    };
    let answer = match castlist {
        None => None,
        Some(castlist) => match castlist.resolve(&issuer, &actor_ref).await {
            Ok(Some(answer)) => Some(answer),
            Ok(None) => {
                return Err(PutFailure::UnknownSender(format!(
                    "castlist に登録の無い送り手（{actor_ref}）"
                )));
            }
            Err(why) => {
                tracing::warn!("castlist: {why} ref={actor_ref}");
                return Err(PutFailure::Unreachable(format!(
                    "castlist に聞けない（{why}）。少し待ってから送り直す"
                )));
            }
        },
    };
    Ok(Some(NewSender {
        issuer,
        actor_ref,
        answer,
    }))
}

/// 答えの送り手の欄。
fn sender_output(sender: &NewSender) -> Result<Sender, String> {
    let (actor, resolved_at) = match &sender.answer {
        None => (None, None),
        Some(answer) => (
            Some(ActorSummary {
                id: answer.actor.id.clone(),
                name: answer.actor.name.clone(),
                kind: answer.actor.kind.clone(),
            }),
            Some(crate::datetime(&answer.as_of)?),
        ),
    };
    Ok(Sender {
        issuer: sender.issuer.clone(),
        r#ref: sender.actor_ref.clone(),
        actor,
        resolved_at,
    })
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

/// ページを 1 枚置く。
/// 検査の順: 引数（400）→ 付随する id（400）→ castlist（400・503）→ DB。
pub async fn create_page(
    db: Arc<Db>,
    castlist: Option<Castlist>,
    input: CreatePageInput,
    Extension(RequestHost(host)): Extension<RequestHost>,
) -> Result<CreatePageOutput, CreatePageError> {
    let Checked {
        n,
        created_at,
        actor,
    } = check(&input).map_err(PutFailure::Invalid)?;
    let sender = ask(castlist.as_ref(), actor).await?;
    let workspace = input.workspace.as_deref().unwrap_or("");
    let body = input.body.as_ref();
    let html = String::from_utf8_lossy(body);
    let (title, plain) = (text::title(&html), text::plain(&html));
    let placed = db
        .put_page(&NewPage {
            project: &input.project,
            key: &input.key,
            kind: &input.kind,
            role: &input.role,
            workspace,
            title: &title,
            text: &plain,
            body,
            n,
            created_at: created_at.as_deref(),
            sender: sender.as_ref(),
        })
        .await
        .map_err(|e| PutFailure::write("CreatePage", e, "その番号は、もう使われている"))?;
    let path = page_path(&input.project, &input.key, &input.kind, placed.n);
    tracing::info!(
        "put {path} role={} workspace={workspace} size={}",
        input.role,
        placed.size
    );
    let created_at = crate::datetime(&placed.created_at)
        .map_err(|e| PutFailure::Internal(internal("CreatePage", &e)))?;
    let sender = sender
        .as_ref()
        .map(sender_output)
        .transpose()
        .map_err(|e| PutFailure::Internal(internal("CreatePage", &e)))?;
    let url = format!("http://{host}{path}");
    Ok(CreatePageOutput {
        project: input.project,
        key: input.key,
        kind: input.kind,
        n: placed.n.to_string(),
        title,
        role: input.role,
        workspace: workspace.to_owned(),
        created_at,
        size: placed.size,
        path,
        sender,
        location: url.clone(),
        url,
    })
}

/// ページの本文を、置いたままの byte で返す。
pub async fn show(
    State(db): State<Arc<Db>>,
    Path((project, key, file)): Path<(String, String, String)>,
) -> Response {
    let Some((kind, n)) = parse_file(&file).filter(|_| is_segment(&project) && is_segment(&key))
    else {
        return not_found();
    };
    match db.body(&project, &key, kind, n).await {
        Ok(Some(body)) => (
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                // 隔てた枠として出す。ページの中の script は動くが、置き場の名で API を呼べない。
                (header::CONTENT_SECURITY_POLICY, "sandbox allow-scripts"),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            ],
            body,
        )
            .into_response(),
        Ok(None) => match db.removed_at(&project, &key, kind, n).await {
            Ok(Some(at)) => reply(
                StatusCode::GONE,
                "Gone",
                &format!("そのページは消した（{at}）"),
            ),
            Ok(None) => not_found(),
            Err(e) => Failure::db("page", e).reply(),
        },
        Err(e) => Failure::db("page", e).reply(),
    }
}

fn not_found() -> Response {
    reply(StatusCode::NOT_FOUND, "NotFound", "そのページは無い")
}
