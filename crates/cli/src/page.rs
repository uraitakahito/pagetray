//! `pagetray page …`: ページを探す・読む・置く・消す・返事の状態を書く・新着を待つ。

use crate::client::{Client, Failure};
use crate::output;
use clap::{Args, Subcommand, ValueEnum};
use pagetray_api::{PageRecord, is_page_path, is_segment, parse_file, parse_page_path};
use serde::Deserialize;
use std::io::{BufRead, BufReader, IsTerminal, Read, Write};
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum PageCommand {
    /// ページを絞って探す（新しい順）
    List(ListArgs),
    /// ページの本文を出す
    View(ViewArgs),
    /// ページを置く（番号は置き場が決める。置き直すと次の番号になる）
    Put(PutArgs),
    /// ページを消す（戻せない）。まず消す物を一覧で出し、--yes を付けた時だけ消す
    Delete(DeleteArgs),
    /// ページの返事の状態（返事待ち・答えた）を書く
    Reply(ReplyArgs),
    /// ページが置かれるたびに 1 行出す（止めるまで待ち続ける）
    Watch(WatchArgs),
}

/// 返事の状態。API の `reply`・`state` の値と同じ名。
#[derive(Clone, Copy, ValueEnum)]
pub enum ReplyState {
    /// 返事待ち（求めて、まだ答えていない）
    Waiting,
    /// 答えた
    Answered,
}

impl ReplyState {
    fn name(self) -> &'static str {
        match self {
            ReplyState::Waiting => "waiting",
            ReplyState::Answered => "answered",
        }
    }
}

/// 待ちの相手。API の `on` の値と同じ名（0.8.0 から）。
#[derive(Clone, Copy, ValueEnum)]
pub enum Party {
    /// Product Manager（書かない時と同じ）
    Pm,
    /// レビューの席（レビューの依頼書）
    Review,
    /// TPM 担当（判定を受けずに上げたレビュー）
    Tpm,
}

impl Party {
    fn name(self) -> &'static str {
        match self {
            Party::Pm => "pm",
            Party::Review => "review",
            Party::Tpm => "tpm",
        }
    }
}

/// レビューの判定。API の `verdict` の値と同じ名（0.8.0 から）。
#[derive(Clone, Copy, ValueEnum)]
pub enum Verdict {
    /// よい
    Approve,
    /// 気になる所あり
    Concerns,
    /// 直すべき
    Reject,
    /// 見られなかった
    NotAssessed,
}

impl Verdict {
    fn name(self) -> &'static str {
        match self {
            Verdict::Approve => "approve",
            Verdict::Concerns => "concerns",
            Verdict::Reject => "reject",
            Verdict::NotAssessed => "not-assessed",
        }
    }
}

#[derive(Args)]
#[command(group(clap::ArgGroup::new("state").required(true).multiple(false).args(["waiting", "answered"])))]
pub struct ReplyArgs {
    /// ページの URL か、`<project>/<key>/<種類>-<番号>`
    #[arg(allow_hyphen_values = true)]
    page: String,
    /// 返事待ちにする
    #[arg(long)]
    waiting: bool,
    /// 答えたにする
    #[arg(long)]
    answered: bool,
    /// 待ちの相手（既定は pm）。答えたにする時は、この相手を待っているページだけが替わる（0.8.0 から）
    #[arg(long, value_enum, value_name = "相手")]
    on: Option<Party>,
    /// ページの記録を JSON で出す（既定は、状態と URL の 1 行）
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
pub struct PutArgs {
    /// 置く HTML の file（`-` は stdin）
    file: PathBuf,
    /// project の名（`.` は、今いる folder の path から作る）
    #[arg(
        short,
        long,
        env = "PAGETRAY_PROJECT",
        value_name = "名",
        allow_hyphen_values = true
    )]
    project: String,
    /// 課題の Key
    #[arg(short, long, value_name = "KEY")]
    key: String,
    /// 種類（brief・plan・result など。英小文字で始まり、英小文字・数字・- で 32 字まで）
    #[arg(long, value_name = "種類")]
    kind: String,
    /// 置く人の役割
    #[arg(
        long,
        env = "PAGETRAY_ROLE",
        default_value = "human",
        value_name = "役割"
    )]
    role: String,
    /// 置く人の居場所
    #[arg(long, default_value = "", value_name = "ID")]
    workspace: String,
    /// 置いたページの記録を JSON で出す（既定は URL だけ）
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
pub struct DeleteArgs {
    /// 消すページの URL か、`<project>/<key>/<種類>-<番号>`（`-` で始まる名は、URL で書くか `--` の後ろに書く）
    // いくつも受ける引数に `-` で始まる値を許すと、後ろの `--yes` まで値として読んでしまうので、許さない。
    #[arg(conflicts_with_all = ["project", "key", "kind"])]
    pages: Vec<String>,
    /// この project のページを消す（--key が無ければ、project のページをみな）。環境変数からは読まない
    #[arg(short, long, value_name = "名", allow_hyphen_values = true)]
    project: Option<String>,
    /// この Key のページだけ
    #[arg(short, long, value_name = "KEY", requires = "project")]
    key: Option<String>,
    /// この種類のページだけ
    #[arg(long, value_name = "種類", requires = "key")]
    kind: Option<String>,
    /// 一覧を見せるだけでなく、本当に消す
    #[arg(long)]
    yes: bool,
}

#[derive(Args)]
pub struct WatchArgs {
    /// 1 件を 1 行の JSON で出す
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
pub struct ListArgs {
    /// project の名（`.` は、今いる folder の path から作る）。無ければ、どの project も
    // tpm の project の名は `-` で始まる（folder の path から作るため）ので、`-` で始まる値も受ける。
    #[arg(
        short,
        long,
        env = "PAGETRAY_PROJECT",
        value_name = "名",
        allow_hyphen_values = true
    )]
    project: Option<String>,
    /// 課題の Key
    #[arg(short, long, value_name = "KEY")]
    key: Option<String>,
    /// 種類（brief・plan・result など）
    #[arg(long, value_name = "種類")]
    kind: Option<String>,
    /// 置いた担当の役割
    #[arg(long, value_name = "役割")]
    role: Option<String>,
    /// 置いた担当の居場所
    #[arg(long, value_name = "ID")]
    workspace: Option<String>,
    /// 題と本文の字を探す
    #[arg(short, long = "query", value_name = "語")]
    q: Option<String>,
    /// 返事の状態で絞る（waiting は返事待ち、answered は答えた物だけ）
    #[arg(long, value_enum, value_name = "状態")]
    reply: Option<ReplyState>,
    /// --reply で絞る時の待ちの相手（既定は pm。0.8.0 から）
    #[arg(long, value_enum, value_name = "相手", requires = "reply")]
    on: Option<Party>,
    /// レビューの判定で絞る（0.8.0 から）
    #[arg(long, value_enum, value_name = "判定")]
    verdict: Option<Verdict>,
    /// この時刻より前に置いた物（2026-01-02T03:04:05Z の形）
    #[arg(long, value_name = "時刻")]
    before: Option<String>,
    /// この時刻より後に置いた物
    #[arg(long, value_name = "時刻")]
    after: Option<String>,
    /// 古い順に出す
    #[arg(long)]
    oldest: bool,
    /// 出す件数（1〜1000）
    #[arg(short = 'L', long, default_value_t = 50, value_name = "件")]
    limit: u32,
    /// JSON で出す。欄を `key,url` のように名指すと、その欄だけ
    #[arg(long, value_name = "欄", num_args = 0..=1, default_missing_value = "", require_equals = true)]
    json: Option<String>,
}

#[derive(Args)]
pub struct ViewArgs {
    /// ページの URL か、`<project>/<key>/<種類>-<番号>`
    #[arg(allow_hyphen_values = true)]
    page: String,
    /// 本文を出さずに、browser で開く
    #[arg(short, long, conflicts_with = "output")]
    web: bool,
    /// 本文を file に書く
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,
}

#[derive(Deserialize)]
struct Listed {
    pages: Vec<PageRecord>,
}

/// `DELETE /api/pages` の答え。
#[derive(Deserialize)]
struct Removed {
    removed: Vec<String>,
}

/// stdout に出す字を返す。
pub fn run(client: &Client, command: PageCommand) -> Result<Vec<u8>, Failure> {
    match command {
        PageCommand::List(args) => list(client, args).map(String::into_bytes),
        PageCommand::View(args) => view(client, args),
        PageCommand::Put(args) => put(client, args).map(String::into_bytes),
        PageCommand::Delete(args) => delete(client, args).map(String::into_bytes),
        PageCommand::Reply(args) => reply(client, args).map(String::into_bytes),
        PageCommand::Watch(args) => watch(client, args).map(|()| Vec::new()),
    }
}

fn list(client: &Client, args: ListArgs) -> Result<String, Failure> {
    let project = args.project.as_deref().map(project_name).transpose()?;
    let limit = args.limit.to_string();
    let mut query: Vec<(&str, &str)> = vec![("limit", &limit)];
    let filters = [
        ("project", &project),
        ("key", &args.key),
        ("kind", &args.kind),
        ("role", &args.role),
        ("workspace", &args.workspace),
        ("q", &args.q),
        ("before", &args.before),
        ("after", &args.after),
    ];
    for (name, value) in filters {
        if let Some(value) = value {
            query.push((name, value));
        }
    }
    if args.oldest {
        query.push(("sort", "created_at"));
    }
    if let Some(state) = args.reply {
        query.push(("reply", state.name()));
    }
    if let Some(on) = args.on {
        query.push(("on", on.name()));
    }
    if let Some(verdict) = args.verdict {
        query.push(("verdict", verdict.name()));
    }
    let listed: Listed = client
        .get("/api/pages", &query)?
        .ok("置き場が一覧を返さなかった")?
        .json()?;
    let pages: Vec<PageRecord> = listed
        .pages
        .into_iter()
        .map(|page| PageRecord {
            url: Some(format!("{}{}", client.base, page.path)),
            ..page
        })
        .collect();
    if let Some(fields) = &args.json {
        return output::records(&pages, output::PAGE_FIELDS, fields);
    }
    let tty = std::io::stdout().is_terminal();
    if pages.is_empty() && tty {
        return Ok("合うページは無い\n".to_owned());
    }
    // project で絞った時は、どの行も同じになる project の欄を出さない。
    let with_project = project.is_none();
    let mut header = vec!["CREATED", "KEY", "PAGE", "ROLE", "REPLY", "URL", "TITLE"];
    if with_project {
        header.insert(1, "PROJECT");
    }
    let rows: Vec<Vec<String>> = pages
        .into_iter()
        .map(|p| {
            let reply = reply_of(&p);
            let mut row = vec![
                p.created_at,
                p.key,
                format!("{}-{}", p.kind, p.n),
                p.role,
                reply,
                p.url.unwrap_or_default(),
                p.title,
            ];
            if with_project {
                row.insert(1, p.project);
            }
            row
        })
        .collect();
    Ok(output::table(&header, &rows, tty))
}

/// 一覧に出す、返事の状態の字（返事を求めていないページは `-`）。Product Manager のほかを待つ物は
/// `waiting:review` のように相手を添える。
fn reply_of(page: &PageRecord) -> String {
    let state = match (&page.asked_at, &page.answered_at) {
        (_, Some(_)) => "answered",
        (Some(_), None) => "waiting",
        (None, None) => return "-".to_owned(),
    };
    match &page.waits_on {
        Some(on) => format!("{state}:{on}"),
        None => state.to_owned(),
    }
}

/// ページ 1 枚の返事の状態を書く（`POST /api/pages/reply`）。
fn reply(client: &Client, args: ReplyArgs) -> Result<String, Failure> {
    let path = page_path_of(&args.page)?;
    let Some((project, key, kind, n)) = parse_page_path(&path) else {
        return Err(Failure::usage(format!(
            "ページの URL か <project>/<key>/<種類>-<番号> を書く: {}",
            args.page
        )));
    };
    let state = if args.answered {
        ReplyState::Answered
    } else {
        ReplyState::Waiting
    };
    let n = n.to_string();
    let mut query = vec![
        ("project", project),
        ("key", key),
        ("kind", kind),
        ("n", n.as_str()),
        ("state", state.name()),
    ];
    if let Some(on) = args.on {
        query.push(("on", on.name()));
    }
    let sent = client.send("POST", "/api/pages/reply", &query, None)?;
    // 0.5.0 より前の置き場には、この道が無い。project が api のページは無いので、405 はその印。
    if sent.status == 405 {
        return Err(Failure::new(
            1,
            format!(
                "置き場（{}）の版が古く、返事の状態を書けない（0.5.0 から）",
                client.base
            ),
        ));
    }
    if sent.status == 409 {
        return Err(Failure::new(
            1,
            format!(
                "{}{path} は、その相手（--on）の返事を待っていない。何も替えていない",
                client.base
            ),
        ));
    }
    let page: PageRecord = sent
        .ok(&format!("返事の状態を書けない: {}{path}", client.base))?
        .json()?;
    let page = PageRecord {
        url: Some(format!("{}{}", client.base, page.path)),
        ..page
    };
    if args.json {
        Ok(output::pretty(
            &serde_json::to_value(&page).unwrap_or_default(),
        ))
    } else {
        Ok(format!(
            "{}\t{}\n",
            reply_of(&page),
            page.url.as_deref().unwrap_or_default()
        ))
    }
}

fn view(client: &Client, args: ViewArgs) -> Result<Vec<u8>, Failure> {
    let path = page_path_of(&args.page)?;
    if args.web {
        crate::open_in_browser(&format!("{}{path}", client.base))?;
        return Ok(Vec::new());
    }
    let body = client
        .get(&path, &[])?
        .ok(&format!("ページを読めない: {}{path}", client.base))?
        .bytes()?;
    match args.output {
        Some(file) => {
            std::fs::write(&file, &body).map_err(|e| {
                Failure::new(1, format!("file に書けない（{}）: {e}", file.display()))
            })?;
            Ok(Vec::new())
        }
        None => Ok(body),
    }
}

fn put(client: &Client, args: PutArgs) -> Result<String, Failure> {
    let project = project_name(&args.project)?;
    let body = if args.file.as_os_str() == "-" {
        let mut body = Vec::new();
        std::io::stdin()
            .read_to_end(&mut body)
            .map_err(|e| Failure::usage(format!("stdin を読めない: {e}")))?;
        body
    } else {
        std::fs::read(&args.file).map_err(|e| {
            Failure::usage(format!("file を読めない（{}）: {e}", args.file.display()))
        })?
    };
    let query = [
        ("project", project.as_str()),
        ("key", args.key.as_str()),
        ("kind", args.kind.as_str()),
        ("role", args.role.as_str()),
        ("workspace", args.workspace.as_str()),
    ];
    let placed: PageRecord = client
        .send(
            "POST",
            "/api/pages",
            &query,
            Some((&body, "text/html; charset=utf-8")),
        )?
        .ok("置き場がページを受け取らなかった")?
        .json()?;
    // 置き場の答えの url は、要求の Host で組んだ物。ここでは、頼んだ先（--url）で組む（一覧と同じ形）。
    let placed = PageRecord {
        url: Some(format!("{}{}", client.base, placed.path)),
        ..placed
    };
    if args.json {
        Ok(output::pretty(
            &serde_json::to_value(&placed).unwrap_or_default(),
        ))
    } else {
        Ok(format!("{}\n", placed.url.unwrap_or_default()))
    }
}

/// 消す物 1 つ: 置き場に頼む引数と、一覧のうち消す物に当たるかを見る path の頭（か、path そのもの）。
struct Target {
    query: Vec<(&'static str, String)>,
    exact: Option<String>,
}

fn delete(client: &Client, args: DeleteArgs) -> Result<String, Failure> {
    crate::refuse_delete()?;
    let mut targets = Vec::new();
    for page in &args.pages {
        let path = page_path_of(page)?;
        let parts: Vec<&str> = path.split('/').collect();
        let (kind, n) = parse_file(parts[3]).unwrap_or_default();
        targets.push(Target {
            query: vec![
                ("project", parts[1].to_owned()),
                ("key", parts[2].to_owned()),
                ("kind", kind.to_owned()),
                ("n", n.to_string()),
            ],
            exact: Some(path),
        });
    }
    if targets.is_empty() {
        let Some(project) = &args.project else {
            return Err(Failure::usage(
                "消すページの URL か、--project <名> [--key <KEY> [--kind <種類>]] を書く",
            ));
        };
        let mut query = vec![("project", project_name(project)?)];
        if let Some(key) = &args.key {
            query.push(("key", key.clone()));
        }
        if let Some(kind) = &args.kind {
            query.push(("kind", kind.clone()));
        }
        targets.push(Target { query, exact: None });
    }

    // まず、消す物を見る（n は一覧のエンドポイントの引数に無いので、path で選ぶ）。
    let mut listed: Vec<PageRecord> = Vec::new();
    for target in &targets {
        let mut query: Vec<(&str, &str)> = target
            .query
            .iter()
            .filter(|(name, _)| *name != "n")
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        query.push(("limit", "1000"));
        let found: Listed = client
            .get("/api/pages", &query)?
            .ok("置き場が一覧を返さなかった")?
            .json()?;
        listed.extend(
            found
                .pages
                .into_iter()
                .filter(|p| target.exact.as_ref().is_none_or(|path| *path == p.path)),
        );
    }
    if listed.is_empty() {
        return Ok("置き場に、消すページは無い\n".to_owned());
    }
    let lines: String = listed
        .iter()
        .map(|p| {
            format!(
                "{}{}\t{}\n",
                client.base,
                p.path,
                p.title.replace(['\t', '\n', '\r'], " ")
            )
        })
        .collect();
    if !args.yes {
        return Ok(format!(
            "消すページ（{} 枚。戻せない。まだ消していない）:\n{lines}消すには、同じ命令に --yes を付ける\n",
            listed.len()
        ));
    }
    let mut removed: Vec<String> = Vec::new();
    for target in &targets {
        let query: Vec<(&str, &str)> = target
            .query
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        let gone: Removed = client
            .send("DELETE", "/api/pages", &query, None)?
            .ok("置き場がページを消さなかった")?
            .json()?;
        removed.extend(gone.removed);
    }
    let lines: String = removed
        .iter()
        .map(|path| format!("{}{path}\n", client.base))
        .collect();
    Ok(format!("消した（{} 枚）:\n{lines}", removed.len()))
}

/// `GET /api/events`（SSE）を読み、`data:` の行（ページの記録）を 1 件ずつ出す。置き場が閉じるか、読む側が閉じるまで続く。
fn watch(client: &Client, args: WatchArgs) -> Result<(), Failure> {
    let reply = client
        .get("/api/events", &[])?
        .ok("置き場が新着を流さなかった")?;
    let mut stdout = std::io::stdout().lock();
    for line in BufReader::new(reply.reader()).lines() {
        let Ok(line) = line else { break };
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let Ok(page) = serde_json::from_str::<PageRecord>(data.trim()) else {
            continue;
        };
        let page = PageRecord {
            url: Some(format!("{}{}", client.base, page.path)),
            ..page
        };
        let out = if args.json {
            serde_json::to_string(&page).unwrap_or_default() + "\n"
        } else {
            let row = vec![
                page.created_at,
                page.project,
                page.key,
                format!("{}-{}", page.kind, page.n),
                page.role,
                page.url.unwrap_or_default(),
                page.title,
            ];
            output::table(&[], &[row], false)
        };
        // 読む側が閉じたら（`| head -1` など）、終わる。
        if stdout
            .write_all(out.as_bytes())
            .and_then(|()| stdout.flush())
            .is_err()
        {
            break;
        }
    }
    Ok(())
}

/// `--project` の値を、置き場の project の名にする。`.` は、今いる folder の path の、英数字と `._-` のほかを `-` にした名
/// （roster の tpm が project の名を作る形と同じ）。
pub fn project_name(value: &str) -> Result<String, Failure> {
    let name = if value == "." {
        let here = std::env::current_dir()
            .map_err(|e| Failure::usage(format!("今いる folder がわからない: {e}")))?;
        folder_name(&here.to_string_lossy())
    } else {
        value.to_owned()
    };
    if is_segment(&name) {
        Ok(name)
    } else {
        Err(Failure::usage(format!(
            "project の名は、英数字と . _ - で 255 字まで: {name}"
        )))
    }
}

fn folder_name(path: &str) -> String {
    path.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// ページの名指し（URL か `<project>/<key>/<種類>-<番号>`）を、置き場の中の path にする。
/// URL は、host と port を見ずに path だけを使う（頼む先は、いつも `--url` の置き場）。
pub fn page_path_of(page: &str) -> Result<String, Failure> {
    let path = match page.split_once("://") {
        Some((_, rest)) => {
            let rest = rest.split(['?', '#']).next().unwrap_or("");
            rest.find('/').map_or("", |at| &rest[at..]).to_owned()
        }
        None => {
            let bare = page.trim_start_matches('/');
            if bare.ends_with(".html") {
                format!("/{bare}")
            } else {
                format!("/{bare}.html")
            }
        }
    };
    if is_page_path(&path) {
        Ok(path)
    } else {
        Err(Failure::usage(format!(
            "ページの URL か <project>/<key>/<種類>-<番号> を書く: {page}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::{folder_name, page_path_of};

    #[test]
    fn pages_are_named_by_url_or_by_parts() {
        for page in [
            "http://127.0.0.1:8801/p/AI-22/plan-2.html",
            "http://127.0.0.1:8800/p/AI-22/plan-2.html?x=1#top",
            "p/AI-22/plan-2",
            "/p/AI-22/plan-2.html",
        ] {
            assert_eq!(
                page_path_of(page).unwrap(),
                "/p/AI-22/plan-2.html",
                "{page}"
            );
        }
        for ng in [
            "http://127.0.0.1:8801/",
            "http://127.0.0.1:8801/api/pages",
            "p/AI-22",
            "p/AI-22/plan",
            "../p/AI-22/plan-2",
            "",
        ] {
            assert!(page_path_of(ng).is_err(), "{ng}");
        }
    }

    #[test]
    fn folders_become_project_names_like_tpm() {
        assert_eq!(folder_name("/srv/me/projects/x y"), "-srv-me-projects-x-y");
        assert_eq!(folder_name("/srv/a.b_c-d"), "-srv-a.b_c-d");
    }
}
