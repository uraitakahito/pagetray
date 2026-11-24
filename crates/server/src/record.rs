//! DB のページの行（[`crate::db::PageRow`]）を、答えのページの記録にする（ListPages の行と、SetReplyState・AddReviewMarks の答えで同じ形）。
//! 返事・判定・見た物・送り手の欄は、ある時だけ（Product Manager の待ちに `waitsOn` は無い）。
use aws_smithy_types::DateTime;
use pagetray_sdk::model::{ActorSummary, Page, Sender};
use pagetray_sdk::output::{AddReviewMarksOutput, SetReplyStateOutput};

use crate::datetime;
use crate::db::PageRow;
use crate::pages::page_path;

/// 答えのページの記録の欄。直せなければ（DB の時刻が読めない）、その訳の字。
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
        let sender = match (r.actor_issuer, r.actor_ref) {
            (Some(issuer), Some(r#ref)) => {
                let actor = match (r.actor_id, r.actor_name, r.actor_kind) {
                    (Some(id), Some(name), Some(kind)) => Some(ActorSummary { id, name, kind }),
                    _ => None,
                };
                Some(Sender {
                    issuer,
                    r#ref,
                    resolved_at: actor.as_ref().and(time(r.resolved_at.as_deref())?),
                    actor,
                })
            }
            _ => None,
        };
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
