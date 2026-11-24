//! 不正終了の原因を後から調べるための記録（今の server の src/runlog.rs を写す。この後の commit で入れる）。
use std::path::{Path, PathBuf};

/// 記録の file 1 つの上限。
pub const LOG_FILE_MAX: u64 = 1024 * 1024;
/// 記録の file の数（今の物と、回した物）。
pub const LOG_FILES: u32 = 4;
/// 終わり方の印の file の上限。
pub const RUNS_MAX: u64 = 64 * 1024;

/// 記録の置き場。写して持ち回れる。
#[derive(Clone)]
pub struct RunLog {
    dir: PathBuf,
}

impl RunLog {
    /// DB の file がある folder の下の `logs/` を置き場にする。
    pub fn beside(db: &Path) -> Self {
        Self::at(db.parent().unwrap_or(Path::new(".")).join("logs"))
    }

    /// この folder を置き場にする。
    pub fn at(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// 起きた印（まだ書かない）。
    pub fn start(&self, _version: &str) {}
}
