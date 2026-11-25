//! `pagetray page …`: ページを探す・読む・置く・消す・返事の状態を書く・新着を待つ。

use crate::client::{Failure, Route, Store, TIMEOUT};
use crate::output::{self, page_record, page_state};
use aws_smithy_types::Blob;
use clap::{Args, Subcommand, ValueEnum};
use pagetray_api::{is_page_path, is_segment, parse_page_path};
use serde_json::{Map, Value};
use std::io::{IsTerminal, Read, Write};
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

/// 待ちの相手。API の `on` の値と同じ名。
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

/// レビューの判定。API の `verdict` の値と同じ名。
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
    /// 待ちの相手（既定は pm）。答えたにする時は、この相手を待っているページだけが替わる
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
    // roster の project の名は `-` で始まる（folder の path から作るため）ので、`-` で始まる値も受ける。
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
    /// --reply で絞る時の待ちの相手（既定は pm）
    #[arg(long, value_enum, value_name = "相手", requires = "reply")]
    on: Option<Party>,
    /// レビューの判定で絞る
    #[arg(long, value_enum, value_name = "判定")]
    verdict: Option<Verdict>,
    /// 古い順に出す
    #[arg(long)]
    oldest: bool,
    /// 出す件数（1〜1000）。続きがあれば、続きの印を stderr に出す
    #[arg(short = 'L', long, default_value_t = 50, value_name = "件", value_parser = clap::value_parser!(i32).range(1..=1000))]
    limit: i32,
    /// 前の答えの続き（前に出た続きの印をそのまま書く。同じ並びの物だけ）
    #[arg(long, value_name = "印")]
    next_token: Option<String>,
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

/// stdout に出す字を返す。
pub async fn run(store: &Store, command: PageCommand) -> Result<Vec<u8>, Failure> {
    match command {
        PageCommand::List(args) => list(store, args).await.map(String::into_bytes),
        PageCommand::View(args) => view(store, args).await,
        PageCommand::Put(args) => put(store, args).await.map(String::into_bytes),
        PageCommand::Delete(args) => delete(store, args).await.map(String::into_bytes),
        PageCommand::Reply(args) => reply(store, args).await.map(String::into_bytes),
        PageCommand::Watch(args) => watch(store, args).await.map(|()| Vec::new()),
    }
}

/// 探す時の条件（`ListPages` の引数）。
#[derive(Default)]
struct Filter {
    project: Option<String>,
    key: Option<String>,
    kind: Option<String>,
    role: Option<String>,
    workspace: Option<String>,
    q: Option<String>,
    reply: Option<&'static str>,
    on: Option<&'static str>,
    verdict: Option<&'static str>,
    oldest: bool,
}

/// 1 頁を読み、記録と続きの印を返す。
async fn page_of(
    store: &Store,
    filter: &Filter,
    max: i32,
    next_token: Option<String>,
) -> Result<(Vec<Map<String, Value>>, Option<String>), Failure> {
    let listed = store
        .api
        .list_pages()
        .set_project(filter.project.clone())
        .set_key(filter.key.clone())
        .set_kind(filter.kind.clone())
        .set_role(filter.role.clone())
        .set_workspace(filter.workspace.clone())
        .set_q(filter.q.clone())
        .set_reply(filter.reply.map(str::to_owned))
        .set_on(filter.on.map(str::to_owned))
        .set_verdict(filter.verdict.map(str::to_owned))
        .set_sort(filter.oldest.then(|| "createdAt".to_owned()))
        .max_results(max)
        .set_next_token(next_token)
        .send()
        .await
        .map_err(|e| store.failure("置き場が一覧を返さなかった", e))?;
    let records = listed
        .pages()
        .iter()
        .map(|p| {
            let mut record = page_record!(p, store.base);
            page_state!(record, p);
            record
        })
        .collect();
    let next = listed
        .next_token()
        .filter(|t| !t.is_empty())
        .map(str::to_owned);
    Ok((records, next))
}

async fn list(store: &Store, args: ListArgs) -> Result<String, Failure> {
    let wanted = match &args.json {
        Some(fields) => Some(output::fields(fields, output::PAGE_FIELDS)?),
        None => None,
    };
    let project = args.project.as_deref().map(project_name).transpose()?;
    let filter = Filter {
        project: project.clone(),
        key: args.key,
        kind: args.kind,
        role: args.role,
        workspace: args.workspace,
        q: args.q,
        reply: args.reply.map(ReplyState::name),
        on: args.on.map(Party::name),
        verdict: args.verdict.map(Verdict::name),
        oldest: args.oldest,
    };
    let (pages, next) = page_of(store, &filter, args.limit, args.next_token).await?;
    if let Some(next) = next {
        eprintln!("pagetray: 続きがある（次の頁は、同じ命令に --next-token {next} を付けて打つ）");
    }
    if let Some(wanted) = wanted {
        return Ok(output::records(pages, &wanted));
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
        .iter()
        .map(|p| {
            let field = |name| output::text(p, name).to_owned();
            let mut row = vec![
                field("createdAt"),
                field("key"),
                format!("{}-{}", field("kind"), field("n")),
                field("role"),
                output::reply_of(p),
                field("url"),
                field("title"),
            ];
            if with_project {
                row.insert(1, field("project"));
            }
            row
        })
        .collect();
    Ok(output::table(&header, &rows, tty))
}

/// ページ 1 枚の返事の状態を書く（`SetReplyState`）。
async fn reply(store: &Store, args: ReplyArgs) -> Result<String, Failure> {
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
    let page = store
        .api
        .set_reply_state()
        .project(project)
        .key(key)
        .kind(kind)
        .n(n.to_string())
        .state(state.name())
        .set_on(args.on.map(|on| on.name().to_owned()))
        .send()
        .await
        .map_err(|e| store.failure(&format!("返事の状態を書けない: {}{path}", store.base), e))?;
    let mut record = page_record!(&page, store.base);
    page_state!(record, &page);
    if args.json {
        Ok(output::pretty(&Value::Object(record)))
    } else {
        Ok(format!(
            "{}\t{}\n",
            output::reply_of(&record),
            output::text(&record, "url")
        ))
    }
}

async fn view(store: &Store, args: ViewArgs) -> Result<Vec<u8>, Failure> {
    let path = page_path_of(&args.page)?;
    if args.web {
        crate::open_in_browser(&format!("{}{path}", store.base))?;
        return Ok(Vec::new());
    }
    let body = store
        .send(Route::Own, "GET", &path, &[], None, Some(TIMEOUT))
        .await?
        .ok(&format!("ページを読めない: {}{path}", store.base))
        .await?
        .bytes()
        .await?;
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

async fn put(store: &Store, args: PutArgs) -> Result<String, Failure> {
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
    let placed = store
        .api
        .create_page()
        .project(project)
        .key(args.key)
        .kind(args.kind)
        .role(args.role)
        .workspace(args.workspace)
        .body(Blob::new(body))
        .send()
        .await
        .map_err(|e| store.failure("置き場がページを受け取らなかった", e))?;
    // 答えの url は、置き場が要求の Host（127.0.0.1 と --url の port）で組んだ物をそのまま使う。
    let mut record = page_record!(&placed, store.base);
    record.insert("url".into(), placed.url().into());
    if args.json {
        Ok(output::pretty(&Value::Object(record)))
    } else {
        Ok(format!("{}\n", output::text(&record, "url")))
    }
}

/// 消す物 1 つ: 置き場に頼む引数と、一覧のうち消す物に当たるかを見る path（名指した時）。
struct Target {
    project: String,
    key: Option<String>,
    kind: Option<String>,
    n: Option<String>,
    exact: Option<String>,
}

async fn delete(store: &Store, args: DeleteArgs) -> Result<String, Failure> {
    crate::refuse_delete()?;
    let mut targets = Vec::new();
    for page in &args.pages {
        let path = page_path_of(page)?;
        let (project, key, kind, n) = parse_page_path(&path).unwrap_or_default();
        targets.push(Target {
            project: project.to_owned(),
            key: Some(key.to_owned()),
            kind: Some(kind.to_owned()),
            n: Some(n.to_string()),
            exact: Some(path.clone()),
        });
    }
    if targets.is_empty() {
        let Some(project) = &args.project else {
            return Err(Failure::usage(
                "消すページの URL か、--project <名> [--key <KEY> [--kind <種類>]] を書く",
            ));
        };
        targets.push(Target {
            project: project_name(project)?,
            key: args.key.clone(),
            kind: args.kind.clone(),
            n: None,
            exact: None,
        });
    }

    // まず、消す物を見る（n は一覧の引数に無いので、path で選ぶ）。続きの印を辿り、合う物をみな読む。
    let mut listed: Vec<Map<String, Value>> = Vec::new();
    for target in &targets {
        let filter = Filter {
            project: Some(target.project.clone()),
            key: target.key.clone(),
            kind: target.kind.clone(),
            ..Filter::default()
        };
        let mut next = None;
        loop {
            let (pages, token) = page_of(store, &filter, 1000, next).await?;
            listed.extend(pages.into_iter().filter(|p| {
                target
                    .exact
                    .as_deref()
                    .is_none_or(|path| path == output::text(p, "path"))
            }));
            match token {
                Some(token) => next = Some(token),
                None => break,
            }
        }
    }
    if listed.is_empty() {
        return Ok("置き場に、消すページは無い\n".to_owned());
    }
    let lines: String = listed
        .iter()
        .map(|p| {
            format!(
                "{}\t{}\n",
                output::text(p, "url"),
                output::text(p, "title").replace(['\t', '\n', '\r'], " ")
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
    for target in targets {
        let gone = store
            .api
            .delete_pages()
            .project(target.project)
            .set_key(target.key)
            .set_kind(target.kind)
            .set_n(target.n)
            .send()
            .await;
        match gone {
            Ok(gone) => removed.extend(gone.removed().iter().cloned()),
            Err(e) => {
                // 消した物は戻せないので、途中で落ちても、そこまでに消した物を出してから止まる。
                if !removed.is_empty() {
                    let lines: String = removed
                        .iter()
                        .map(|path| format!("{}{path}\n", store.base))
                        .collect();
                    eprint!("pagetray: 消した（{} 枚）:\n{lines}", removed.len());
                }
                return Err(store.failure("置き場がページを消さなかった", e));
            }
        }
    }
    let lines: String = removed
        .iter()
        .map(|path| format!("{}{path}\n", store.base))
        .collect();
    Ok(format!("消した（{} 枚）:\n{lines}", removed.len()))
}

/// `GET /api/events`（SSE）を読み、ページを置いた知らせ（event の名の無い物）の記録を 1 件ずつ出す。
/// ほかの知らせ（`file`・`reply`・`review`）は出さない。置き場が閉じるか、読む側が閉じるまで続く。
async fn watch(store: &Store, args: WatchArgs) -> Result<(), Failure> {
    let mut reply = store
        .send(Route::Own, "GET", "/api/events", &[], None, None)
        .await?
        .ok("置き場が新着を流さなかった")
        .await?;
    let mut stdout = std::io::stdout().lock();
    let mut events = Events::default();
    while let Some(chunk) = reply.chunk().await? {
        for data in events.push(&chunk) {
            let Ok(Value::Object(mut page)) = serde_json::from_str::<Value>(&data) else {
                continue;
            };
            let url = format!("{}{}", store.base, output::text(&page, "path"));
            page.insert("url".into(), url.into());
            let out = if args.json {
                serde_json::to_string(&page).unwrap_or_default() + "\n"
            } else {
                let field = |name| output::text(&page, name).to_owned();
                let row = vec![
                    field("createdAt"),
                    field("project"),
                    field("key"),
                    format!("{}-{}", field("kind"), field("n")),
                    field("role"),
                    field("url"),
                    field("title"),
                ];
                output::table(&[], &[row], false)
            };
            // 読む側が閉じたら（`| head -1` など）、終わる。
            if stdout
                .write_all(out.as_bytes())
                .and_then(|()| stdout.flush())
                .is_err()
            {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// SSE の行を読み、event の名の無い知らせの data を取り出す。
#[derive(Default)]
struct Events {
    /// まだ改行の来ていない字。
    rest: Vec<u8>,
    /// 読んでいる知らせの event の名。
    name: Option<String>,
    /// 読んでいる知らせの data（行は改行でつなぐ）。
    data: Vec<String>,
}

impl Events {
    /// 届いた byte を足し、読み終えた、ページを置いた知らせの data を返す。
    fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.rest.extend_from_slice(chunk);
        let mut done = Vec::new();
        while let Some(at) = self.rest.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.rest.drain(..=at).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                // 知らせの終わり。
                let data = std::mem::take(&mut self.data);
                if self.name.take().is_none() && !data.is_empty() {
                    done.push(data.join("\n"));
                }
            } else if let Some(value) = field(line, "event") {
                self.name = Some(value.to_owned()).filter(|n| n != "message");
            } else if let Some(value) = field(line, "data") {
                self.data.push(value.to_owned());
            }
        }
        done
    }
}

/// SSE の 1 行 `名: 値` の値（`:` の後ろの空白 1 つは除く）。
fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let value = line.strip_prefix(name)?.strip_prefix(':')?;
    Some(value.strip_prefix(' ').unwrap_or(value))
}

/// `--project` の値を、置き場の project の名にする。`.` は、今いる folder の path の、英数字と `._-` のほかを `-` にした名
/// （roster が project の名を作る形と同じ）。
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
    use super::{Events, folder_name, page_path_of};

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

    #[test]
    fn only_events_without_a_name_are_pages() {
        let mut events = Events::default();
        // 切れ目は、行の途中にも来る。
        assert!(events.push(b"data: {\"a\"").is_empty());
        assert_eq!(events.push(b":1}\n\n"), ["{\"a\":1}"]);
        assert!(events.push(b"event: reply\ndata: {}\n\n").is_empty());
        assert!(events.push(b"event: file\r\ndata: {}\r\n\r\n").is_empty());
        // 注釈の行（keep-alive）は読み飛ばす。
        assert!(events.push(b":\n\n").is_empty());
        assert_eq!(events.push(b"data:x\ndata: y\n\n"), ["x\ny"]);
    }
}
