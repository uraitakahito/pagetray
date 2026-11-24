//! 一覧の UI。サーバーが HTML を組んで返す。絞り込み・並べ替え・検索は link と form で動き、
//! 条件は URL に出る。script は、新着の印・未読の印・返事の印の差し替え・時刻の表示と、Key の画面で file を
//! 上げることに使う。

use crate::api::{is_segment, page_path};
use crate::castlist::Castlist;
use crate::db::{Count, FileFilter, Filter, PageMeta, Reply, ReviewOf, WaitsOn};
use crate::{AppState, MAX_FILE, internal, pages};
use askama::Template;
use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Response};
use pagetray_api::{FILE_TYPES, SenderRecord, VERDICTS, parse_page_path};
use serde::Deserialize;

/// 1 度に出す件数。続きは「続きを出す」の link で出す。
const PAGE_SIZE: i64 = 200;

#[derive(Clone, Default, Deserialize)]
pub struct View {
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
    /// 並びの続きを出す時の境の時刻。
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
    /// castlist の名。castlist に聞いていなければ「未確認」。
    name: String,
    /// castlist のアクターの画面（`<castlist の link の頭>/actors/<id>`）。castlist の URL が無い・未確認なら無い。
    href: Option<String>,
    /// 付随する id。
    reference: String,
}

fn sender_view(castlist: Option<&Castlist>, sender: Option<SenderRecord>) -> Option<SenderView> {
    let sender = sender?;
    let href = castlist
        .zip(sender.actor.as_ref())
        .map(|(c, a)| format!("{}/actors/{}", c.link(), a.id));
    Some(SenderView {
        name: sender.actor.map_or_else(|| "未確認".to_owned(), |a| a.name),
        href,
        reference: sender.actor_ref,
    })
}

/// 返事の状態の、画面に出す字（待ちの相手ごと）。
fn reply_label(reply: Reply, on: WaitsOn) -> &'static str {
    match (reply, on) {
        (Reply::Waiting, WaitsOn::Pm) => "返事待ち",
        (Reply::Answered, WaitsOn::Pm) => "答えた",
        (Reply::Waiting, WaitsOn::Review) => "レビュー待ち",
        (Reply::Answered, WaitsOn::Review) => "レビュー済み",
        (Reply::Waiting, WaitsOn::Tpm) => "上げた",
        (Reply::Answered, WaitsOn::Tpm) => "決まった",
    }
}

/// レビューの判定の名（DB の字を 'static にする）と、画面に出す字。知らない字なら空。
fn verdict_view(verdict: Option<&str>) -> (&'static str, &'static str) {
    let name = VERDICTS
        .into_iter()
        .find(|v| Some(*v) == verdict)
        .unwrap_or("");
    let label = match name {
        "approve" => "よい",
        "concerns" => "気になる所あり",
        "reject" => "直すべき",
        "not-assessed" => "見られなかった",
        _ => "",
    };
    (name, label)
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
struct FileRow {
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
    files: Vec<FileRow>,
    /// 上げられる拡張子と Content-Type の組の並び（JSON。並びは置き場が断る時の文と同じ）。script が送る前に見る。
    types: String,
    /// `accept` に書く拡張子（`.png,.jpg,…`）。
    accept: String,
    max: usize,
    /// 上げる枠の script が送る道（名は script が `name` の引数で足す。template は新しい server と共に使う）。
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

pub async fn index(State(state): State<AppState>, Query(view): Query<View>) -> Response {
    render(state, view).await
}

/// `/<project>/`: 前の置き場の folder の一覧の代わり。
pub async fn project(
    State(state): State<AppState>,
    Path(project): Path<String>,
    Query(view): Query<View>,
) -> Response {
    let project = Some(project);
    render(state, View { project, ..view }).await
}

/// `/<project>/<key>/`
pub async fn key(
    State(state): State<AppState>,
    Path((project, key)): Path<(String, String)>,
    Query(view): Query<View>,
) -> Response {
    let (project, key) = (Some(project), Some(key));
    render(
        state,
        View {
            project,
            key,
            ..view
        },
    )
    .await
}

async fn render(state: AppState, mut view: View) -> Response {
    // 空の欄（検索の form が送る `q=` など）は、条件が無いのと同じに扱う。
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
    ] {
        *field = field.take().filter(|v| !v.trim().is_empty());
    }
    let reply = view.reply.as_deref().and_then(Reply::parse);
    view.reply = reply.map(|r| r.name().to_owned());
    // 待ちの相手は、返事の状態と組の時だけ。Product Manager は書かない（前からの URL と同じ形）。
    let on = view
        .on
        .as_deref()
        .and_then(WaitsOn::parse)
        .filter(|_| reply.is_some())
        .unwrap_or_default();
    view.on = (on != WaitsOn::Pm).then(|| on.name().to_owned());
    view.verdict = view.verdict.filter(|v| VERDICTS.contains(&v.as_str()));
    view.sort = view.sort.filter(|s| s == "created_at");
    view.open = view.open.filter(|path| pages::is_page_path(path));

    let oldest_first = view.oldest_first();
    let (before, after) = match (view.at.clone(), oldest_first) {
        (at, false) => (at, None),
        (at, true) => (None, at),
    };
    let filter = Filter {
        project: view.project.clone(),
        key: view.key.clone(),
        kind: view.kind.clone(),
        role: view.role.clone(),
        workspace: view.workspace.clone(),
        q: view.q.clone(),
        reply,
        on,
        verdict: view.verdict.clone(),
        actor: view.actor.clone(),
        before,
        after,
        oldest_first,
        limit: PAGE_SIZE,
    };
    let (pages, facets, key_view) = match tokio::try_join!(
        state.db.list(&filter),
        state.db.facets(),
        key_view(&state, &view)
    ) {
        Ok(found) => found,
        Err(e) => return internal(e),
    };
    let paths: Vec<String> = pages
        .iter()
        .map(|m| page_path(&m.project, &m.key, &m.kind, m.n))
        .collect();
    let reviews = match state.db.reviews_of(&paths).await {
        Ok(found) => found,
        Err(e) => return internal(e),
    };

    let more_href = (pages.len() as i64 == PAGE_SIZE)
        .then(|| pages.last().map(|last| last.created_at.clone()))
        .flatten()
        .map(|at| view.with(|v| v.at = Some(at)));
    let group =
        |title, counts: Vec<Count>, field: fn(&mut View) -> &mut Option<String>| FilterGroup {
            title,
            choices: counts
                .into_iter()
                .map(|c| {
                    let mut probe = view.clone();
                    let active = field(&mut probe).as_deref() == Some(c.value.as_str());
                    let next = (!active).then(|| c.value.clone());
                    Choice {
                        label: if c.value.is_empty() {
                            "（不明）".to_owned()
                        } else {
                            c.value
                        },
                        note: c.note,
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
            (Reply::Waiting, WaitsOn::Pm, facets.waiting),
            (Reply::Answered, WaitsOn::Pm, facets.answered),
            (Reply::Waiting, WaitsOn::Review, facets.review_waiting),
            (Reply::Waiting, WaitsOn::Tpm, facets.escalated),
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
                    v.on = (!active && party != WaitsOn::Pm).then(|| party.name().to_owned());
                }),
                active,
            }
        })
        .collect(),
    };
    // 判定の候補も、画面の字で出す。
    let verdicts = FilterGroup {
        title: "判定",
        choices: facets
            .verdicts
            .into_iter()
            .map(|c| {
                let active = view.verdict.as_deref() == Some(c.value.as_str());
                let next = (!active).then(|| c.value.clone());
                Choice {
                    label: verdict_view(Some(&c.value)).1.to_owned(),
                    note: String::new(),
                    count: c.count,
                    href: view.with(|v| v.verdict = next),
                    active,
                }
            })
            .collect(),
    };
    // 送り手の候補は、値（castlist のアクターの id）でなく、castlist の名で出す。
    let senders = FilterGroup {
        title: "送り手",
        choices: facets
            .senders
            .into_iter()
            .map(|c| {
                let active = view.actor.as_deref() == Some(c.value.as_str());
                let next = (!active).then(|| c.value.clone());
                Choice {
                    label: c.note,
                    note: String::new(),
                    count: c.count,
                    href: view.with(|v| v.actor = next),
                    active,
                }
            })
            .collect(),
    };
    // 操作の帯の釦は、ほかの絞り込みを外して、その待ちだけに絞る（並びは保つ）。
    let counter = |label, title, party: WaitsOn, count| {
        let only = View {
            reply: Some(Reply::Waiting.name().to_owned()),
            on: (party != WaitsOn::Pm).then(|| party.name().to_owned()),
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
        WaitsOn::Pm,
        facets.waiting,
    );
    let review_counter = counter(
        "レビュー待ち",
        "レビューの判定を待っている依頼書だけに絞る",
        WaitsOn::Review,
        facets.review_waiting,
    );
    let tpm_counter = counter(
        "上げた",
        "判定を受けずに TPM 担当へ上げ、まだ決まっていないレビューだけに絞る",
        WaitsOn::Tpm,
        facets.escalated,
    );
    let page = Index {
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
            group("担当者（役割）", facets.roles, |v| &mut v.role),
            group("workspace", facets.workspaces, |v| &mut v.workspace),
            group("種類", facets.kinds, |v| &mut v.kind),
            group("Key", facets.keys, |v| &mut v.key),
            group("project", facets.projects, |v| &mut v.project),
        ],
        rows: pages
            .into_iter()
            .map(|meta| row(&view, state.castlist.as_ref(), &reviews, meta))
            .collect(),
        more_href,
        viewer: view.open.clone(),
        key_view,
    };
    match page.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => internal(e),
    }
}

fn row(view: &View, castlist: Option<&Castlist>, reviews: &[ReviewOf], meta: PageMeta) -> Row {
    let path = page_path(&meta.project, &meta.key, &meta.kind, meta.n);
    let mut next = view.clone();
    next.open = Some(path.clone());
    let state = match (&meta.asked_at, &meta.answered_at) {
        (_, Some(_)) => Some(Reply::Answered),
        (Some(_), None) => Some(Reply::Waiting),
        (None, None) => None,
    };
    let party = WaitsOn::of(meta.waits_on.as_deref());
    let (verdict, verdict_label) = verdict_view(meta.verdict.as_deref());
    let targets = meta
        .targets
        .iter()
        .map(|t| link_view(view, &meta.project, &meta.key, t))
        .collect();
    let reviews = reviews
        .iter()
        .filter(|r| r.target == path)
        .map(|r| {
            let (verdict, verdict_label) = verdict_view(r.verdict.as_deref());
            ReviewView {
                link: link_view(
                    view,
                    &meta.project,
                    &meta.key,
                    &page_path(&r.project, &r.key, "review", r.n),
                ),
                verdict,
                verdict_label,
            }
        })
        .collect();
    Row {
        href: next.href(),
        active: view.open.as_deref() == Some(path.as_str()),
        targets_label: if meta.kind == "review" {
            "見た物"
        } else {
            "見る物"
        },
        path,
        key: meta.key,
        name: format!("{}-{}", meta.kind, meta.n),
        title: meta.title,
        role: meta.role,
        workspace: meta.workspace,
        time: short_time(&meta.created_at),
        created_at: meta.created_at,
        reply: state.map_or("", Reply::name),
        on: if state.is_some() && party != WaitsOn::Pm {
            party.name()
        } else {
            ""
        },
        reply_label: state.map_or("", |s| reply_label(s, party)),
        verdict,
        verdict_label,
        targets,
        reviews,
        sender: sender_view(castlist, meta.sender),
    }
}

/// project と Key を 1 つずつ選んでいれば、その Key の file（新しい順）と、上げる枠に要る物。
async fn key_view(state: &AppState, view: &View) -> Result<Option<KeyView>, sqlx::Error> {
    let (Some(project), Some(key)) = (&view.project, &view.key) else {
        return Ok(None);
    };
    if !is_segment(project) || !is_segment(key) || project == "api" {
        return Ok(None);
    }
    let filter = FileFilter {
        project: Some(project.clone()),
        key: Some(key.clone()),
        role: None,
        workspace: None,
        q: None,
        actor: None,
        before: None,
        after: None,
        oldest_first: false,
        limit: PAGE_SIZE,
    };
    let files = state.db.list_files(&filter).await?;
    let types: Vec<(&str, &str)> = FILE_TYPES
        .iter()
        .map(|&(ext, mime, _)| (ext, mime))
        .collect();
    let accept: Vec<String> = FILE_TYPES
        .iter()
        .map(|(ext, _, _)| format!(".{ext}"))
        .collect();
    Ok(Some(KeyView {
        project: project.clone(),
        key: key.clone(),
        files: files
            .into_iter()
            .map(|f| {
                let name = format!("{}-{}.{}", f.name, f.n, f.ext);
                FileRow {
                    reference: format!("files/{name}"),
                    name,
                    image: f.r#type.starts_with("image/"),
                    size: size(f.size),
                    time: short_time(&f.created_at),
                    path: f.path,
                    ext: f.ext,
                    role: f.role,
                    created_at: f.created_at,
                    sender: sender_view(state.castlist.as_ref(), f.sender),
                }
            })
            .collect(),
        types: serde_json::to_string(&types).unwrap_or_default(),
        accept: accept.join(","),
        max: MAX_FILE,
        upload: format!("/api/files?project={}&key={}", encode(project), encode(key)),
    }))
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

/// URL の query の値にする（英数字と - . _ ~ のほかは %XX）。
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
            at: Some("2026-11-21T00:00:00Z".into()),
            ..View::default()
        };
        assert_eq!(
            view.href(),
            "/?role=tpm&q=%E7%B5%9E%E3%82%8A%E8%BE%BC%E3%81%BF%20%26%3D&at=2026-11-21T00%3A00%3A00Z"
        );
        assert_eq!(view.with(|v| v.q = None), "/?role=tpm");
        assert_eq!(View::default().href(), "/");
    }

    #[test]
    fn sizes_read_as_bytes_kib_and_mib() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(48213), "47.1 KiB");
        assert_eq!(size(3 * 1024 * 1024 + 200 * 1024), "3.2 MiB");
    }
}
