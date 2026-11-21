//! pagetray: ページの置き場（pagetray-server）を、端末から操作する CLI。
//! 口の約束は docs/src/api.md。置き場に認証は無く、相手はこの Mac の中（127.0.0.1）の置き場だけ。
//!
//! 終わりの code: 0 できた・1 置き場が断った・2 使い方の誤り・3 置き場が応えない・4 口の版が合わない。

mod client;
mod output;
mod page;

use clap::{Args, CommandFactory, Parser, Subcommand};
use client::{Client, Failure, TIMEOUT};
use pagetray_api::{API_VERSION, is_segment};
use serde::Deserialize;
use serde_json::json;
use std::io::Write;
use std::process::ExitCode;

/// 置き場の URL の既定（README の「動かす」の port）。
const DEFAULT_URL: &str = "http://127.0.0.1:8801";

/// ページの置き場（pagetray）を操作する
#[derive(Parser)]
#[command(name = "pagetray", version, disable_help_subcommand = true)]
struct Cli {
    /// 置き場の URL
    #[arg(long, global = true, env = "PAGETRAY_URL", default_value = DEFAULT_URL, value_name = "URL")]
    url: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 置き場が生きているか、版が合っているかを見る
    Status(StatusArgs),
    /// ページを探す・読む
    #[command(subcommand)]
    Page(page::PageCommand),
    /// 一覧の UI を browser で開く
    Browse(BrowseArgs),
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

/// `GET /api/version` の答え。
#[derive(Deserialize)]
struct Version {
    version: String,
    api: u32,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli).and_then(|out| emit(&out)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("pagetray: {}", failure.message);
            ExitCode::from(failure.code)
        }
    }
}

/// 命令を動かし、stdout に出す字を返す。
fn run(cli: Cli) -> Result<Vec<u8>, Failure> {
    match cli.command {
        Command::Status(args) => status(&Client::new(&cli.url, Some(TIMEOUT))?, &args),
        Command::Page(command) => page::run(&Client::new(&cli.url, Some(TIMEOUT))?, command),
        Command::Browse(args) => browse(&Client::new(&cli.url, Some(TIMEOUT))?, &args),
        Command::Completion { shell } => {
            let mut script = Vec::new();
            clap_complete::generate(shell, &mut Cli::command(), "pagetray", &mut script);
            Ok(script)
        }
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

fn status(client: &Client, args: &StatusArgs) -> Result<Vec<u8>, Failure> {
    client
        .get("/healthz", &[])?
        .ok("置き場が生きていると答えなかった")?;
    let server: Version = client
        .get("/api/version", &[])?
        .ok("置き場が版を返さなかった")?
        .json()?;
    let cli = env!("CARGO_PKG_VERSION");
    let out = if args.json {
        output::pretty(&json!({
            "url": client.base,
            "server": server.version,
            "api": server.api,
            "cli": cli,
            "cli_api": API_VERSION,
        }))
    } else {
        format!(
            "置き場: {}（生きている）\nサーバーの版: {}\n口の版: {}（この pagetray が話せる版と合っている）\npagetray の版: {cli}\n",
            client.base, server.version, server.api
        )
    };
    Ok(out.into_bytes())
}

fn browse(client: &Client, args: &BrowseArgs) -> Result<Vec<u8>, Failure> {
    let mut url = format!("{}/", client.base);
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
