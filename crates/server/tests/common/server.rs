//! 受け入れの試験の道具: 本物の binary（pagetray-next）を空いた port で起こし、生の HTTP/1.1 で叩く。
//! 今の server の tests/server.rs の Server と、castlist の新しい server の tests/common/server.rs を移した物。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use super::{remove_db, temp_db};

/// 起こした pagetray-next。落とす時に止める。
pub struct Server {
    child: Child,
    pub port: u16,
    /// 一時の DB。落とす時に、folder ごと消す（`keep` なら残す）。
    pub db: PathBuf,
    /// 起こし直して前回の終わり方を見る試験は、DB の folder を残す。
    keep: bool,
}

/// 1 つの答え。
#[derive(Debug)]
pub struct Answer {
    pub status: u16,
    pub head: String,
    pub body: Vec<u8>,
}

impl Answer {
    /// header の値（名は大文字と小文字を問わずに引く）。
    pub fn header(&self, name: &str) -> Option<String> {
        self.head.lines().skip(1).find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim().eq_ignore_ascii_case(name)).then(|| value.trim().to_owned())
        })
    }

    /// 本文の字。
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// 本文の JSON（読めなければ Null）。
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null)
    }
}

/// 起こす binary。
pub fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_pagetray-next")
}

/// castlist の設定を外して、binary を起こす命令を組む（DB と待ち受ける所だけを渡す）。
pub fn command(db: &Path, listen: &str) -> Command {
    let mut command = Command::new(binary());
    command
        .env("PAGETRAY_DB", db)
        .env("PAGETRAY_LISTEN", listen)
        .env_remove("PAGETRAY_CASTLIST_URL")
        .env_remove("PAGETRAY_CASTLIST_LINK_URL");
    command
}

impl Server {
    /// 空いた port で起こし、受けられるようになるまで待つ。
    pub fn start() -> Self {
        let mut server = Self::start_at(&temp_db("server"));
        server.keep = false;
        server
    }

    /// この DB で起こす（同じ folder で起こし直せる）。落としても folder は消さない。stdout は DB の隣の file に受ける。
    pub fn start_at(db: &Path) -> Self {
        Self::start_with(db, &[])
    }

    /// 環境変数を足して、この DB で起こす。
    ///
    /// 空いた port は「取って、放して、渡す」ので、並べて流れる別の試験の server に先に取られることがある。
    /// つながるかでは見分けられない（つながった先が別の server かもしれない）。自分の stdout に、待ち受けた後の
    /// 行（listen=）が出るまで待ち、待ち受けられずに終わったら、印を戻して別の port でやり直す。
    pub fn start_with(db: &Path, env: &[(&str, &str)]) -> Self {
        let db = db.to_owned();
        let runs = db.with_file_name("logs").join("runs.log");
        let before = std::fs::read(&runs).ok();
        for _ in 0..20 {
            let port = TcpListener::bind("127.0.0.1:0")
                .and_then(|l| l.local_addr())
                .expect("空いた port")
                .port();
            let out = db.with_file_name(format!("stdout-{port}.txt"));
            let stdout = std::fs::File::create(&out).expect("stdout の file");
            let mut command = command(&db, &format!("127.0.0.1:{port}"));
            for (name, value) in env {
                command.env(name, value);
            }
            let mut child = command
                .stdout(Stdio::from(stdout))
                .stderr(Stdio::null())
                .spawn()
                .expect("pagetray-next を起こす");
            let started = Instant::now();
            let listening = loop {
                let text = std::fs::read_to_string(&out).unwrap_or_default();
                if text.contains(&format!(" listen=127.0.0.1:{port} ")) {
                    break true;
                }
                if child.try_wait().expect("try_wait").is_some() {
                    break false;
                }
                if started.elapsed() > Duration::from_secs(10) {
                    // 起こした物を残さない。
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("10 秒で受けられるようにならない: {text}");
                }
                std::thread::sleep(Duration::from_millis(20));
            };
            if listening {
                return Self {
                    child,
                    port,
                    db,
                    keep: true,
                };
            }
            // 取られた port で終わった回の印を消す（試験が見るのは、やり直しの前までの印）。
            match &before {
                Some(bytes) => std::fs::write(&runs, bytes).expect("印を戻す"),
                None => drop(std::fs::remove_file(&runs)),
            }
        }
        panic!("20 回やり直しても、空いた port で起きない");
    }

    /// つなぐ（読む待ちは 10 秒まで）。
    pub fn connect(&self) -> TcpStream {
        let stream = TcpStream::connect(("127.0.0.1", self.port)).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("timeout");
        stream
    }

    /// 生の要求を送り、答えを読む（要求は Connection: close にする）。
    pub fn send(&self, raw: &[u8]) -> Answer {
        let mut stream = self.connect();
        stream.write_all(raw).expect("write");
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).expect("read");
        let split = bytes
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .unwrap_or(bytes.len());
        let head = String::from_utf8_lossy(&bytes[..split]).into_owned();
        let mut body = bytes.get(split + 4..).unwrap_or_default().to_vec();
        if head
            .to_ascii_lowercase()
            .contains("\r\ntransfer-encoding: chunked")
        {
            body = dechunk(&body);
        }
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        Answer { status, head, body }
    }

    /// 生の要求（字）を送る。
    pub fn send_text(&self, raw: &str) -> Answer {
        self.send(raw.as_bytes())
    }

    pub fn get(&self, path: &str) -> Answer {
        self.send_text(&format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        ))
    }

    /// 本文を付けた要求。
    pub fn post(&self, path: &str, content_type: &str, body: &[u8]) -> Answer {
        let mut raw = format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        raw.extend_from_slice(body);
        self.send(&raw)
    }

    /// 止める合図（SIGTERM）を送る。
    pub fn terminate(&mut self) {
        let status = Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status()
            .expect("kill");
        assert!(status.success());
    }

    /// stdout に出た字。
    pub fn stdout(&self) -> String {
        std::fs::read_to_string(self.db.with_file_name(format!("stdout-{}.txt", self.port)))
            .unwrap_or_default()
    }

    /// 止める合図を送り、終わるのを待つ。終わりの code を返す。
    pub fn stop(&mut self) -> Option<i32> {
        self.terminate();
        self.child.wait().expect("wait").code()
    }

    /// 合図なしで切る（SIGKILL）。
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    pub fn try_wait(&mut self) -> Option<ExitStatus> {
        self.child.try_wait().expect("try_wait")
    }

    /// 終わるのを `limit` まで待つ。終われば、その終わり方。
    pub fn wait_for(&mut self, limit: Duration) -> Option<ExitStatus> {
        let started = Instant::now();
        loop {
            if let Some(status) = self.try_wait() {
                return Some(status);
            }
            if started.elapsed() > limit {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// chunked の本文をつなぐ（長さを前に言わない答え。backup の file など）。
fn dechunk(mut raw: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    loop {
        let Some(end) = raw.windows(2).position(|w| w == b"\r\n") else {
            return body;
        };
        let size = std::str::from_utf8(&raw[..end])
            .ok()
            .and_then(|s| usize::from_str_radix(s.split(';').next().unwrap_or("").trim(), 16).ok())
            .expect("chunk の長さ");
        if size == 0 {
            return body;
        }
        let start = end + 2;
        body.extend_from_slice(&raw[start..start + size]);
        raw = &raw[start + size + 2..];
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if !self.keep {
            remove_db(&self.db);
        }
    }
}
