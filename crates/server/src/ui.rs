//! 画面（一覧の UI。`/`・`/{project}/`・`/{project}/{key}/`）。メッセージの一覧（`/messages`）は [`messages`]。
//!
//! - サーバーが HTML を組んで返す（askama。template は repo の templates/index.html。askama.toml）。
//!   字はみな HTML として逃がし、URL の引数に入れる値は %XX にする（[`encode`]）。
//! - 絞り込み・並べ替え・検索は link と form で動き、条件は URL に出る。引数の名と値は 0.8.x の server と同じ（snake_case の
//!   `sort=created_at` など。API の引数とは別）。違うのは続きの印 `at` で、前の頁の最後の行の時刻と id（API の `nextToken`
//!   と同じ形）。同じ時刻の行が頁の境に並んでも抜けない。読めない印は書かないのと同じ。
//! - 待ちの数と絞り込みの候補は、絞りに関わらず置き場の全部（[`crate::facets`]）。
//! - Key の画面（project と Key を 1 つずつ選んだ時）だけ、file の枠と上げる枠を出す（project が api なら出さない）。
//!   上げる枠の script は、API の道（`POST /api/projects/{p}/keys/{k}/files?name=…`）に、file の Content-Type で送る。
//! - 答えには CSP（[`CSP`]）を付け、inline の style と script は template の物（hash）だけを許す。
mod messages;

use std::sync::{Arc, LazyLock};

use askama::Template;
use axum::Router;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::castlist::Castlist;
use crate::db::{Db, FileRow, PageRow, Party, Reply};
use crate::errors::{Failure, reply};
use crate::facets::{Count, ReviewOf};
use crate::files::FILE_TYPES;
use crate::pages::{is_segment, page_path, parse_page_path};
use crate::search::{FileQuery, PageQuery};
use crate::token;

/// 1 度に出す件数。続きは「続きを出す」の link で出す。
const PAGE_SIZE: i64 = 200;

/// 続きの印の頭（API の `nextToken` と同じ形。ページの検索の印）。
const TOKEN_KIND: &str = "pages";

/// レビューの判定の字（定義の `Verdict` と同じ）と、画面に出す字。
const VERDICTS: [(&str, &str); 4] = [
    ("approve", "よい"),
    ("concerns", "気になる所あり"),
    ("reject", "直すべき"),
    ("not-assessed", "見られなかった"),
];

/// template（repo の templates/index.html）。inline の style と script の hash を、ここから取る。
const TEMPLATE: &str = include_str!("../../../templates/index.html");

/// 画面の答えに付ける Content-Security-Policy。inline の style と script は template の物（[`inline_hash`]）だけ。
/// 本文の枠（iframe）は置き場のページだけ、知らせ（EventSource）と上げる枠の送り先（fetch）は置き場だけ、
/// 図は置き場の file と、上げる前の見本（blob:）と favicon（data:）。form は置き場へだけ送れる。ほかの所から枠に入れさせない。
pub static CSP: LazyLock<HeaderValue> = LazyLock::new(|| csp(TEMPLATE));

/// `template` の画面の答えに付ける Content-Security-Policy（inline の style と script は、その template の物だけ）。
fn csp(template: &str) -> HeaderValue {
    let csp = format!(
        "default-src 'none'; script-src {}; style-src {}; img-src 'self' data: blob:; connect-src 'self'; frame-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
        inline_hash(template, "script"),
        inline_hash(template, "style"),
    );
    HeaderValue::from_str(&csp).expect("CSP は ASCII の字（hash は base64）")
}

/// template の中の 1 つの `<tag>`…`</tag>` の中身の SHA-256（CSP の `'sha256-…'`）。中身に template の式は無いので、
/// 組んだ画面の中身と同じ字（tests/ui.rs が、組んだ画面の中身と突き合わせる）。
fn inline_hash(template: &str, tag: &str) -> String {
    let body = template
        .split_once(&format!("<{tag}>"))
        .and_then(|(_, rest)| rest.split_once(&format!("</{tag}>")))
        .map_or("", |(body, _)| body);
    format!(
        "'sha256-{}'",
        aws_smithy_types::base64::encode(Sha256::digest(body.as_bytes()))
    )
}

/// 画面が使う物。
#[derive(Clone)]
struct Ui {
    db: Arc<Db>,
    /// 送り手の名の link の頭（`PAGETRAY_CASTLIST_LINK_URL`）。castlist が無ければ link は出さない。
    castlist: Option<Castlist>,
}

/// 画面の道。
pub fn routes(db: Arc<Db>, castlist: Option<Castlist>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/{project}/", get(project))
        .route("/{project}/{key}/", get(key))
        // メッセージの一覧。`/{project}/` は後ろに `/` が付くので、project の名が messages でもぶつからない。
        .route("/messages", get(messages::index))
        .with_state(Ui { db, castlist })
}

#[derive(Clone, Default, Deserialize)]
struct View {
    project: Option<String>,
    key: Option<String>,
    kind: Option<String>,
    role: Option<String>,
    workspace: Option<String>,
    q: Option<String>,
    /// 返事の状態（`waiting`・`answered`）。
    reply: Option<String>,
    /// 返事の状態の、待ちの相手（`review`・`tpm`。Product Manager は書かない）。
    on: Option<String>,
    /// レビューの判定。
    verdict: Option<String>,
    /// 送り手（castlist のアクターの id）。
    actor: Option<String>,
    sort: Option<String>,
    /// 並びの続きの印（前の頁の最後の行の時刻と id）。
    at: Option<String>,
    /// 本文の枠に出すページの path（URL の引数の名は `open`。前に配った URL を切らないように、変えない）。
    open: Option<String>,
}

impl View {
    fn oldest_first(&self) -> bool {
        self.sort.as_deref() == Some("created_at")
    }

    /// この条件の一覧の URL。空の条件は出さない。
    fn href(&self) -> String {
        let pairs = [
            ("project", &self.project),
            ("key", &self.key),
            ("kind", &self.kind),
            ("role", &self.role),
            ("workspace", &self.workspace),
            ("q", &self.q),
            ("reply", &self.reply),
            ("on", &self.on),
            ("verdict", &self.verdict),
            ("actor", &self.actor),
            ("sort", &self.sort),
            ("at", &self.at),
            ("open", &self.open),
        ];
        let query: Vec<String> = pairs
            .into_iter()
            .filter_map(|(name, value)| Some(format!("{name}={}", encode(value.as_deref()?))))
            .collect();
        if query.is_empty() {
            "/".to_owned()
        } else {
            format!("/?{}", query.join("&"))
        }
    }

    /// 条件を 1 つ変えた一覧の URL。並びの続きの境は、条件が変わると意味が無いので外す。
    fn with(&self, change: impl FnOnce(&mut View)) -> String {
        let mut next = self.clone();
        next.at = None;
        change(&mut next);
        next.href()
    }
}

struct Choice {
    label: String,
    note: String,
    count: i64,
    href: String,
    active: bool,
}

/// 絞り込みの列の、見出し 1 つ分（見出しと、その候補）。
struct FilterGroup {
    title: &'static str,
    choices: Vec<Choice>,
}

struct Row {
    path: String,
    href: String,
    active: bool,
    key: String,
    name: String,
    title: String,
    role: String,
    workspace: String,
    created_at: String,
    /// script が動かない時に出す時刻（UTC）。
    time: String,
    /// 返事の状態の名（`waiting`・`answered`）。返事を求めていないページは空。
    reply: &'static str,
    /// 待ちの相手（`review`・`tpm`）。Product Manager と、返事を求めていないページは空。
    on: &'static str,
    /// 返事の印の字（返事待ち・答えた・レビュー待ち・レビュー済み・上げた・決まった）。
    reply_label: &'static str,
    /// レビューの判定の名と字。判定の無いページは空。
    verdict: &'static str,
    verdict_label: &'static str,
    /// 見た物（レビューのページ）・見る物（ほかのページ）の見出しと link。
    targets_label: &'static str,
    targets: Vec<LinkView>,
    /// このページを見たレビュー（古い順）。
    reviews: Vec<ReviewView>,
    sender: Option<SenderView>,
}

/// 行の下に出す link 1 つ。置き場のページなら一覧の中の本文の枠に、ほかの URL なら別の tab で開く。
struct LinkView {
    href: String,
    label: String,
    external: bool,
}

/// 見られたページの行に出す、そのページを見たレビュー 1 つ。
struct ReviewView {
    link: LinkView,
    verdict: &'static str,
    verdict_label: &'static str,
}

/// 行に出す送り手: castlist の名（castlist のアクターの画面への link）と、その下に小さく付随する id。
struct SenderView {
    /// castlist の名。castlist の答えが無ければ「未確認」。
    name: String,
    /// castlist のアクターの画面（`<castlist の link の頭>/actors/<id>`）。castlist が無い・答えが無いなら無い。
    href: Option<String>,
    /// 付随する id。
    reference: String,
}

/// 送り手の列（付随する id・castlist のアクターの id と名）を、出す形にする。付随する id が無ければ無い。
/// アクターの id は castlist の答えを読む時に形（小文字の UUID）を見てあるので、link の path にそのまま入れる。
fn sender_view(
    castlist: Option<&Castlist>,
    actor_ref: Option<String>,
    actor_id: Option<String>,
    actor_name: Option<String>,
) -> Option<SenderView> {
    let reference = actor_ref?;
    let href = castlist
        .zip(actor_id.as_ref())
        .map(|(c, id)| format!("{}/actors/{id}", c.link()));
    Some(SenderView {
        name: actor_name.unwrap_or_else(|| "未確認".to_owned()),
        href,
        reference,
    })
}

/// 返事の状態の、画面に出す字（待ちの相手ごと）。
fn reply_label(reply: Reply, on: Party) -> &'static str {
    match (reply, on) {
        (Reply::Waiting, Party::Pm) => "返事待ち",
        (Reply::Answered, Party::Pm) => "答えた",
        (Reply::Waiting, Party::Review) => "レビュー待ち",
        (Reply::Answered, Party::Review) => "レビュー済み",
        (Reply::Waiting, Party::Tpm) => "上げた",
        (Reply::Answered, Party::Tpm) => "決まった",
    }
}

/// レビューの判定の名（DB の字を 'static にする）と、画面に出す字。知らない字なら空。
fn verdict_view(verdict: Option<&str>) -> (&'static str, &'static str) {
    VERDICTS
        .into_iter()
        .find(|(name, _)| Some(*name) == verdict)
        .unwrap_or(("", ""))
}

/// 見た物の link。置き場のページは、行と同じく一覧の中の本文の枠で開く（並びの続きの境も保つ）。
/// 字は、同じ Key なら `<kind>-<n>`、同じ project なら `<Key>/<kind>-<n>`。PR の URL（`…/<repo>/pull/<番号>`）は
/// `<repo>#<番号>`（別の repo の同じ番号の PR を見分けるため）。ほかの URL はそのまま。
fn link_view(view: &View, project: &str, key: &str, target: &str) -> LinkView {
    if let Some((p, k, kind, n)) = parse_page_path(target) {
        let mut next = view.clone();
        next.open = Some(target.to_owned());
        let label = match (p == project, k == key) {
            (true, true) => format!("{kind}-{n}"),
            (true, false) => format!("{k}/{kind}-{n}"),
            _ => format!("{p}/{k}/{kind}-{n}"),
        };
        return LinkView {
            href: next.href(),
            label,
            external: false,
        };
    }
    let pr = target.split_once("/pull/").and_then(|(head, rest)| {
        let repo = head.rsplit('/').next().filter(|r| !r.is_empty())?;
        let n = rest.split(['/', '?', '#']).next()?;
        (!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).then(|| format!("{repo}#{n}"))
    });
    LinkView {
        href: target.to_owned(),
        label: pr.unwrap_or_else(|| target.to_owned()),
        external: true,
    }
}

/// file の枠の、file 1 つ分。
struct FileItem {
    path: String,
    /// `<語>-<n>.<拡張子>`。
    name: String,
    /// 同じ Key のページから参照する時の、相対の path（`files/<語>-<n>.<拡張子>`）。
    reference: String,
    ext: String,
    image: bool,
    size: String,
    role: String,
    created_at: String,
    time: String,
    sender: Option<SenderView>,
}

/// Key の画面だけに出す物（file の枠と上げる枠）。
struct KeyView {
    project: String,
    key: String,
    files: Vec<FileItem>,
    /// 上げられる拡張子と Content-Type の組の並び（JSON。並びは置き場が断る時の文と同じ）。script が送る前に見る。
    types: String,
    /// `accept` に書く拡張子（`.png,.jpg,…`）。
    accept: String,
    max: usize,
    /// 上げる枠の script が送る道（`/api/projects/<project>/keys/<key>/files`。名は script が `name` の引数で足す）。
    upload: String,
}

/// 操作の帯の、待ちの数の釦 1 つ（押すと、その待ちだけに絞る）。数が 0 なら隠す。id は template に書く。
struct Counter {
    label: &'static str,
    title: &'static str,
    count: i64,
    href: String,
    active: bool,
}

#[derive(Template)]
#[template(path = "index.html")]
struct Index {
    q: String,
    hidden: Vec<(&'static str, String)>,
    sort_href: String,
    sort_label: &'static str,
    reload_href: String,
    reset_href: String,
    filtered: bool,
    /// Product Manager の返事待ちのページの数（絞り込みに関わらず、置き場の全部）。tab の題に出す。
    waiting: i64,
    /// 操作の帯の、待ちの数の釦（`#waiting` 返事待ち・`#review-waiting` レビュー待ち・`#escalated` 上げた）。
    /// 数は、絞り込みに関わらず置き場の全部。
    pm_counter: Counter,
    review_counter: Counter,
    tpm_counter: Counter,
    /// 絞り込みの列に出す物。
    filters: Vec<FilterGroup>,
    /// 一覧の列に出す行。
    rows: Vec<Row>,
    more_href: Option<String>,
    /// 本文の枠に出すページの path。
    viewer: Option<String>,
    /// project と Key を 1 つずつ選んだ時（Key の画面）だけ。
    key_view: Option<KeyView>,
}

/// 画面の引数。読めなければ（同じ名を 2 度書いた など）400。
type Params = Result<Query<View>, QueryRejection>;

async fn index(State(ui): State<Ui>, view: Params) -> Response {
    render(ui, view, |_| {}).await
}

/// `/<project>/`: 前の置き場の folder の一覧の代わり。
async fn project(State(ui): State<Ui>, Path(project): Path<String>, view: Params) -> Response {
    render(ui, view, |v| v.project = Some(project)).await
}

/// `/<project>/<key>/`
async fn key(
    State(ui): State<Ui>,
    Path((project, key)): Path<(String, String)>,
    view: Params,
) -> Response {
    render(ui, view, |v| {
        v.project = Some(project);
        v.key = Some(key);
    })
    .await
}

async fn render(ui: Ui, view: Params, place: impl FnOnce(&mut View)) -> Response {
    let Ok(Query(mut view)) = view else {
        return unreadable();
    };
    place(&mut view);
    screen(page(&ui, view).await, &CSP)
}

/// 画面の引数が読めない時の答え（400）。
fn unreadable() -> Response {
    reply(
        StatusCode::BAD_REQUEST,
        "ValidationException",
        "画面の引数が読めない（同じ名の引数を 2 度書いた など）",
    )
}

/// 組んだ画面の答え（その画面の CSP を付ける）。組めなければ、その誤りの答え。
fn screen(html: Result<String, Failure>, csp: &HeaderValue) -> Response {
    match html {
        Ok(html) => {
            let mut response = Html(html).into_response();
            response
                .headers_mut()
                .insert(header::CONTENT_SECURITY_POLICY, csp.clone());
            response
        }
        Err(failure) => failure.reply(),
    }
}

/// 条件を揃え、DB を読み、画面を組む。
async fn page(ui: &Ui, mut view: View) -> Result<String, Failure> {
    // 空の欄（検索の form が送る `q=` など）は、条件が無いのと同じに扱う。workspace だけは空も条件。
    for field in [
        &mut view.project,
        &mut view.key,
        &mut view.kind,
        &mut view.role,
        &mut view.q,
        &mut view.reply,
        &mut view.on,
        &mut view.verdict,
        &mut view.actor,
        &mut view.at,
    ] {
        *field = field.take().filter(|v| !v.trim().is_empty());
    }
    // 知らない返事の状態と判定は、条件が無いのと同じ。
    let reply = view.reply.as_deref().and_then(Reply::parse);
    view.reply = reply.map(|r| r.name().to_owned());
    // 待ちの相手は、返事の状態と組の時だけ。Product Manager は書かない（前からの URL と同じ形）。
    let on = view
        .on
        .as_deref()
        .and_then(Party::parse)
        .filter(|_| reply.is_some())
        .unwrap_or_default();
    view.on = (on != Party::Pm).then(|| on.name().to_owned());
    view.verdict = view
        .verdict
        .filter(|v| VERDICTS.iter().any(|(name, _)| name == v));
    view.sort = view.sort.filter(|s| s == "created_at");
    view.open = view.open.filter(|path| parse_page_path(path).is_some());
    let oldest_first = view.oldest_first();
    // 読めない続きの印と、ほかの並びの印は、書かないのと同じ（頭から出す）。
    let after = view
        .at
        .as_deref()
        .and_then(|at| token::after(at, TOKEN_KIND, "画面の続き", oldest_first).ok());
    if after.is_none() {
        view.at = None;
    }

    let query = PageQuery {
        project: view.project.clone(),
        key: view.key.clone(),
        kind: view.kind.clone(),
        role: view.role.clone(),
        workspace: view.workspace.clone(),
        q: view.q.clone(),
        reply: reply.map(|r| (r, on)),
        verdict: view.verdict.clone(),
        actor: view.actor.clone(),
        oldest: oldest_first,
        after,
        limit: PAGE_SIZE,
    };
    let (found, facets, files) =
        tokio::try_join!(ui.db.list(&query), ui.db.facets(), key_files(&ui.db, &view))
            .map_err(|e| Failure::db("ui", e))?;
    let paths: Vec<String> = found
        .rows
        .iter()
        .map(|r| page_path(&r.project, &r.key, &r.kind, r.n))
        .collect();
    let reviews = ui
        .db
        .reviews_of(&paths)
        .await
        .map_err(|e| Failure::db("ui", e))?;

    let more_href = found.more.then(|| found.rows.last()).flatten().map(|last| {
        let at = token::next(TOKEN_KIND, oldest_first, &last.created_at, last.id);
        view.with(|v| v.at = Some(at))
    });
    // 候補の並び。`show` が、候補の字と添える字を決める（値そのもの・判定の字・castlist の名）。
    let group = |title,
                 counts: Vec<Count>,
                 field: fn(&mut View) -> &mut Option<String>,
                 show: fn(&Count) -> (String, String)| FilterGroup {
        title,
        choices: counts
            .into_iter()
            .map(|c| {
                let mut probe = view.clone();
                let active = field(&mut probe).as_deref() == Some(c.value.as_str());
                let (label, note) = show(&c);
                let next = (!active).then_some(c.value);
                Choice {
                    label,
                    note,
                    count: c.count,
                    href: view.with(|v| *field(v) = next),
                    active,
                }
            })
            .collect(),
    };
    // 返事の候補は、値（waiting・answered と待ちの相手）でなく、画面の字で出す。件数が 0 の候補は出さない。
    let replies = FilterGroup {
        title: "返事",
        choices: [
            (Reply::Waiting, Party::Pm, facets.waiting),
            (Reply::Answered, Party::Pm, facets.answered),
            (Reply::Waiting, Party::Review, facets.review_waiting),
            (Reply::Waiting, Party::Tpm, facets.escalated),
        ]
        .into_iter()
        .filter(|(_, _, count)| *count > 0)
        .map(|(state, party, count)| {
            let active = reply == Some(state) && on == party;
            Choice {
                label: reply_label(state, party).to_owned(),
                note: String::new(),
                count,
                href: view.with(|v| {
                    v.reply = (!active).then(|| state.name().to_owned());
                    v.on = (!active && party != Party::Pm).then(|| party.name().to_owned());
                }),
                active,
            }
        })
        .collect(),
    };
    // 判定と送り手の候補は、値でなく画面の字（判定の字・castlist の名）で出す。
    let verdicts = group(
        "判定",
        facets.verdicts,
        |v| &mut v.verdict,
        |c| (verdict_view(Some(&c.value)).1.to_owned(), String::new()),
    );
    let senders = group(
        "送り手",
        facets.senders,
        |v| &mut v.actor,
        |c| (c.note.clone(), String::new()),
    );
    // 操作の帯の釦は、ほかの絞り込みを外して、その待ちだけに絞る（並びは保つ）。
    let counter = |label, title, party: Party, count| {
        let only = View {
            reply: Some(Reply::Waiting.name().to_owned()),
            on: (party != Party::Pm).then(|| party.name().to_owned()),
            sort: view.sort.clone(),
            ..View::default()
        };
        Counter {
            label,
            title,
            count,
            href: only.href(),
            active: reply == Some(Reply::Waiting) && on == party,
        }
    };
    let pm_counter = counter(
        "返事待ち",
        "Product Manager の返事を待っているページだけに絞る",
        Party::Pm,
        facets.waiting,
    );
    let review_counter = counter(
        "レビュー待ち",
        "レビューの判定を待っている依頼書だけに絞る",
        Party::Review,
        facets.review_waiting,
    );
    let tpm_counter = counter(
        "上げた",
        "判定を受けずに TPM 担当へ上げ、まだ決まっていないレビューだけに絞る",
        Party::Tpm,
        facets.escalated,
    );
    let key_view = files.map(|(project, key, files)| KeyView {
        upload: format!("/api/projects/{project}/keys/{key}/files"),
        files: files
            .into_iter()
            .map(|f| file_item(ui.castlist.as_ref(), f))
            .collect(),
        types: serde_json::to_string(
            &FILE_TYPES
                .iter()
                .map(|&(ext, mime, _)| (ext, mime))
                .collect::<Vec<_>>(),
        )
        .unwrap_or_default(),
        accept: FILE_TYPES
            .iter()
            .map(|(ext, _, _)| format!(".{ext}"))
            .collect::<Vec<_>>()
            .join(","),
        max: crate::MAX_BODY,
        project,
        key,
    });
    let index = Index {
        q: view.q.clone().unwrap_or_default(),
        hidden: [
            ("project", &view.project),
            ("key", &view.key),
            ("kind", &view.kind),
            ("role", &view.role),
            ("workspace", &view.workspace),
            ("reply", &view.reply),
            ("on", &view.on),
            ("verdict", &view.verdict),
            ("actor", &view.actor),
            ("sort", &view.sort),
        ]
        .into_iter()
        .filter_map(|(name, value)| Some((name, value.clone()?)))
        .collect(),
        sort_href: view.with(|v| v.sort = (!oldest_first).then(|| "created_at".to_owned())),
        sort_label: if oldest_first {
            "古い順"
        } else {
            "新しい順"
        },
        reload_href: view.with(|_| {}),
        reset_href: View {
            sort: view.sort.clone(),
            ..View::default()
        }
        .href(),
        filtered: [
            &view.project,
            &view.key,
            &view.kind,
            &view.role,
            &view.workspace,
            &view.q,
            &view.reply,
            &view.verdict,
            &view.actor,
        ]
        .iter()
        .any(|v| v.is_some()),
        waiting: facets.waiting,
        pm_counter,
        review_counter,
        tpm_counter,
        filters: vec![
            replies,
            verdicts,
            senders,
            group("担当者（役割）", facets.roles, |v| &mut v.role, plain),
            group("workspace", facets.workspaces, |v| &mut v.workspace, plain),
            group("種類", facets.kinds, |v| &mut v.kind, plain),
            group("Key", facets.keys, |v| &mut v.key, plain),
            group("project", facets.projects, |v| &mut v.project, plain),
        ],
        rows: found
            .rows
            .into_iter()
            .map(|r| row(&view, ui.castlist.as_ref(), &reviews, r))
            .collect(),
        more_href,
        viewer: view.open.clone(),
        key_view,
    };
    index.render().map_err(|e| Failure::data("ui", &e))
}

/// 値そのものを出す候補の字（空の値は「（不明）」）と、添える字。
fn plain(c: &Count) -> (String, String) {
    let label = if c.value.is_empty() {
        "（不明）".to_owned()
    } else {
        c.value.clone()
    };
    (label, c.note.clone())
}

fn row(view: &View, castlist: Option<&Castlist>, reviews: &[ReviewOf], r: PageRow) -> Row {
    let path = page_path(&r.project, &r.key, &r.kind, r.n);
    let mut next = view.clone();
    next.open = Some(path.clone());
    let state = match (&r.asked_at, &r.answered_at) {
        (_, Some(_)) => Some(Reply::Answered),
        (Some(_), None) => Some(Reply::Waiting),
        (None, None) => None,
    };
    let party = Party::of(r.waits_on.as_deref());
    let (verdict, verdict_label) = verdict_view(r.verdict.as_deref());
    let targets = r
        .targets
        .as_deref()
        .filter(|t| !t.is_empty())
        .map(|t| {
            t.split('\n')
                .map(|t| link_view(view, &r.project, &r.key, t))
                .collect()
        })
        .unwrap_or_default();
    let reviews = reviews
        .iter()
        .filter(|v| v.target == path)
        .map(|v| {
            let (verdict, verdict_label) = verdict_view(v.verdict.as_deref());
            ReviewView {
                link: link_view(
                    view,
                    &r.project,
                    &r.key,
                    &page_path(&v.project, &v.key, "review", v.n),
                ),
                verdict,
                verdict_label,
            }
        })
        .collect();
    Row {
        href: next.href(),
        active: view.open.as_deref() == Some(path.as_str()),
        targets_label: if r.kind == "review" {
            "見た物"
        } else {
            "見る物"
        },
        name: format!("{}-{}", r.kind, r.n),
        time: short_time(&r.created_at),
        reply: state.map_or("", Reply::name),
        on: if state.is_some() && party != Party::Pm {
            party.name()
        } else {
            ""
        },
        reply_label: state.map_or("", |s| reply_label(s, party)),
        verdict,
        verdict_label,
        targets,
        reviews,
        sender: sender_view(castlist, r.actor_ref, r.actor_id, r.actor_name),
        path,
        key: r.key,
        title: r.title,
        role: r.role,
        workspace: r.workspace,
        created_at: r.created_at,
    }
}

fn file_item(castlist: Option<&Castlist>, f: FileRow) -> FileItem {
    let name = format!("{}-{}.{}", f.name, f.n, f.ext);
    FileItem {
        path: crate::files::file_path(&f.project, &f.key, &f.name, &f.ext, f.n),
        reference: format!("files/{name}"),
        name,
        image: f.content_type.starts_with("image/"),
        size: size(f.size),
        time: short_time(&f.created_at),
        ext: f.ext,
        role: f.role,
        created_at: f.created_at,
        sender: sender_view(castlist, f.actor_ref, f.actor_id, f.actor_name),
    }
}

/// project と Key を 1 つずつ選んでいれば、その Key の file（新しい順。1 度に [`PAGE_SIZE`] まで）。
/// 形の違う project・Key と、project の api（file を置けない）には出さない。
async fn key_files(
    db: &Db,
    view: &View,
) -> Result<Option<(String, String, Vec<FileRow>)>, sqlx::Error> {
    let (Some(project), Some(key)) = (&view.project, &view.key) else {
        return Ok(None);
    };
    if !is_segment(project) || !is_segment(key) || project == "api" {
        return Ok(None);
    }
    let query = FileQuery {
        project: Some(project.clone()),
        key: Some(key.clone()),
        limit: PAGE_SIZE,
        ..FileQuery::default()
    };
    let found = db.list_files(&query).await?;
    Ok(Some((project.clone(), key.clone(), found.rows)))
}

/// 2026-11-21T04:45:22.472Z → 11-21 04:45（script が動かない時に出す、UTC の時刻）
fn short_time(at: &str) -> String {
    at.get(5..16).unwrap_or_default().replace('T', " ")
}

/// 人が読む大きさ（`512 B`・`47.1 KiB`・`3.2 MiB`）。
fn size(bytes: i64) -> String {
    const KIB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KIB {
        format!("{bytes} B")
    } else if b < KIB * KIB {
        format!("{:.1} KiB", b / KIB)
    } else {
        format!("{:.1} MiB", b / KIB / KIB)
    }
}

/// URL の query の値にする（英数字と - . _ ~ のほかは %XX。`?`・`&`・`#`・空白・`/` も）。
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
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
    use super::*;

    #[test]
    fn links_carry_the_conditions_and_drop_empty_ones() {
        let view = View {
            role: Some("tpm".into()),
            q: Some("絞り込み &=".into()),
            actor: Some("a b?c&d#e/f".into()),
            at: Some("7061676573".into()),
            ..View::default()
        };
        assert_eq!(
            view.href(),
            "/?role=tpm&q=%E7%B5%9E%E3%82%8A%E8%BE%BC%E3%81%BF%20%26%3D&actor=a%20b%3Fc%26d%23e%2Ff&at=7061676573"
        );
        assert_eq!(view.with(|v| (v.q, v.actor) = (None, None)), "/?role=tpm");
        assert_eq!(View::default().href(), "/");
    }

    #[test]
    fn sizes_read_as_bytes_kib_and_mib() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(48213), "47.1 KiB");
        assert_eq!(size(3 * 1024 * 1024 + 200 * 1024), "3.2 MiB");
    }

    #[test]
    fn the_csp_hashes_the_inline_style_and_script_of_the_template() {
        let csp = CSP.to_str().unwrap();
        assert!(csp.starts_with("default-src 'none'; script-src 'sha256-"));
        // 空の中身の hash（取り出しに落ちた時の物）ではない。
        let empty = format!(
            "'sha256-{}'",
            aws_smithy_types::base64::encode(Sha256::digest(b""))
        );
        assert!(!csp.contains(&empty), "{csp}");
    }
}
