//! 不正終了の原因を後から調べるための記録。DB の file がある folder の下の `logs/` に置く（本番では `/data/logs/`）。
//!
//! - `server.log`・`server.log.1`〜`.3`: 時刻つきの行。1 つ [`LOG_FILE_MAX`]、合わせて [`LOG_FILES`] つまで。
//! - `runs.log`: 終わり方の印（起動・合図・停止・panic・誤り）。[`RUNS_MAX`] まで。
//! - `alive`: 生きている印の時刻。[`ALIVE_EVERY`] ごとに上書きする。
//!
//! 止める合図も panic の hook も来ない終わり方（SIGKILL・メモリ不足・VM ごと切られた）では何も書けない。
//! だから、次に起きた時に「停止の行が無い」ことで見つける（[`RunLog::previous`]）。
//!
//! **ここだけは、tokio の糸を塞ぐ同期の書き方をする。** 行を裏の糸に渡して書く形（tracing-appender の
//! non_blocking）は、`panic = "abort"` では渡したままの行を落とす。知りたいのは落ちる直前の行なので、
//! 1 行ごとに、間に buffer を挟まず write する。1 行の write は短い。
//! 記録のせいで道具を落とさない: 書けない時は行を捨て、stderr には変わり目に 1 行だけ出す。

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, TryLockError};
use std::time::Duration;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::{FormatTime, SystemTime};

/// 記録の file 1 つの上限。
pub const LOG_FILE_MAX: u64 = 1024 * 1024;
/// 記録の file の数（今の物と、回した物）。
pub const LOG_FILES: u32 = 4;
/// 終わり方の印の file の上限。
pub const RUNS_MAX: u64 = 64 * 1024;
/// 終わり方の印の 1 行の上限（panic の文は切り詰める）。
pub const RUN_LINE_MAX: usize = 1024;
/// 生きている印を刻む間。落ちた時刻は、この幅で分かる。
pub const ALIVE_EVERY: Duration = Duration::from_secs(30);

const LOG: &str = "server.log";
const RUNS: &str = "runs.log";
const ALIVE: &str = "alive";

/// 前の回の終わり方。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Previous {
    /// 印が無い（初めて起きた）。
    First,
    /// 止める合図を受け、閉じ終えた。
    Clean { line: String },
    /// 止める合図は受けたが、閉じ終える前に切られた。
    Signalled { line: String },
    /// panic で落ちた。
    Panicked { line: String },
    /// 誤りで終わった（DB を開けない・待ち受けられない、など）。
    Failed { line: String },
    /// 停止の行が無い（SIGKILL・メモリ不足・VM ごと切られた）。`alive` は最後に生きていた時刻。
    Unmarked { started: String, alive: String },
}

impl std::fmt::Display for Previous {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::First => write!(f, "previous: none (first start)"),
            Self::Clean { line } => write!(f, "previous: stopped cleanly ({line})"),
            Self::Signalled { line } => {
                write!(f, "previous: got a stop signal but did not finish ({line})")
            }
            Self::Panicked { line } => write!(f, "previous: panicked ({line})"),
            Self::Failed { line } => write!(f, "previous: failed ({line})"),
            Self::Unmarked { started, alive } => {
                write!(f, "previous: no stop mark ({started}; last alive {alive})")
            }
        }
    }
}

/// 今の時刻（UTC・RFC 3339）。記録の行の頭と同じ形。
pub fn now() -> String {
    let mut text = String::new();
    let _ = SystemTime.format_time(&mut Writer::new(&mut text));
    text
}

/// 記録の置き場。写して持ち回れる。
#[derive(Clone)]
pub struct RunLog {
    dir: PathBuf,
    log: Arc<Mutex<Capped>>,
}

impl RunLog {
    /// DB の file がある folder の下の `logs/` を置き場にする。作れなくても誤りにしない（file なしで動く）。
    pub fn beside(db: &Path) -> Self {
        Self::at(db.parent().unwrap_or(Path::new(".")).join("logs"))
    }

    /// この folder を置き場にする。
    pub fn at(dir: PathBuf) -> Self {
        let log = Arc::new(Mutex::new(Capped::new(&dir, LOG_FILE_MAX, LOG_FILES)));
        Self { dir, log }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// tracing の書き先（記録の file）。
    pub fn writer(&self) -> LogWriter {
        LogWriter(self.log.clone())
    }

    /// 前の回の終わり方を、印から読む。[`RunLog::start`] の前に呼ぶ。
    pub fn previous(&self) -> Previous {
        let runs = fs::read(self.dir.join(RUNS)).unwrap_or_default();
        let runs = String::from_utf8_lossy(&runs);
        let lines: Vec<&str> = runs.lines().filter(|l| !l.trim().is_empty()).collect();
        let Some(start) = lines.iter().rposition(|l| kind(l) == "start") else {
            return Previous::First;
        };
        // 起きた後の、最後の印で決める（debug の build では、問い合わせの中の panic の後も動き続ける。
        // その後にきれいに止まったなら、きれいに止まった）。
        let last = lines[start + 1..]
            .iter()
            .rfind(|l| matches!(kind(l), "panic" | "failed" | "stopped" | "signal"));
        if let Some(line) = last {
            let line = (*line).to_owned();
            match kind(&line) {
                "panic" => Previous::Panicked { line },
                "failed" => Previous::Failed { line },
                "stopped" => Previous::Clean { line },
                _ => Previous::Signalled { line },
            }
        } else {
            let alive = fs::read(self.dir.join(ALIVE)).unwrap_or_default();
            let alive = String::from_utf8_lossy(&alive).trim().to_owned();
            Previous::Unmarked {
                started: lines[start].to_owned(),
                alive: if alive.is_empty() {
                    "unknown".to_owned()
                } else {
                    alive
                },
            }
        }
    }

    /// 起きた印。
    pub fn start(&self, version: &str) {
        self.mark(&format!("start version={version}"));
        self.beat();
    }

    /// 止める合図を受けた印。
    pub fn signal(&self, name: &str) {
        self.mark(&format!("signal {name}"));
    }

    /// 閉じ終えた印。
    pub fn stopped(&self) {
        self.mark("stopped");
    }

    /// 誤りで終わる印。
    pub fn failed(&self, why: &str) {
        self.mark(&format!("failed {}", one_line(why)));
    }

    /// 生きている印を刻む（別の名に書いてから入れ替える。書きかけで止められても、前の時刻が残る）。
    pub fn beat(&self) {
        let tmp = self.dir.join(format!("{ALIVE}.tmp"));
        if write_new(&tmp, now().as_bytes()).is_ok() {
            let _ = fs::rename(&tmp, self.dir.join(ALIVE));
        }
    }

    /// panic の印と、記録の file の ERROR の行。panic の hook から呼ぶ。
    ///
    /// 記録の書き手の lock を待たない: 書き手の中で panic した時に、同じ糸が同じ lock を取りに行くと固まる。
    /// 印は lock を通さずに書き、記録の file へは `try_lock` で、取れなければ書かない。
    pub fn panicked(&self, location: &str, message: &str) {
        let message = one_line(message);
        self.mark(&format!("panic at={location} msg={message}"));
        // 取れないのは、今だれかが持っている時だけ。毒が回っているだけなら、書く。
        let log = match self.log.try_lock() {
            Ok(log) => Some(log),
            Err(TryLockError::Poisoned(e)) => Some(e.into_inner()),
            Err(TryLockError::WouldBlock) => None,
        };
        if let Some(mut log) = log {
            log.line(format!("{} ERROR panic at={location} msg={message}\n", now()).as_bytes());
        }
    }

    /// panic の時に [`RunLog::panicked`] を呼ぶ hook を置く。元の hook（stderr に出す）も続けて呼ぶ。
    pub fn hook_panics(&self) {
        let log = self.clone();
        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let location = info
                .location()
                .map(|l| format!("{}:{}", l.file(), l.line()))
                .unwrap_or_default();
            let payload = info.payload();
            let message = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_default();
            log.panicked(&location, &message);
            before(info);
        }));
    }

    /// 印と記録を、古い順につないだ字（`GET /api/logs` の答え）。多くて [`RUNS_MAX`] + [`LOG_FILE_MAX`] × [`LOG_FILES`]。
    pub fn read_all(&self) -> Vec<u8> {
        // 読む間は書き手の lock を持つ（途中で回ると file の名がずれ、1 つぶんの行が抜ける）。
        let _held = self.log.lock().unwrap_or_else(PoisonError::into_inner);
        let mut all = Vec::new();
        all.extend_from_slice(format!("== {RUNS}\n").as_bytes());
        all.extend(fs::read(self.dir.join(RUNS)).unwrap_or_default());
        all.extend_from_slice(format!("== {LOG} (oldest first)\n").as_bytes());
        for n in (0..LOG_FILES).rev() {
            all.extend(fs::read(self.dir.join(rotated(n))).unwrap_or_default());
        }
        all
    }

    /// 印を 1 行足す。足す前に大きさを見て、上限を超えるなら古い半分を落とす。書けなければ黙って捨てる。
    fn mark(&self, text: &str) {
        let line = format!("{} {}\n", now(), clip(text, RUN_LINE_MAX - 40));
        let path = self.dir.join(RUNS);
        let old = fs::read(&path).unwrap_or_default();
        let _ = fs::create_dir_all(&self.dir);
        if old.len() + line.len() + 1 > RUNS_MAX as usize {
            // 新しい半分を、行の頭から残す。別の名に書いてから入れ替える。
            let from = old.len().saturating_sub(RUNS_MAX as usize / 2);
            let from = old[from..]
                .iter()
                .position(|b| *b == b'\n')
                .map_or(old.len(), |i| from + i + 1);
            let mut kept = old[from..].to_vec();
            // 前の回が行の途中で切れていたら、行を分ける。
            if kept.last().is_some_and(|last| *last != b'\n') {
                kept.push(b'\n');
            }
            kept.extend_from_slice(line.as_bytes());
            let tmp = self.dir.join(format!("{RUNS}.tmp"));
            if write_new(&tmp, &kept).is_ok() {
                let _ = fs::rename(&tmp, &path);
            }
            return;
        }
        // 前の回が行の途中で切れていたら、行を分ける。
        let line = match old.last() {
            Some(last) if *last != b'\n' => format!("\n{line}"),
            _ => line,
        };
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = file.write_all(line.as_bytes());
        }
    }
}

/// 印の行の種類（時刻の次の語）。
fn kind(line: &str) -> &str {
    line.split_whitespace().nth(1).unwrap_or("")
}

/// 改行を空白にする（印は 1 件 1 行）。
fn one_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}

/// 字の境で、`max` byte までに切り詰める。
fn clip(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    file.write_all(bytes)
}

/// 回した file の名。0 が今の物。
fn rotated(n: u32) -> String {
    if n == 0 {
        LOG.to_owned()
    } else {
        format!("{LOG}.{n}")
    }
}

/// 大きさで回す書き手。`dir` の中の記録の file は、合わせて `max` × `keep` を超えない。行の途中では切らない。
struct Capped {
    dir: PathBuf,
    max: u64,
    keep: u32,
    /// 開いている今の file と、その大きさ。開けていなければ None。
    file: Option<(File, u64)>,
    /// 書けない間は true（stderr に出すのは、変わり目だけ）。
    failing: bool,
}

impl Capped {
    fn new(dir: &Path, max: u64, keep: u32) -> Self {
        Self {
            dir: dir.to_owned(),
            max,
            keep,
            file: None,
            failing: false,
        }
    }

    /// 1 行（1 件）を足す。書けなければ捨てる。
    fn line(&mut self, bytes: &[u8]) {
        let result = self.append(bytes);
        match (&result, self.failing) {
            (Err(e), false) => {
                self.failing = true;
                self.file = None;
                eprintln!(
                    "log files are not writable, dropping lines: {}: {e}",
                    self.dir.display()
                );
            }
            (Ok(()), true) => {
                self.failing = false;
                eprintln!("log files are writable again: {}", self.dir.display());
            }
            (Err(_), true) => self.file = None,
            (Ok(()), false) => {}
        }
    }

    fn append(&mut self, bytes: &[u8]) -> io::Result<()> {
        // 上限より長い 1 行は切り詰め、行の終わりを付け直す。
        let max = self.max as usize;
        let clipped;
        let bytes = if bytes.len() > max {
            // 字（UTF-8）の途中では切らない: 続きの byte（10xxxxxx）の前まで戻る。
            let mut end = max - 1;
            while end > 0 && bytes[end] & 0xC0 == 0x80 {
                end -= 1;
            }
            let mut head = bytes[..end].to_vec();
            head.push(b'\n');
            clipped = head;
            &clipped[..]
        } else {
            bytes
        };
        if self.file.is_none() {
            fs::create_dir_all(&self.dir)?;
            let mut file = OpenOptions::new()
                .create(true)
                .read(true)
                .append(true)
                .open(self.dir.join(LOG))?;
            // 起こし直した時は、今ある大きさから数える。
            let mut len = file.metadata()?.len();
            // 前の行が途中で切れていたら（書きかけで切られた・途中まで書いて誤りになった）、行を分ける。
            if len > 0 {
                let mut last = [0u8; 1];
                file.seek(SeekFrom::End(-1))?;
                file.read_exact(&mut last)?;
                if last[0] != b'\n' {
                    file.write_all(b"\n")?;
                    len += 1;
                }
            }
            self.file = Some((file, len));
        }
        if self
            .file
            .as_ref()
            .is_some_and(|(_, len)| len + bytes.len() as u64 > self.max)
        {
            self.rotate()?;
        }
        let (file, len) = self.file.as_mut().expect("上で開いた");
        file.write_all(bytes)?;
        *len += bytes.len() as u64;
        Ok(())
    }

    /// 今の file を `.1` に、`.1` を `.2` に…と送り、一番古い物を消す。
    fn rotate(&mut self) -> io::Result<()> {
        self.file = None;
        for n in (1..self.keep).rev() {
            let from = self.dir.join(rotated(n - 1));
            if from.exists() {
                fs::rename(from, self.dir.join(rotated(n)))?;
            }
        }
        if self.keep == 1 {
            fs::remove_file(self.dir.join(LOG))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join(LOG))?;
        self.file = Some((file, 0));
        Ok(())
    }
}

/// tracing の書き先。1 件ごとに lock を取り、1 行を足す。
#[derive(Clone)]
pub struct LogWriter(Arc<Mutex<Capped>>);

/// 1 件ぶんの書き手。
pub struct LogLine<'a>(MutexGuard<'a, Capped>);

impl<'a> MakeWriter<'a> for LogWriter {
    type Writer = LogLine<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        // 試験の中の panic で毒が回っても、書き続ける。
        LogLine(self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl Write for LogLine<'_> {
    /// tracing は 1 件を 1 度の write で渡す。失敗は呼び手に返さない（返すと tracing が stderr に毎回出す）。
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.line(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// 記録の出し先を決める。1 度だけ呼ぶ。
///
/// - 記録の file: INFO 以上（sqlx は WARN 以上）。問い合わせごとの行もここに書く。
/// - stdout: 同じ行のうち、問い合わせごとの行（tower_http）を除いた物。container の log には上限が無いので、
///   量を増やさない。
pub fn init(log: &RunLog) {
    use tracing::level_filters::LevelFilter;
    use tracing_subscriber::Layer;
    use tracing_subscriber::filter::Targets;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let targets = Targets::new()
        .with_default(LevelFilter::INFO)
        .with_target("sqlx", LevelFilter::WARN);
    let to_file = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(log.writer())
        .with_filter(targets.clone());
    let to_stdout = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(io::stdout)
        .with_filter(targets.with_target("tower_http", LevelFilter::OFF));
    tracing_subscriber::registry()
        .with(to_file)
        .with(to_stdout)
        .init();
}

/// [`ALIVE_EVERY`] ごとに、生きている印を刻み続ける。
pub async fn keep_alive(log: RunLog) {
    loop {
        tokio::time::sleep(ALIVE_EVERY).await;
        log.beat();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// 試験ごとの空の folder。
    fn dir(tag: &str) -> PathBuf {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "pagetray-runlog-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn total(dir: &Path) -> u64 {
        fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.metadata().map_or(0, |m| m.len()))
                    .sum()
            })
            .unwrap_or(0)
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn 上限の_8_倍を書いても_記録の_file_は合わせて上限を超えず_行の途中で切れない() {
        let dir = dir("cap");
        let mut log = Capped::new(&dir, 4096, 4);
        let mut written = 0;
        let mut n = 0;
        while written < 4096 * 4 * 8 {
            let line = format!("line {n} {}\n", "x".repeat(n % 97));
            log.line(line.as_bytes());
            written += line.len();
            n += 1;
            assert!(total(&dir) <= 4096 * 4, "{n} 行目で {}", total(&dir));
        }
        assert_eq!(
            names(&dir),
            ["server.log", "server.log.1", "server.log.2", "server.log.3"]
        );
        for name in names(&dir) {
            let bytes = fs::read(dir.join(&name)).unwrap();
            assert!(bytes.len() <= 4096, "{name}");
            assert_eq!(bytes.last(), Some(&b'\n'), "{name} は行の終わりで切れる");
            assert!(bytes.starts_with(b"line "), "{name} は行の頭から始まる");
        }
        // 一番新しい行は今の file に、古い物から消える。
        let newest = fs::read_to_string(dir.join("server.log")).unwrap();
        assert!(newest.contains(&format!("line {} ", n - 1)));
        let oldest = fs::read_to_string(dir.join("server.log.3")).unwrap();
        assert!(!oldest.contains("line 0 "));
    }

    #[test]
    fn 本物の上限のまま_上限の_5_倍を_tracing_の書き先に書いても_合わせて_4_mib_を超えない() {
        let log = RunLog::at(dir("real-cap"));
        let writer = log.writer();
        let line = format!("{}\n", "r".repeat(199));
        let mut written = 0;
        while written < LOG_FILE_MAX * u64::from(LOG_FILES) * 5 {
            writer.make_writer().write_all(line.as_bytes()).unwrap();
            written += line.len() as u64;
        }
        log.start("1.2.3");
        assert_eq!(
            names(log.dir()),
            [
                "alive",
                "runs.log",
                "server.log",
                "server.log.1",
                "server.log.2",
                "server.log.3"
            ]
        );
        for n in 0..LOG_FILES {
            let len = fs::metadata(log.dir().join(rotated(n))).unwrap().len();
            assert!(len <= LOG_FILE_MAX, "{n}: {len}");
        }
        let all = total(log.dir());
        assert!(
            all <= LOG_FILE_MAX * u64::from(LOG_FILES) + RUNS_MAX,
            "{all}"
        );
        assert!(
            all > LOG_FILE_MAX * u64::from(LOG_FILES - 1),
            "回した file が残る: {all}"
        );
        // 読む道の字も、上限の内。
        assert!(log.read_all().len() as u64 <= LOG_FILE_MAX * u64::from(LOG_FILES) + RUNS_MAX + 64);
    }

    #[test]
    fn 長い_1_行は_字の途中で切らない() {
        let dir = dir("long-utf8");
        let mut log = Capped::new(&dir, 1000, 2);
        log.line(format!("{}\n", "あ".repeat(2000)).as_bytes());
        let bytes = fs::read(dir.join("server.log")).unwrap();
        assert!(bytes.len() <= 1000 && bytes.len() > 990, "{}", bytes.len());
        let text = String::from_utf8(bytes).expect("UTF-8 のまま");
        assert!(text.ends_with("あ\n"));
    }

    #[test]
    fn 開き直した時に前の行が途中で切れていたら_行を分ける() {
        let dir = dir("torn-log");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("server.log"), b"whole\ntor").unwrap();
        let mut log = Capped::new(&dir, 1000, 2);
        log.line(b"next\n");
        assert_eq!(
            fs::read_to_string(dir.join("server.log")).unwrap(),
            "whole\ntor\nnext\n"
        );
        assert!(total(&dir) <= 2000);
    }

    #[test]
    fn panic_の後もきれいに止まれば_きれいに止まった() {
        // debug の build では、問い合わせの中の panic の後も動き続ける。
        let log = RunLog::at(dir("panic-then-clean"));
        log.start("1.2.3");
        log.panicked("src/x.rs:7", "途中");
        log.signal("SIGTERM");
        log.stopped();
        assert!(matches!(log.previous(), Previous::Clean { .. }));
    }

    #[test]
    fn 毒の回った_lock_でも_panic_の行を記録に書く() {
        let log = RunLog::at(dir("poisoned"));
        let poison = log.clone();
        let _ = std::thread::spawn(move || {
            let _held = poison.log.lock().unwrap();
            std::panic::resume_unwind(Box::new("毒"));
        })
        .join();
        assert!(log.log.is_poisoned());
        log.panicked("src/z.rs:3", "毒の後");
        let server = fs::read_to_string(log.dir().join("server.log")).unwrap();
        assert!(server.contains(" ERROR panic at=src/z.rs:3"), "{server}");
    }

    #[test]
    fn 印を削る時も_切れた最後の行の後で改行する() {
        let log = RunLog::at(dir("torn-trim"));
        // 上限のすぐ手前まで埋め、最後の行を途中で切る。
        while fs::metadata(log.dir().join("runs.log")).map_or(0, |m| m.len()) < RUNS_MAX - 600 {
            log.failed(&"v".repeat(400));
        }
        let path = log.dir().join("runs.log");
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(format!("2026-01-01T00:00:00.000000Z sig{}", "n".repeat(500)).as_bytes())
            .unwrap();
        log.start("1.2.4");
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            (text.len() as u64) < RUNS_MAX / 2 + 2048,
            "削った: {}",
            text.len()
        );
        let last = text.lines().last().unwrap();
        assert!(
            last.starts_with("20") && last.ends_with(" start version=1.2.4"),
            "{last}"
        );
        assert!(matches!(log.previous(), Previous::Unmarked { .. }));
    }

    #[test]
    fn 上限より長い_1_行は切り詰める() {
        let dir = dir("long");
        let mut log = Capped::new(&dir, 1000, 2);
        log.line(format!("{}\n", "y".repeat(5000)).as_bytes());
        let bytes = fs::read(dir.join("server.log")).unwrap();
        assert_eq!(bytes.len(), 1000);
        assert_eq!(bytes.last(), Some(&b'\n'));
        assert!(total(&dir) <= 2000);
    }

    #[test]
    fn 起こし直した時は_今ある大きさから数えて回す() {
        let dir = dir("reopen");
        // 起動のたびに 900 byte ずつ書く。数え直さないと、1 つの file が上限を超える。
        for run in 0..12 {
            let mut log = Capped::new(&dir, 1000, 3);
            log.line(format!("run {run} {}\n", "z".repeat(890)).as_bytes());
            for name in names(&dir) {
                let len = fs::metadata(dir.join(&name)).unwrap().len();
                assert!(len <= 1000, "{run} 回目の {name} が {len}");
            }
            assert!(total(&dir) <= 3000);
        }
        assert_eq!(names(&dir).len(), 3);
    }

    #[test]
    fn 書けない間は行を捨て_書けるようになったら続ける() {
        let dir = dir("broken");
        // 置き場の名の所に file があると、folder を作れない。
        fs::write(&dir, b"").unwrap();
        let mut log = Capped::new(&dir, 1000, 2);
        log.line(b"lost 1\n");
        log.line(b"lost 2\n");
        assert!(log.failing);
        fs::remove_file(&dir).unwrap();
        log.line(b"kept\n");
        assert!(!log.failing);
        assert_eq!(
            fs::read_to_string(dir.join("server.log")).unwrap(),
            "kept\n"
        );
    }

    #[test]
    fn 初めての起動は_前回が無い() {
        let log = RunLog::at(dir("first"));
        assert_eq!(log.previous(), Previous::First);
    }

    #[test]
    fn 合図を受けて閉じ終えると_きれいに止まった() {
        let log = RunLog::at(dir("clean"));
        log.start("1.2.3");
        log.signal("SIGTERM");
        log.stopped();
        let Previous::Clean { line } = log.previous() else {
            panic!("{:?}", log.previous());
        };
        assert!(line.ends_with(" stopped"), "{line}");
        assert!(line.starts_with("20"), "頭は時刻: {line}");
    }

    #[test]
    fn 合図だけで終わると_終わり切らなかった() {
        let log = RunLog::at(dir("signalled"));
        log.start("1.2.3");
        log.signal("SIGTERM");
        let Previous::Signalled { line } = log.previous() else {
            panic!("{:?}", log.previous());
        };
        assert!(line.ends_with(" signal SIGTERM"), "{line}");
    }

    #[test]
    fn 停止の行が無いと_印なしと最後に生きていた時刻() {
        let log = RunLog::at(dir("unmarked"));
        // 前の回はきれいに止まり、今の回が印なしで終わった。
        log.start("1.2.2");
        log.signal("SIGTERM");
        log.stopped();
        log.start("1.2.3");
        let Previous::Unmarked { started, alive } = log.previous() else {
            panic!("{:?}", log.previous());
        };
        assert!(started.ends_with(" start version=1.2.3"), "{started}");
        assert!(alive.starts_with("20") && alive.ends_with('Z'), "{alive}");
    }

    #[test]
    fn 誤りで終わると_訳が残る() {
        let log = RunLog::at(dir("failed"));
        log.start("1.2.3");
        log.failed("DB を開けない:\n壊れている");
        let Previous::Failed { line } = log.previous() else {
            panic!("{:?}", log.previous());
        };
        assert!(
            line.ends_with(" failed DB を開けない: 壊れている"),
            "{line}"
        );
    }

    #[test]
    fn panic_の印に場所と文が残り_長い文は切り詰める() {
        let log = RunLog::at(dir("panic"));
        log.start("1.2.3");
        log.panicked("src/x.rs:7", &"あ".repeat(4000));
        let Previous::Panicked { line } = log.previous() else {
            panic!("{:?}", log.previous());
        };
        assert!(
            line.contains(" panic at=src/x.rs:7 msg=あ"),
            "{}",
            &line[..80]
        );
        assert!(line.len() < RUN_LINE_MAX, "{}", line.len());
        // 記録の file にも ERROR の行が残る。
        let server = fs::read_to_string(log.dir().join("server.log")).unwrap();
        assert!(server.contains(" ERROR panic at=src/x.rs:7"));
    }

    #[test]
    fn 書き手の_lock_を持ったままの_panic_でも固まらず_印は残る() {
        let log = RunLog::at(dir("locked"));
        log.start("1.2.3");
        let held = log.log.lock().unwrap();
        log.panicked("src/y.rs:1", "書き手の中");
        drop(held);
        assert!(matches!(log.previous(), Previous::Panicked { .. }));
        assert!(!log.dir().join("server.log").exists());
    }

    #[test]
    fn hook_は_panic_の場所と文を印に書く() {
        let log = RunLog::at(dir("hook"));
        log.start("1.2.3");
        log.hook_panics();
        let caught = std::panic::catch_unwind(|| panic!("hook の試験 {}", 42));
        let _ = std::panic::take_hook();
        assert!(caught.is_err());
        // hook は process 全体の物なので、並べて流れるほかの試験の panic の印も入りうる。自分の文の行を探す。
        let runs = fs::read_to_string(log.dir().join("runs.log")).unwrap();
        let line = runs
            .lines()
            .find(|l| l.ends_with("msg=hook の試験 42"))
            .unwrap_or_else(|| panic!("{runs}"));
        assert!(
            line.contains(" panic at=crates/server/src/runlog.rs:"),
            "{line}"
        );
    }

    #[test]
    fn 最後の行が途中で切れていても_次の印は別の行になる() {
        let log = RunLog::at(dir("torn"));
        log.start("1.2.3");
        let path = log.dir().join("runs.log");
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"2026-01-01T00:00:00.000000Z sig").unwrap();
        assert!(matches!(log.previous(), Previous::Unmarked { .. }));
        log.start("1.2.4");
        log.signal("SIGTERM");
        log.stopped();
        assert!(matches!(log.previous(), Previous::Clean { .. }));
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(" sig\n20"), "切れた行の後で改行する: {text}");
    }

    #[test]
    fn 印の_file_は上限を超えず_新しい行が残る() {
        let log = RunLog::at(dir("runs-cap"));
        for n in 0..2000 {
            log.failed(&format!("{n} {}", "w".repeat(200)));
            let len = fs::metadata(log.dir().join("runs.log")).unwrap().len();
            assert!(len <= RUNS_MAX, "{n} 回目で {len}");
        }
        let text = fs::read_to_string(log.dir().join("runs.log")).unwrap();
        assert!(text.contains(" failed 1999 "));
        assert!(!text.contains(" failed 0 "));
        assert!(text.starts_with("20"), "行の頭から残す");
        assert!(!log.dir().join("runs.log.tmp").exists());
    }

    #[test]
    fn 生きている印は上書きで_増えない() {
        let log = RunLog::at(dir("alive"));
        fs::create_dir_all(log.dir()).unwrap();
        for _ in 0..50 {
            log.beat();
        }
        let alive = fs::read_to_string(log.dir().join("alive")).unwrap();
        assert!(alive.len() < 40 && alive.ends_with('Z'), "{alive}");
        assert_eq!(names(log.dir()), ["alive"]);
    }

    #[test]
    fn 置き場を作れなくても_印と記録は誤りにしない() {
        let dir = dir("nodir");
        fs::write(&dir, b"").unwrap();
        let log = RunLog::at(dir.clone());
        log.start("1.2.3");
        log.signal("SIGTERM");
        log.stopped();
        log.panicked("a:1", "b");
        assert_eq!(log.previous(), Previous::First);
        assert!(log.read_all().starts_with("== runs.log".as_bytes()));
        fs::remove_file(&dir).unwrap();
    }

    #[test]
    fn 読む道の字は_印と_古い順の記録() {
        let log = RunLog::at(dir("read"));
        log.start("1.2.3");
        {
            let mut capped = log.log.lock().unwrap();
            capped.max = 20;
            capped.line(b"first line here\n");
            capped.line(b"second line here\n");
        }
        let text = String::from_utf8(log.read_all()).unwrap();
        let runs = text.find("start version=1.2.3").unwrap();
        let first = text.find("first line here").unwrap();
        let second = text.find("second line here").unwrap();
        assert!(runs < first && first < second, "{text}");
    }
}
