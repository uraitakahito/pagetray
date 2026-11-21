//! pagetray: ページの置き場（pagetray-server）を、端末から操作する CLI。
//! API の仕様は docs/src/api.md。置き場に認証は無く、相手はこの Mac の中（127.0.0.1）の置き場だけ。
//!
//! 終わりの code: 0 できた・1 置き場が断った・2 使い方の誤り・3 置き場が応えない・4 API の版が合わない。

mod client;
mod file;
mod output;
mod page;

use clap::{Args, CommandFactory, Parser, Subcommand};
use client::{Client, Failure, REFUSED, TIMEOUT};
use pagetray_api::{API_VERSION, is_segment};
use serde::Deserialize;
use serde_json::json;
use std::io::{Read, Write};
use std::path::PathBuf;
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
    /// ページを探す・読む・置く・消す
    #[command(subcommand)]
    Page(page::PageCommand),
    /// file（画像・PDF・字の file）を探す・読む・置く・消す
    #[command(subcommand)]
    File(file::FileCommand),
    /// 一覧の UI を browser で開く
    Browse(BrowseArgs),
    /// DB の写し（SQLite の file 1 つ）を取る
    Backup(BackupArgs),
    /// 置き場の API を直に呼ぶ（命令がまだ無いエンドポイントにも届く）
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
struct BackupArgs {
    /// 写しを書く file（置かれたページの本文がみな入る。repo にも CI にも置かない）
    #[arg(short, long, value_name = "FILE")]
    output: PathBuf,
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
        // 新着は、置かれるまで待ち続ける。
        Command::Page(command @ page::PageCommand::Watch(_)) => {
            page::run(&Client::new(&cli.url, None)?, command)
        }
        Command::Page(command) => page::run(&Client::new(&cli.url, Some(TIMEOUT))?, command),
        Command::File(command) => file::run(&Client::new(&cli.url, Some(TIMEOUT))?, command),
        Command::Backup(args) => backup(&Client::new(&cli.url, None)?, &args),
        Command::Api(args) => api(&Client::new(&cli.url, Some(TIMEOUT))?, &args),
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
            "置き場: {}（生きている）\nサーバーの版: {}\nAPI の版: {}（この pagetray が話せる版と合っている）\npagetray の版: {cli}\n",
            client.base, server.version, server.api
        )
    };
    Ok(out.into_bytes())
}

/// DB の写しを file に書く。途中の file は残さない（書き終えてから、名を付け替える）。
fn backup(client: &Client, args: &BackupArgs) -> Result<Vec<u8>, Failure> {
    let reply = client
        .get("/api/backup", &[])?
        .ok("置き場が DB の写しを返さなかった")?;
    let mut partial = args.output.clone().into_os_string();
    partial.push(format!(".part-{}", std::process::id()));
    let partial = PathBuf::from(partial);
    let written = (|| -> std::io::Result<u64> {
        let mut file = std::fs::File::create(&partial)?;
        let size = std::io::copy(&mut reply.reader(), &mut file)?;
        file.sync_all()?;
        // SQLite の file は、決まった 16 字で始まる。違えば、写しではない。
        let mut head = [0u8; 16];
        std::fs::File::open(&partial)?.read_exact(&mut head)?;
        if &head != b"SQLite format 3\0" {
            return Err(std::io::Error::other("SQLite の file でない"));
        }
        std::fs::rename(&partial, &args.output)?;
        Ok(size)
    })();
    match written {
        Ok(size) => Ok(format!("写した: {}（{size} bytes）\n", args.output.display()).into_bytes()),
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            Err(Failure::new(
                1,
                format!("写しを書けない（{}）: {e}", args.output.display()),
            ))
        }
    }
}

/// API を直に呼び、答えの本文をそのまま出す。2xx でなければ、本文を出した上で code 1。
fn api(client: &Client, args: &ApiArgs) -> Result<Vec<u8>, Failure> {
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
    let reply = client.send(
        &method,
        &args.path,
        &query,
        body.as_deref().map(|b| (b, "text/html; charset=utf-8")),
    )?;
    let status = reply.status;
    let answer = reply.bytes()?;
    if (200..300).contains(&status) {
        return Ok(answer);
    }
    emit(&answer)?;
    Err(Failure::new(
        REFUSED,
        format!("置き場が {status} を返した: {method} {}", args.path),
    ))
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

/// `PAGETRAY_NO_DELETE` が付いていれば（空と `0` のほか）、消す命令（page delete・file delete・api の DELETE）は
/// 何もせずに止まる。消してよい人を呼ぶ側が決める時に使う（skills の tpm は、担当を起こす時にこれを付ける）。
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
