//! pagetray-cli-next: 新しい API の置き場（新しい server。binary pagetray-next）を、端末から操作する CLI（AI-36）。
//! 形の決まりは docs/src/next.md（「CLI」の節）。置き場に認証は無く、相手はこの Mac の中の置き場だけ。
//!
//! 終わりの code: 0 できた・1 置き場が断った・2 使い方の誤り・3 置き場が応えない・
//! 4 置き場の API が、この CLI の話す形でない（今の API の置き場・定義より古い置き場）。

mod client;
mod file;
mod output;
mod page;

use clap::{Args, CommandFactory, Parser, Subcommand};
use client::{Failure, REFUSED, Store, TIMEOUT};
use pagetray_api::is_segment;
use serde_json::json;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

/// 置き場の URL の既定（README の「動かす」の port）。
const DEFAULT_URL: &str = "http://127.0.0.1:8801";

/// ページの置き場（pagetray）を操作する（新しい API 向け）
#[derive(Parser)]
#[command(name = "pagetray-cli-next", version, disable_help_subcommand = true)]
struct Cli {
    /// 置き場の URL
    #[arg(long, global = true, env = "PAGETRAY_URL", default_value = DEFAULT_URL, value_name = "URL")]
    url: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 置き場が生きているかと、サーバーの版を見る
    Status(StatusArgs),
    /// ページを探す・読む・置く・消す
    #[command(subcommand)]
    Page(page::PageCommand),
    /// file（画像・PDF・字の file）を探す・読む・置く・消す
    #[command(subcommand)]
    File(file::FileCommand),
    /// 一覧の UI を browser で開く
    Browse(BrowseArgs),
    /// 置き場の API を直に呼ぶ（命令がまだ無い操作にも届く）
    Api(ApiArgs),
    /// shell の補完の script を出す
    Completion {
        /// shell の名
        shell: clap_complete::Shell,
    },
}

#[derive(Args)]
struct StatusArgs {
    /// JSON で出す
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct BrowseArgs {
    /// 絞る先: `<project>` か `<project>/<key>`
    #[arg(allow_hyphen_values = true)]
    target: Option<String>,
    /// 開かずに、URL だけを出す
    #[arg(short = 'n', long)]
    no_browser: bool,
}

#[derive(Args)]
struct ApiArgs {
    /// 置き場の中の path（`/api/pages` など）
    path: String,
    /// HTTP の method（既定は GET。--input があれば POST）
    #[arg(short = 'X', long, value_name = "METHOD")]
    method: Option<String>,
    /// query の引数（`名=値`。何度でも書ける）
    #[arg(
        short,
        long = "field",
        value_name = "名=値",
        allow_hyphen_values = true
    )]
    fields: Vec<String>,
    /// 要求の本文にする file（`-` は stdin）
    #[arg(long, value_name = "FILE")]
    input: Option<PathBuf>,
    /// 本文の種類（ページを置く時は text/html、file を置く時は file の種類）
    #[arg(long, default_value = "application/json", value_name = "TYPE")]
    content_type: String,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("pagetray-cli-next: 動かせない: {e}");
            return ExitCode::from(1);
        }
    };
    match runtime.block_on(run(cli)).and_then(|out| emit(&out)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("pagetray-cli-next: {}", failure.message);
            ExitCode::from(failure.code)
        }
    }
}

/// 命令を動かし、stdout に出す字を返す。
async fn run(cli: Cli) -> Result<Vec<u8>, Failure> {
    if let Command::Completion { shell } = cli.command {
        let mut script = Vec::new();
        clap_complete::generate(shell, &mut Cli::command(), "pagetray-cli-next", &mut script);
        return Ok(script);
    }
    let store = Store::new(&cli.url)?;
    match cli.command {
        Command::Status(args) => status(&store, &args).await,
        Command::Page(command) => page::run(&store, command).await,
        Command::File(command) => file::run(&store, command).await,
        Command::Api(args) => api(&store, &args).await,
        Command::Browse(args) => browse(&store, &args),
        Command::Completion { .. } => unreachable!("上で返した"),
    }
}

/// stdout に書く。読む側が先に閉じた時（`| head` など）は、誤りにしない。
fn emit(out: &[u8]) -> Result<(), Failure> {
    let mut stdout = std::io::stdout().lock();
    match stdout.write_all(out).and_then(|()| stdout.flush()) {
        Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => {
            Err(Failure::new(1, format!("出力を書けない: {e}")))
        }
        _ => Ok(()),
    }
}

/// `GET /healthz`（`GetHealth`）で、生きているかとサーバーの版を見る。形の版は、定義の release の tag との
/// `smithy diff` が守るので、版の突き合わせはしない（今の API の置き場と、定義より古い置き場は code 4）。
async fn status(store: &Store, args: &StatusArgs) -> Result<Vec<u8>, Failure> {
    let health = store
        .api
        .get_health()
        .send()
        .await
        .map_err(|e| store.failure("置き場が生きていると答えなかった", e))?;
    let cli = env!("CARGO_PKG_VERSION");
    let out = if args.json {
        output::pretty(&json!({
            "url": store.base,
            "server": health.version(),
            "cli": cli,
        }))
    } else {
        format!(
            "置き場: {}（{}）\nサーバーの版: {}\npagetray-cli-next の版: {cli}\n",
            store.base,
            if health.status() == "ok" {
                "生きている"
            } else {
                health.status()
            },
            health.version()
        )
    };
    Ok(out.into_bytes())
}

/// API を直に呼び、答えの本文をそのまま出す。2xx でなければ、本文を出した上で code 1。
async fn api(store: &Store, args: &ApiArgs) -> Result<Vec<u8>, Failure> {
    if !args.path.starts_with('/') {
        return Err(Failure::usage(format!(
            "path は / で始める（/api/pages など）: {}",
            args.path
        )));
    }
    let body =
        match &args.input {
            Some(file) if file.as_os_str() == "-" => {
                let mut body = Vec::new();
                std::io::stdin()
                    .read_to_end(&mut body)
                    .map_err(|e| Failure::usage(format!("stdin を読めない: {e}")))?;
                Some(body)
            }
            Some(file) => Some(std::fs::read(file).map_err(|e| {
                Failure::usage(format!("file を読めない（{}）: {e}", file.display()))
            })?),
            None => None,
        };
    let method = match &args.method {
        Some(method) => method.to_ascii_uppercase(),
        None if body.is_some() => "POST".to_owned(),
        None => "GET".to_owned(),
    };
    if method == "DELETE" {
        refuse_delete()?;
    }
    let mut query = Vec::new();
    for field in &args.fields {
        let Some(pair) = field.split_once('=') else {
            return Err(Failure::usage(format!(
                "--field は 名=値 の形で書く: {field}"
            )));
        };
        query.push(pair);
    }
    let reply = store
        .send(
            &method,
            &args.path,
            &query,
            body.map(|b| (b, args.content_type.as_str())),
            Some(TIMEOUT),
        )
        .await?;
    if reply.is_success() {
        return reply.bytes().await;
    }
    let status = reply.status;
    let kind = reply.error_type.clone().unwrap_or_else(|| "?".to_owned());
    let answer = reply.bytes().await?;
    emit(&answer)?;
    Err(Failure::new(
        REFUSED,
        format!("置き場が {status} {kind} を返した: {method} {}", args.path),
    ))
}

fn browse(store: &Store, args: &BrowseArgs) -> Result<Vec<u8>, Failure> {
    let mut url = format!("{}/", store.base);
    if let Some(target) = &args.target {
        let parts: Vec<&str> = target.trim_matches('/').split('/').collect();
        if parts.len() > 2 || !parts.iter().all(|part| is_segment(part)) {
            return Err(Failure::usage(format!(
                "<project> か <project>/<key> を書く: {target}"
            )));
        }
        for part in parts {
            url.push_str(part);
            url.push('/');
        }
    }
    if args.no_browser {
        return Ok(format!("{url}\n").into_bytes());
    }
    open_in_browser(&url)?;
    Ok(Vec::new())
}

/// `PAGETRAY_NO_DELETE` が付いていれば（空と `0` のほか）、消す命令（page delete・file delete・api の DELETE）は
/// 何もせずに止まる。消してよい人を呼ぶ側が決める時に使う（roster は、担当を起こす時にこれを付ける）。
pub fn refuse_delete() -> Result<(), Failure> {
    match std::env::var("PAGETRAY_NO_DELETE") {
        Ok(value) if !value.is_empty() && value != "0" => Err(Failure::new(
            REFUSED,
            "PAGETRAY_NO_DELETE が付いているので、消さない（何も消していない）",
        )),
        _ => Ok(()),
    }
}

/// URL を browser で開く。`$BROWSER` があればその命令で、無ければ OS の命令（macOS は `open`）で。
fn open_in_browser(url: &str) -> Result<(), Failure> {
    let program = std::env::var("BROWSER")
        .ok()
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| {
            if cfg!(target_os = "macos") {
                "open"
            } else {
                "xdg-open"
            }
            .to_owned()
        });
    match std::process::Command::new(&program).arg(url).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(Failure::new(
            1,
            format!("{program} が開けなかった（{status}）: {url}"),
        )),
        Err(e) => Err(Failure::new(
            1,
            format!("{program} を動かせない（{e}）。URL: {url}"),
        )),
    }
}
