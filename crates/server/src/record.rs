//! DB のページの行（[`crate::db::PageRow`]）を、答えのページの記録にする（ListPages の行と、SetReplyState・AddReviewMarks の答えで同じ形）。
//! 返事・判定・見た物・送り手の欄は、ある時だけ（Product Manager の待ちに `waitsOn` は無い）。
//! file の行（[`crate::db::FileRow`]）も同じく、ListFiles の行と CreateFile の答えで同じ形にする（[`FileRecord`]）。
use aws_smithy_types::DateTime;
use pagetray_sdk::model::{ActorSummary, File, Page, Sender};
use pagetray_sdk::output::{AddReviewMarksOutput, CreateFileOutput, SetReplyStateOutput};

use crate::datetime;
use crate::db::{FileRow, PageRow};
use crate::files::file_path;
use crate::pages::page_path;

/// 送り手の列（senders を left join した物）を、答えの送り手の欄にする。付随する id が無ければ無い。
/// castlist の答え（アクターと答えた時刻）は、ある時だけ。
fn sender(
    issuer: Option<String>,
    r#ref: Option<String>,
    actor: (Option<String>, Option<String>, Option<String>),
    resolved_at: Option<&str>,
) -> Result<Option<Sender>, String> {
    let (Some(issuer), Some(r#ref)) = (issuer, r#ref) else {
        return Ok(None);
    };
    let actor = match actor {
        (Some(id), Some(name), Some(kind)) => Some(ActorSummary { id, name, kind }),
        _ => None,
    };
    Ok(Some(Sender {
        issuer,
        r#ref,
        resolved_at: actor.as_ref().and(resolved_at.map(datetime).transpose()?),
        actor,
    }))
}

/// 答えのページの記録の欄。直せなければ（DB の時刻が読めない）、その訳の字。
#[derive(Clone)]
pub struct Record {
    project: String,
    key: String,
    kind: String,
    n: String,
    title: String,
    role: String,
    workspace: String,
    created_at: DateTime,
    size: i64,
    path: String,
    sender: Option<Sender>,
    asked_at: Option<DateTime>,
    answered_at: Option<DateTime>,
    waits_on: Option<String>,
    verdict: Option<String>,
    targets: Option<Vec<String>>,
}

impl TryFrom<PageRow> for Record {
    type Error = String;

    fn try_from(r: PageRow) -> Result<Self, String> {
        let time = |t: Option<&str>| t.map(datetime).transpose();
        let sender = sender(
            r.actor_issuer,
            r.actor_ref,
            (r.actor_id, r.actor_name, r.actor_kind),
            r.resolved_at.as_deref(),
        )?;
        Ok(Self {
            path: page_path(&r.project, &r.key, &r.kind, r.n),
            created_at: datetime(&r.created_at)?,
            asked_at: time(r.asked_at.as_deref())?,
            answered_at: time(r.answered_at.as_deref())?,
            project: r.project,
            key: r.key,
            kind: r.kind,
            n: r.n.to_string(),
            title: r.title,
            role: r.role,
            workspace: r.workspace,
            size: r.size,
            sender,
            waits_on: r.waits_on,
            verdict: r.verdict,
            targets: r
                .targets
                .filter(|t| !t.is_empty())
                .map(|t| t.split('\n').map(str::to_owned).collect()),
        })
    }
}

/// 記録を、定義の同じ欄を持つ形（一覧の行と、返事・印の答え）にする。
macro_rules! from_record {
    ($($ty:ty),*) => {$(
        impl From<Record> for $ty {
            fn from(r: Record) -> Self {
                Self {
                    project: r.project,
                    key: r.key,
                    kind: r.kind,
                    n: r.n,
                    title: r.title,
                    role: r.role,
                    workspace: r.workspace,
                    created_at: r.created_at,
                    size: r.size,
                    path: r.path,
                    sender: r.sender,
                    asked_at: r.asked_at,
                    answered_at: r.answered_at,
                    waits_on: r.waits_on,
                    verdict: r.verdict,
                    targets: r.targets,
                }
            }
        }
    )*};
}

from_record!(Page, SetReplyStateOutput, AddReviewMarksOutput);

/// 答えの file の記録の欄。直せなければ（DB の時刻が読めない）、その訳の字。
#[derive(Clone)]
pub struct FileRecord {
    project: String,
    key: String,
    name: String,
    ext: String,
    n: String,
    content_type: String,
    size: i64,
    sha256: String,
    role: String,
    workspace: String,
    created_at: DateTime,
    path: String,
    sender: Option<Sender>,
}

impl TryFrom<FileRow> for FileRecord {
    type Error = String;

    fn try_from(r: FileRow) -> Result<Self, String> {
        Ok(Self {
            path: file_path(&r.project, &r.key, &r.name, &r.ext, r.n),
            created_at: datetime(&r.created_at)?,
            sender: sender(
                r.actor_issuer,
                r.actor_ref,
                (r.actor_id, r.actor_name, r.actor_kind),
                r.resolved_at.as_deref(),
            )?,
            project: r.project,
            key: r.key,
            name: r.name,
            ext: r.ext,
            n: r.n.to_string(),
            content_type: r.content_type,
            size: r.size,
            sha256: r.sha256,
            role: r.role,
            workspace: r.workspace,
        })
    }
}

impl FileRecord {
    /// この server の中の path（`/<project>/<key>/files/<語>-<n>.<拡張子>`）。
    pub fn path(&self) -> &str {
        &self.path
    }

    /// 置いた答え（要求の `Host` で組んだ `url` と、同じ字の `Location`）。
    pub fn created(self, url: String) -> CreateFileOutput {
        CreateFileOutput {
            project: self.project,
            key: self.key,
            name: self.name,
            ext: self.ext,
            n: self.n,
            r#type: self.content_type,
            size: self.size,
            sha256: self.sha256,
            role: self.role,
            workspace: self.workspace,
            created_at: self.created_at,
            path: self.path,
            sender: self.sender,
            location: url.clone(),
            url,
        }
    }
}

impl From<FileRecord> for File {
    fn from(r: FileRecord) -> Self {
        Self {
            project: r.project,
            key: r.key,
            name: r.name,
            ext: r.ext,
            n: r.n,
            r#type: r.content_type,
            size: r.size,
            sha256: r.sha256,
            role: r.role,
            workspace: r.workspace,
            created_at: r.created_at,
            path: r.path,
            sender: r.sender,
        }
    }
}
