//! binary を起こして、終わり方の印と記録の file を見る（決まりは docs/src/run.md の「落ちた後に調べる」）。
//! 試験ごとに空の folder を作り、その中に DB を置く。記録は DB の隣の logs/ にできるので、試験どうしで混ざらない。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// 試験ごとの空の folder。落とす時に消す。
struct Home(PathBuf);

impl Home {
    fn new(tag: &str) -> Self {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "pagetray-server-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn logs(&self) -> PathBuf {
        self.0.join("logs")
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.logs().join(name)).unwrap_or_default()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 起こした pagetray-server。stdout は file に受ける。
struct Server {
    child: Child,
    port: u16,
    stdout: PathBuf,
}

impl Server {
    /// この folder の DB で起こし、受けられるようになるまで待つ。
    fn start(home: &Home) -> Self {
        let port = TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .unwrap()
            .port();
        let stdout = home.0.join(format!("stdout-{port}.txt"));
        let mut child = spawn(&home.0.join("pages.db"), port, &stdout);
        let started = Instant::now();
        while TcpStream::connect(("127.0.0.1", port)).is_err() {
            if let Some(status) = child.try_wait().unwrap() {
                panic!("起きる前に終わった: {status}");
            }
            assert!(started.elapsed() < Duration::from_secs(10), "起きない");
            std::thread::sleep(Duration::from_millis(20));
        }
        Self {
            child,
            port,
            stdout,
        }
    }

    fn get(&self, path: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream
            .write_all(
                format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn stdout(&self) -> String {
        std::fs::read_to_string(&self.stdout).unwrap()
    }

    /// 止める合図（SIGTERM）を送り、終わるのを待つ。終わりの code を返す。
    fn terminate(mut self) -> Option<i32> {
        let pid = self.child.id().to_string();
        assert!(
            Command::new("kill")
                .args(["-TERM", &pid])
                .status()
                .unwrap()
                .success()
        );
        self.child.wait().unwrap().code()
    }

    /// 合図なしで切る（SIGKILL）。
    fn kill(mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }
}

fn spawn(db: &Path, port: u16, stdout: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_pagetray-server"))
        .env("PAGETRAY_LISTEN", format!("127.0.0.1:{port}"))
        .env("PAGETRAY_DB", db)
        .env_remove("PAGETRAY_CASTLIST_URL")
        .env_remove("PAGETRAY_CASTLIST_LINK_URL")
        .stdout(std::fs::File::create(stdout).unwrap())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn total(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.metadata().unwrap().len())
        .sum()
}

#[test]
fn 初めての起動は_前回が無いと出て_起きた行に時刻が付く() {
    let home = Home::new("first");
    let server = Server::start(&home);
    let out = server.stdout();
    assert!(out.contains("前回: 無し（初めての起動）"), "{out}");
    let line = out.lines().find(|l| l.contains(" listen=")).unwrap();
    assert!(
        line.starts_with("20") && line.contains("Z  INFO "),
        "{line}"
    );
    assert!(
        line.contains(&format!(
            "pagetray {} listen=127.0.0.1:{} db=",
            env!("CARGO_PKG_VERSION"),
            server.port
        )),
        "{line}"
    );
    assert_eq!(server.terminate(), Some(0));
}

#[test]
fn 止める合図で止めると_次の起動で_きれいに止まったと出る() {
    let home = Home::new("clean");
    assert_eq!(Server::start(&home).terminate(), Some(0));
    let runs = home.read("runs.log");
    let kinds: Vec<&str> = runs
        .lines()
        .map(|l| l.split_whitespace().nth(1).unwrap())
        .collect();
    assert_eq!(kinds, ["start", "signal", "stopped"], "{runs}");
    let server = Server::start(&home);
    assert!(
        server.stdout().contains("前回: きれいに止まった（"),
        "{}",
        server.stdout()
    );
    server.terminate();
}

#[test]
fn 合図なしで切られると_次の起動で_停止の印なしと最後に生きていた時刻が出る() {
    let home = Home::new("killed");
    Server::start(&home).kill();
    let server = Server::start(&home);
    let out = server.stdout();
    let line = out.lines().find(|l| l.contains("前回: ")).unwrap();
    assert!(line.contains("前回: 停止の印なし（"), "{line}");
    assert!(line.contains("最後に生きていた時刻 20"), "{line}");
    server.terminate();
}

#[test]
fn db_を開けずに終わると_次の起動で_誤りで終わったと訳が出る() {
    let home = Home::new("failed");
    // DB の名の所に folder があると、開けない。
    std::fs::create_dir_all(home.0.join("pages.db")).unwrap();
    let out = home.0.join("stdout-failed.txt");
    let status = spawn(&home.0.join("pages.db"), 1, &out).wait().unwrap();
    assert_eq!(status.code(), Some(1));
    std::fs::remove_dir_all(home.0.join("pages.db")).unwrap();
    let server = Server::start(&home);
    assert!(
        server.stdout().contains("前回: 誤りで終わった（"),
        "{}",
        server.stdout()
    );
    assert!(home.read("runs.log").contains(" failed "));
    server.terminate();
}

#[test]
fn 問い合わせの行は記録の_file_に時刻つきで残り_stdout_には出ない() {
    let home = Home::new("requests");
    let server = Server::start(&home);
    assert!(server.get("/api/version").contains("200 OK"));
    assert!(server.get("/no/such/page.html").contains("404"));
    let log = home.read("server.log");
    let line = log
        .lines()
        .find(|l| l.contains("uri=/api/version"))
        .unwrap_or_else(|| panic!("{log}"));
    assert!(
        line.starts_with("20") && line.contains("status=200"),
        "{line}"
    );
    assert!(log.contains("uri=/no/such/page.html") && log.contains("status=404"));
    // 起きた行は、記録の file と stdout の両方にある。
    assert!(log.contains(" listen=127.0.0.1:"));
    let out = server.stdout();
    assert!(out.contains(" listen=127.0.0.1:"));
    assert!(!out.contains("uri=/api/version"), "{out}");
    // 読む道は、印と記録を返す。
    let logs = server.get("/api/logs");
    assert!(logs.contains("200 OK") && logs.contains("text/plain"));
    assert!(logs.contains(" start version=") && logs.contains("uri=/api/version"));
    server.terminate();
}

#[test]
fn 多く問い合わせても_logs_の合わせた大きさは上限の内() {
    let home = Home::new("cap");
    let server = Server::start(&home);
    // 1 行 約 200 byte × 7,000 = 約 1.4 MiB。file 1 つの上限（1 MiB）を超えて回る。
    let long = "x".repeat(60);
    for n in 0..7000 {
        assert!(server.get(&format!("/p/k/{n}-{long}.html")).contains("404"));
    }
    let limit = pagetray::runlog::LOG_FILE_MAX * u64::from(pagetray::runlog::LOG_FILES)
        + pagetray::runlog::RUNS_MAX
        + 64;
    let size = total(&home.logs());
    assert!(size <= limit, "{size}");
    let rotated = std::fs::metadata(home.logs().join("server.log.1"))
        .unwrap()
        .len();
    assert!(rotated <= pagetray::runlog::LOG_FILE_MAX, "{rotated}");
    assert!(rotated > pagetray::runlog::LOG_FILE_MAX - 1024, "{rotated}");
    server.terminate();
}

#[test]
fn 並べて起こした_2_つの_server_の記録は混ざらない() {
    let (a, b) = (Home::new("a"), Home::new("b"));
    let (sa, sb) = (Server::start(&a), Server::start(&b));
    assert!(sa.get("/only-a").contains("404"));
    assert!(sb.get("/only-b").contains("404"));
    assert!(a.read("server.log").contains("uri=/only-a"));
    assert!(!a.read("server.log").contains("uri=/only-b"));
    assert!(!b.read("server.log").contains("uri=/only-a"));
    assert_eq!(a.read("runs.log").lines().count(), 1);
    sa.terminate();
    sb.terminate();
}
