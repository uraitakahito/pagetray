//! `pagetray-cli-next file …`: file（画像・PDF・字の file）を探す・読む・置く・消す。
//! 置く前に、人が中身を見たか（`--reviewed`）と、字の file の秘密の形を、手元で見る（送る前に止める）。
//! 手元で見る所（名の作り方・EXIF・秘密の形）は、今の CLI（crates/cli/src/file.rs）と同じ物を写してある
//! （今の CLI は切り替えの日まで替えない。段 9 で今の CLI を消す時に、ここだけが残る）。

use crate::client::{Failure, REFUSED, Route, Store, TIMEOUT};
use crate::output::{self, file_record};
use crate::page::project_name;
use aws_smithy_types::Blob;
use clap::{Args, Subcommand};
use pagetray_api::{
    FILE_TYPES, MAX_FILE, Sniff, file_type, is_segment, parse_file_name, sniff, split_file_name,
};
use serde_json::{Map, Value};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum FileCommand {
    /// file を絞って探す（新しい順）
    List(ListArgs),
    /// file の中身を出す（画像と PDF は、端末には出さない。-o か pipe で）
    View(ViewArgs),
    /// file を置く（番号は置き場が決める。同じ名で置き直すと次の番号になる）
    Put(PutArgs),
    /// file を消す（戻せない）。まず消す物を一覧で出し、--yes を付けた時だけ消す
    Delete(DeleteArgs),
}

#[derive(Args)]
pub struct ListArgs {
    /// project の名（`.` は、今いる folder の path から作る）。無ければ、どの project も
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
    /// 置いた担当の役割
    #[arg(long, value_name = "役割")]
    role: Option<String>,
    /// 置いた担当の居場所
    #[arg(long, value_name = "ID")]
    workspace: Option<String>,
    /// 名（`<語>.<拡張子>`）の字を探す
    #[arg(short, long = "query", value_name = "語")]
    q: Option<String>,
    /// 古い順に出す
    #[arg(long)]
    oldest: bool,
    /// 出す件数（1〜1000）。続きがあれば、続きの印を stderr に出す
    #[arg(short = 'L', long, default_value_t = 50, value_name = "件", value_parser = clap::value_parser!(i32).range(1..=1000))]
    limit: i32,
    /// 前の答えの続き（前に出た続きの印をそのまま書く。同じ並びの物だけ）
    #[arg(long, value_name = "印")]
    next_token: Option<String>,
    /// JSON で出す。欄を `name,url` のように名指すと、その欄だけ
    #[arg(long, value_name = "欄", num_args = 0..=1, default_missing_value = "", require_equals = true)]
    json: Option<String>,
}

#[derive(Args)]
pub struct ViewArgs {
    /// file の URL か、`<project>/<key>/files/<語>-<番号>.<拡張子>`
    #[arg(allow_hyphen_values = true)]
    file: String,
    /// 中身を file に書く
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,
}

#[derive(Args)]
pub struct PutArgs {
    /// 置く file
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
    /// 置く名（`<語>.<拡張子>`）。無ければ、file の名から作る
    #[arg(long, value_name = "名")]
    name: Option<String>,
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
    /// 中身を開いて、秘密や個人の情報が写っていないかを見た（画像と PDF に要る）
    #[arg(long)]
    reviewed: bool,
    /// 字の file に秘密の形があっても置く
    #[arg(long)]
    allow_secret: bool,
    /// 置いた file の記録を JSON で出す（既定は URL と、ページからの参照）
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
pub struct DeleteArgs {
    /// 消す file の URL か、`<project>/<key>/files/<語>-<番号>.<拡張子>`（`-` で始まる名は、URL で書くか `--` の後ろに書く）
    // いくつも受ける引数に `-` で始まる値を許すと、後ろの `--yes` まで値として読んでしまうので、許さない。
    #[arg(conflicts_with_all = ["project", "key", "name"])]
    files: Vec<String>,
    /// この project の file を消す（--key が無ければ、project の file をみな）。環境変数からは読まない
    #[arg(short, long, value_name = "名", allow_hyphen_values = true)]
    project: Option<String>,
    /// この Key の file だけ
    #[arg(short, long, value_name = "KEY", requires = "project")]
    key: Option<String>,
    /// この名（`<語>.<拡張子>`）の file だけ（どの番号も）
    #[arg(long, value_name = "名", requires = "key")]
    name: Option<String>,
    /// 一覧を見せるだけでなく、本当に消す
    #[arg(long)]
    yes: bool,
}

/// stdout に出す字を返す。
pub async fn run(store: &Store, command: FileCommand) -> Result<Vec<u8>, Failure> {
    match command {
        FileCommand::List(args) => list(store, args).await.map(String::into_bytes),
        FileCommand::View(args) => view(store, args).await,
        FileCommand::Put(args) => put(store, args).await.map(String::into_bytes),
        FileCommand::Delete(args) => delete(store, args).await.map(String::into_bytes),
    }
}

/// 探す時の条件（`ListFiles` の引数）。
#[derive(Default)]
struct Filter {
    project: Option<String>,
    key: Option<String>,
    role: Option<String>,
    workspace: Option<String>,
    q: Option<String>,
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
        .list_files()
        .set_project(filter.project.clone())
        .set_key(filter.key.clone())
        .set_role(filter.role.clone())
        .set_workspace(filter.workspace.clone())
        .set_q(filter.q.clone())
        .set_sort(filter.oldest.then(|| "createdAt".to_owned()))
        .max_results(max)
        .set_next_token(next_token)
        .send()
        .await
        .map_err(|e| store.failure("置き場が一覧を返さなかった", e))?;
    let records = listed
        .files()
        .iter()
        .map(|f| file_record!(f, store.base))
        .collect();
    let next = listed
        .next_token()
        .filter(|t| !t.is_empty())
        .map(str::to_owned);
    Ok((records, next))
}

async fn list(store: &Store, args: ListArgs) -> Result<String, Failure> {
    let wanted = match &args.json {
        Some(fields) => Some(output::fields(fields, output::FILE_FIELDS)?),
        None => None,
    };
    let project = args.project.as_deref().map(project_name).transpose()?;
    let filter = Filter {
        project: project.clone(),
        key: args.key,
        role: args.role,
        workspace: args.workspace,
        q: args.q,
        oldest: args.oldest,
    };
    let (files, next) = page_of(store, &filter, args.limit, args.next_token).await?;
    if let Some(next) = next {
        eprintln!(
            "pagetray-cli-next: 続きがある（次の頁は、同じ命令に --next-token {next} を付けて打つ）"
        );
    }
    if let Some(wanted) = wanted {
        return Ok(output::records(files, &wanted));
    }
    let tty = std::io::stdout().is_terminal();
    if files.is_empty() && tty {
        return Ok("合う file は無い\n".to_owned());
    }
    // project で絞った時は、どの行も同じになる project の欄を出さない。
    let with_project = project.is_none();
    let mut header = vec!["CREATED", "KEY", "FILE", "SIZE", "ROLE", "URL"];
    if with_project {
        header.insert(1, "PROJECT");
    }
    let rows: Vec<Vec<String>> = files
        .iter()
        .map(|f| {
            let field = |name| output::text(f, name).to_owned();
            let mut row = vec![
                field("createdAt"),
                field("key"),
                format!("{}-{}.{}", field("name"), field("n"), field("ext")),
                f.get("size").map(ToString::to_string).unwrap_or_default(),
                field("role"),
                field("url"),
            ];
            if with_project {
                row.insert(1, field("project"));
            }
            row
        })
        .collect();
    Ok(output::table(&header, &rows, tty))
}

async fn view(store: &Store, args: ViewArgs) -> Result<Vec<u8>, Failure> {
    let path = file_path_of(&args.file)?;
    // 画像と PDF の byte を端末に流すと、画面が崩れる。頼む前に止める。
    if args.output.is_none() && !is_text(&path) && std::io::stdout().is_terminal() {
        return Err(Failure::usage(
            "画像と PDF の中身は端末に出さない。-o <file> で file に書くか、pipe でつなぐ",
        ));
    }
    let body = store
        .send(Route::Own, "GET", &path, &[], None, Some(TIMEOUT))
        .await?
        .ok(&format!("file を読めない: {}{path}", store.base))
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

/// path の拡張子が、字の file の物か。
fn is_text(path: &str) -> bool {
    path.rsplit_once('.')
        .and_then(|(_, ext)| file_type(ext))
        .is_some_and(|(_, kind)| kind == Sniff::Text)
}

async fn put(store: &Store, args: PutArgs) -> Result<String, Failure> {
    let project = project_name(&args.project)?;
    let (stem, ext) = match &args.name {
        Some(name) => {
            let (stem, ext) = split_file_name(name).ok_or_else(|| {
                Failure::usage(format!(
                    "--name は <語>.<拡張子>。語は英小文字・数字・- _ で 64 字まで（英小文字か数字で始まる）: {name}"
                ))
            })?;
            (stem.to_owned(), ext.to_owned())
        }
        None => name_from(&args.file)?,
    };
    let Some((mime, kind)) = file_type(&ext) else {
        return Err(Failure::usage(format!(
            ".{ext} は置けない（置けるのは {}）",
            accepted()
        )));
    };
    let body = std::fs::read(&args.file)
        .map_err(|e| Failure::usage(format!("file を読めない（{}）: {e}", args.file.display())))?;
    let shown = args.file.display();
    if body.is_empty() {
        return Err(Failure::usage(format!("file が空: {shown}")));
    }
    if body.len() > MAX_FILE {
        return Err(Failure::usage(format!(
            "file が大きすぎる（{} bytes。1 つ {} MiB まで）: {shown}",
            body.len(),
            MAX_FILE / 1024 / 1024
        )));
    }
    if !sniff(kind, &body) {
        return Err(Failure::usage(format!("中身が .{ext} の形でない: {shown}")));
    }
    if kind == Sniff::Jpeg && has_exif(&body) {
        eprintln!(
            "pagetray-cli-next: 注意: この JPEG には EXIF があり、写した場所（位置情報）が入っているかもしれない。消すなら、exiftool -all= などで消してから置く: {shown}"
        );
    }
    if kind == Sniff::Text {
        let lines = secret_lines(&String::from_utf8_lossy(&body));
        if !lines.is_empty() {
            let at = lines
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("・");
            if !args.allow_secret {
                return Err(Failure::new(
                    REFUSED,
                    format!(
                        "秘密（token・鍵）の形の字が {at} 行目にあるので、置かない（何も送っていない）。消してから置き直す。わざと置くなら --allow-secret を付ける: {shown}"
                    ),
                ));
            }
            eprintln!(
                "pagetray-cli-next: 注意: 秘密（token・鍵）の形の字が {at} 行目にあるが、--allow-secret が付いているので置く: {shown}"
            );
        }
    } else if !args.reviewed {
        return Err(Failure::new(
            REFUSED,
            format!(
                "画像と PDF は、置く前に人が中身を見る（何も送っていない）。その file を開いて（担当なら Read tool で）、token・鍵・個人の情報・ほかの人の画面・この Mac の path（home の folder の名など）が写っていないかを確かめ、--reviewed を付けて打ち直す: {shown}"
            ),
        ));
    }

    let placed = store
        .api
        .create_file()
        .project(project)
        .key(args.key)
        .name(format!("{stem}.{ext}"))
        .role(args.role)
        .workspace(args.workspace)
        .content_type(mime)
        .body(Blob::new(body))
        .send()
        .await
        .map_err(|e| store.failure("置き場が file を受け取らなかった", e))?;
    // 答えの url は、置き場が要求の Host（127.0.0.1 と --url の port）で組んだ物をそのまま使う。
    let mut record = file_record!(&placed, store.base);
    record.insert("url".into(), placed.url().into());
    if args.json {
        return Ok(output::pretty(&Value::Object(record)));
    }
    // 2 行目は、同じ Key のページから参照する相対の path。
    Ok(format!(
        "{}\nfiles/{}-{}.{}\n",
        output::text(&record, "url"),
        placed.name(),
        placed.n(),
        placed.ext()
    ))
}

fn accepted() -> String {
    FILE_TYPES
        .iter()
        .map(|(ext, _, _)| *ext)
        .collect::<Vec<_>>()
        .join("・")
}

/// file の名から、置く名の語と拡張子を作る。
fn name_from(file: &Path) -> Result<(String, String), Failure> {
    let base = file
        .file_name()
        .map(|b| b.to_string_lossy().into_owned())
        .unwrap_or_default();
    match base.rsplit_once('.') {
        Some((stem, ext)) if !ext.is_empty() => Ok((stem_from(stem), ext.to_ascii_lowercase())),
        _ => Err(Failure::usage(format!(
            "file の名に拡張子が無い。--name <語>.<拡張子> で名を付ける: {}",
            file.display()
        ))),
    }
}

/// 名の語を作る: 英小文字にし、英小文字・数字・`-`・`_` のほかが続く所を `-` 1 つにし、頭と尻の `-`（頭は `_` も）を
/// 除いて 64 字に切る。字が残らなければ `file`。
fn stem_from(stem: &str) -> String {
    let mut out = String::new();
    for c in stem.chars().map(|c| c.to_ascii_lowercase()) {
        if matches!(c, 'a'..='z' | '0'..='9' | '-' | '_') {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_start_matches(['-', '_']);
    let out: String = out.chars().take(64).collect();
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        "file".to_owned()
    } else {
        out.to_owned()
    }
}

/// JPEG の頭の区切り（SOS の前まで）に、EXIF の APP1（`FF E1` と `Exif\0\0`）があるか。
fn has_exif(bytes: &[u8]) -> bool {
    let mut at = 2; // FF D8 の後ろ
    while at + 4 <= bytes.len() {
        if bytes[at] != 0xff {
            return false;
        }
        let marker = bytes[at + 1];
        match marker {
            // 詰め物の FF。
            0xff => {
                at += 1;
                continue;
            }
            // 長さを持たない印。
            0x01 | 0xd0..=0xd7 => {
                at += 2;
                continue;
            }
            // 絵の本体が始まる・終わる。この後ろに EXIF は無い。
            0xda | 0xd9 => return false,
            _ => {}
        }
        let len = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        if len < 2 {
            return false;
        }
        if marker == 0xe1 && bytes[at + 4..].starts_with(b"Exif\0\0") {
            return true;
        }
        at += 2 + len;
    }
    false
}

/// 秘密の形 1 つ: 頭の字・その後ろに続く字の種類・その字の最少の数。
/// 頭の字は、逆さに書く（そのまま書くと binary に token の頭が入り、ci/scan-binary.sh の走査に当たるため）。
struct Secret {
    reversed_prefix: &'static str,
    tail: fn(u8) -> bool,
    min: usize,
}

fn token_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')
}

/// 字の file で探す秘密の形（ci/strings-deny.txt の token の頭と揃える）。
const SECRETS: &[Secret] = &[
    // GitHub の token（ci/strings-deny.txt の gh[pousr]_）。
    Secret {
        reversed_prefix: "_phg",
        tail: |b| b.is_ascii_alphanumeric(),
        min: 30,
    },
    Secret {
        reversed_prefix: "_ohg",
        tail: |b| b.is_ascii_alphanumeric(),
        min: 30,
    },
    Secret {
        reversed_prefix: "_uhg",
        tail: |b| b.is_ascii_alphanumeric(),
        min: 30,
    },
    Secret {
        reversed_prefix: "_shg",
        tail: |b| b.is_ascii_alphanumeric(),
        min: 30,
    },
    Secret {
        reversed_prefix: "_rhg",
        tail: |b| b.is_ascii_alphanumeric(),
        min: 30,
    },
    Secret {
        reversed_prefix: "_tap_buhtig",
        tail: token_char,
        min: 20,
    },
    // OpenAI・Anthropic などの API の鍵。
    Secret {
        reversed_prefix: "-ks",
        tail: token_char,
        min: 20,
    },
    // AWS の access key（後ろは英大文字と数字で 16 字）。
    Secret {
        reversed_prefix: "AIKA",
        tail: |b| b.is_ascii_uppercase() || b.is_ascii_digit(),
        min: 16,
    },
    // Slack の token。
    Secret {
        reversed_prefix: "-bxox",
        tail: token_char,
        min: 10,
    },
    Secret {
        reversed_prefix: "-pxox",
        tail: token_char,
        min: 10,
    },
    // Atlassian の API token。
    Secret {
        reversed_prefix: "TTATA",
        tail: |b| token_char(b) || b == b'=',
        min: 20,
    },
];

/// 秘密の鍵の頭の行（`-----BEGIN [A-Z ]*PRIVATE KEY`）の、前と後ろの字（これも逆さに書く）。
const KEY_BEGIN: &str = " NIGEB-----";
const KEY_END: &str = "YEK ETAVIRP";

fn unreversed(s: &str) -> String {
    s.chars().rev().collect()
}

/// 秘密の形の字がある行の番号（1 から）。
fn secret_lines(text: &str) -> Vec<usize> {
    let secrets: Vec<(String, &Secret)> = SECRETS
        .iter()
        .map(|s| (unreversed(s.reversed_prefix), s))
        .collect();
    let (begin, end) = (unreversed(KEY_BEGIN), unreversed(KEY_END));
    let mut hits = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let bytes = line.as_bytes();
        let token = secrets.iter().any(|(prefix, secret)| {
            line.match_indices(prefix.as_str()).any(|(at, _)| {
                // 語の途中（`task-…` の `sk-` など）は見ない。
                let starts_word = at == 0 || !bytes[at - 1].is_ascii_alphanumeric();
                let tail = bytes[at + prefix.len()..]
                    .iter()
                    .take_while(|b| (secret.tail)(**b))
                    .count();
                starts_word && tail >= secret.min
            })
        });
        let key = line.match_indices(begin.as_str()).any(|(at, _)| {
            let rest = &line[at + begin.len()..];
            let words = rest
                .bytes()
                .take_while(|b| b.is_ascii_uppercase() || *b == b' ')
                .count();
            rest[..words].contains(end.as_str())
        });
        if token || key {
            hits.push(i + 1);
        }
    }
    hits
}

/// 消す物 1 つ: 置き場に頼む引数と、一覧のうち消す物に当たるかを見る path（名指した時）か名（`--name` の時）。
struct Target {
    project: String,
    key: Option<String>,
    name: Option<String>,
    n: Option<String>,
    exact: Option<String>,
}

impl Target {
    fn covers(&self, file: &Map<String, Value>) -> bool {
        self.exact
            .as_deref()
            .is_none_or(|path| path == output::text(file, "path"))
            && self.name.as_deref().is_none_or(|name| {
                name == format!(
                    "{}.{}",
                    output::text(file, "name"),
                    output::text(file, "ext")
                )
            })
    }
}

async fn delete(store: &Store, args: DeleteArgs) -> Result<String, Failure> {
    crate::refuse_delete()?;
    let mut targets = Vec::new();
    for file in &args.files {
        let path = file_path_of(file)?;
        let parts: Vec<&str> = path.split('/').collect();
        let (name, ext, n) = parse_file_name(parts[4]).unwrap_or_default();
        targets.push(Target {
            project: parts[1].to_owned(),
            key: Some(parts[2].to_owned()),
            name: Some(format!("{name}.{ext}")),
            n: Some(n.to_string()),
            exact: Some(path.clone()),
        });
    }
    if targets.is_empty() {
        let Some(project) = &args.project else {
            return Err(Failure::usage(
                "消す file の URL か、--project <名> [--key <KEY> [--name <語>.<拡張子>]] を書く",
            ));
        };
        if let Some(full) = &args.name {
            split_file_name(full)
                .ok_or_else(|| Failure::usage(format!("--name は <語>.<拡張子>: {full}")))?;
        }
        targets.push(Target {
            project: project_name(project)?,
            key: args.key.clone(),
            name: args.name.clone(),
            n: None,
            exact: None,
        });
    }

    // まず、消す物を見る（name と n は一覧の引数に無いので、ここで選ぶ）。続きの印を辿り、合う物をみな読む。
    let mut listed: Vec<Map<String, Value>> = Vec::new();
    for target in &targets {
        let filter = Filter {
            project: Some(target.project.clone()),
            key: target.key.clone(),
            ..Filter::default()
        };
        let mut next = None;
        loop {
            let (files, token) = page_of(store, &filter, 1000, next).await?;
            listed.extend(files.into_iter().filter(|f| target.covers(f)));
            match token {
                Some(token) => next = Some(token),
                None => break,
            }
        }
    }
    if listed.is_empty() {
        return Ok("置き場に、消す file は無い\n".to_owned());
    }
    let lines: String = listed
        .iter()
        .map(|f| {
            format!(
                "{}\t{} bytes\n",
                output::text(f, "url"),
                f.get("size").map(ToString::to_string).unwrap_or_default()
            )
        })
        .collect();
    if !args.yes {
        return Ok(format!(
            "消す file（{} 個。戻せない。まだ消していない）:\n{lines}消すには、同じ命令に --yes を付ける\n",
            listed.len()
        ));
    }
    let mut removed: Vec<String> = Vec::new();
    for target in targets {
        let gone = store
            .api
            .delete_files()
            .project(target.project)
            .set_key(target.key)
            .set_name(target.name)
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
                    eprint!(
                        "pagetray-cli-next: 消した（{} 個）:\n{lines}",
                        removed.len()
                    );
                }
                return Err(store.failure("置き場が file を消さなかった", e));
            }
        }
    }
    let lines: String = removed
        .iter()
        .map(|path| format!("{}{path}\n", store.base))
        .collect();
    Ok(format!("消した（{} 個）:\n{lines}", removed.len()))
}

/// file の名指し（URL か `<project>/<key>/files/<語>-<番号>.<拡張子>`）を、置き場の中の path にする。
/// URL は、host と port を見ずに path だけを使う（頼む先は、いつも `--url` の置き場）。
fn file_path_of(file: &str) -> Result<String, Failure> {
    let path = match file.split_once("://") {
        Some((_, rest)) => {
            let rest = rest.split(['?', '#']).next().unwrap_or("");
            rest.find('/').map_or("", |at| &rest[at..]).to_owned()
        }
        None => format!("/{}", file.trim_start_matches('/')),
    };
    let parts: Vec<&str> = path.split('/').collect();
    let ok = parts.len() == 5
        && is_segment(parts[1])
        && is_segment(parts[2])
        && parts[3] == "files"
        && parse_file_name(parts[4]).is_some();
    if ok {
        Ok(path)
    } else {
        Err(Failure::usage(format!(
            "file の URL か <project>/<key>/files/<語>-<番号>.<拡張子> を書く: {file}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_come_from_the_file_name() {
        let name = |f: &str| name_from(Path::new(f)).unwrap();
        assert_eq!(
            name("/tmp/Screenshot 2026-11-21 at 10.00.png"),
            (
                "screenshot-2026-11-21-at-10-00".to_owned(),
                "png".to_owned()
            )
        );
        assert_eq!(
            name("a/PHOTO.JPEG"),
            ("photo".to_owned(), "jpeg".to_owned())
        );
        assert_eq!(name("画像.png"), ("file".to_owned(), "png".to_owned()));
        assert_eq!(name("__a__b--.txt"), ("a__b".to_owned(), "txt".to_owned()));
        assert_eq!(name(".png").0, "file");
        let long = name(&format!("{}-x.md", "a".repeat(63))).0;
        assert_eq!(long, "a".repeat(63));
        assert!(name_from(Path::new("README")).is_err());
        assert!(name_from(Path::new("x.")).is_err());
    }

    #[test]
    fn files_are_named_by_url_or_by_parts() {
        for file in [
            "http://127.0.0.1:8801/p/AI-24/files/screen-2.png",
            "http://127.0.0.1:8800/p/AI-24/files/screen-2.png?x#y",
            "p/AI-24/files/screen-2.png",
            "/p/AI-24/files/screen-2.png",
        ] {
            assert_eq!(
                file_path_of(file).unwrap(),
                "/p/AI-24/files/screen-2.png",
                "{file}"
            );
        }
        for ng in [
            "p/AI-24/screen-2.png",
            "p/AI-24/files/screen.png",
            "p/AI-24/files/screen-2.svg",
            "p/AI-24/plan-1.html",
            "../p/AI-24/files/screen-2.png",
            "http://127.0.0.1:8801/",
            "",
        ] {
            assert!(file_path_of(ng).is_err(), "{ng}");
        }
    }

    #[test]
    fn exif_is_found_only_before_the_image_data() {
        let app0 = b"\xff\xe0\x00\x10JFIF\0\x01\x01\0\0\x01\0\x01\0\0";
        let exif = b"\xff\xe1\x00\x0eExif\0\0MM\0\x2a\0\0";
        let sos = b"\xff\xda\x00\x02";
        let jpeg = |parts: &[&[u8]]| [&b"\xff\xd8"[..], &parts.concat()].concat();
        assert!(has_exif(&jpeg(&[app0, exif, sos])));
        assert!(!has_exif(&jpeg(&[app0, sos])));
        assert!(!has_exif(&jpeg(&[app0, sos, exif])));
        assert!(!has_exif(b"\xff\xd8\xff"));
        assert!(!has_exif(&jpeg(&[b"\xff\xe1\x00\x08XMP\0\0\0"])));
    }

    /// 試験の秘密の形も、source にそのまま書かない（source の走査に当たるため）。
    fn fake(reversed: &str, tail: &str) -> String {
        unreversed(reversed) + tail
    }

    #[test]
    fn secret_shapes_are_found_by_line() {
        let tail = "x".repeat(36);
        let text = [
            "ふつうの行".to_owned(),
            format!("t: {}", fake("_phg", &tail)),
            "task-abcdefghijklmnopqrstuvwxyz".to_owned(),
            fake("AIKA", "ABCDEFGHIJ234567"),
            fake(KEY_BEGIN, "RSA PRIVATE KEY-----"),
            format!("x={}", fake("-ks", "proj-abcdefghijklmnopqrstuvwxyz")),
            fake("_phg", "short"),
            fake("-bxox", "1234567890-abc"),
            fake("TTATA", &tail),
            fake("_tap_buhtig", &tail),
            fake("_ohg", &tail),
            fake(KEY_BEGIN, "PUBLIC KEY-----"),
        ]
        .join("\n");
        assert_eq!(secret_lines(&text), [2, 4, 5, 6, 8, 9, 10, 11]);
        assert!(secret_lines("# 見出し\nふつうの字\n").is_empty());
    }
}
