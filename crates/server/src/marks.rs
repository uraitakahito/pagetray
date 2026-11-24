//! ページ 1 枚に書く操作: 返事の状態（SetReplyState。`PUT …/pages/{n}/reply`）と、レビューの印（AddReviewMarks。
//! `POST …/pages/{n}/review`）。答えは、書いた後のページの記録。無いページと消したページは 404（410 でない。今の server と同じ）。
//! だれが書いてよいかは、呼ぶ側（roster）が持つ。ここでは人を見分けない。browser の中のページからは書けない（守りの層）。
use std::sync::Arc;

use pagetray_sdk::error::{AddReviewMarksError, Conflict, NotFound, SetReplyStateError};
use pagetray_sdk::input::{AddReviewMarksInput, SetReplyStateInput};
use pagetray_sdk::output::{AddReviewMarksOutput, SetReplyStateOutput};

use crate::db::{Db, Marked, PageKey, PageRow, Party, Reply};
use crate::errors::{Failure, invalid};
use crate::events::{Events, Note};
use crate::pages::{is_dot, parse_page_path};
use crate::record::Record;

/// ページ 1 枚が持てる見た物の数（1 度の要求にも、足した後の数にも掛かる）。
pub const MAX_TARGETS: usize = 100;
/// 見た物の URL の字の数の上限（byte でなく字で数える）。
pub const MAX_TARGET: usize = 2048;

/// 道の project・key・番号を見る（形は定義が見てある。. と .. は、ここで断る）。合わなければ、その訳。
fn page_key<'a>(
    project: &'a str,
    key: &'a str,
    kind: &'a str,
    n: &str,
) -> Result<PageKey<'a>, String> {
    if is_dot(project) {
        return Err("project に . と .. は使えない".to_owned());
    }
    if is_dot(key) {
        return Err("key に . と .. は使えない".to_owned());
    }
    // 頭に 0 の無い 18 桁までは、定義が見てある。
    let n = n.parse().map_err(|_| "n は 1 以上の数".to_owned())?;
    Ok(PageKey {
        project,
        key,
        kind,
        n,
    })
}

fn not_found() -> NotFound {
    NotFound {
        message: "そのページは無い".to_owned(),
    }
}

/// 書いた後の行を、答えの記録にする。
fn record(operation: &str, row: PageRow) -> Result<Record, Failure> {
    Record::try_from(row).map_err(|e| Failure::data(operation, &e))
}

pub async fn set_reply_state(
    db: Arc<Db>,
    events: Events,
    input: SetReplyStateInput,
) -> Result<SetReplyStateOutput, SetReplyStateError> {
    const OPERATION: &str = "SetReplyState";
    let page = page_key(&input.project, &input.key, &input.kind, &input.n).map_err(invalid)?;
    // 形（waiting・answered と pm・review・tpm）は定義が見てある。
    let reply =
        Reply::parse(&input.state).ok_or_else(|| invalid("state は waiting か answered"))?;
    let on = match input.on.as_deref() {
        None => Party::Pm,
        Some(on) => Party::parse(on).ok_or_else(|| invalid("on は pm・review・tpm のどれか"))?,
    };
    match db
        .set_reply(&page, reply, on)
        .await
        .map_err(|e| Failure::write(OPERATION, e))?
    {
        Marked::Done(row) => {
            tracing::info!(
                "reply {} state={} on={}",
                crate::pages::page_path(page.project, page.key, page.kind, page.n),
                input.state,
                input.on.as_deref().unwrap_or("pm")
            );
            // 答えの記録に直せてから知らせる（直せずに 500 を返す書き込みを、画面に知らせない）。
            let record = record(OPERATION, *row)?;
            events.send(Note::Reply(record.clone().into()));
            Ok(record.into())
        }
        Marked::Missing => Err(not_found().into()),
        Marked::Refused => Err(Conflict {
            message: "そのページは、on の相手の返事を待っていない（何も替えていない）".to_owned(),
        }
        .into()),
    }
}

/// 見た物として受ける字か: この置き場のページの path（`/<project>/<key>/<kind>-<n>.html`）か、
/// `https://` で始まり、空白と制御の字を含まない、[`MAX_TARGET`] 字までの URL（今の crates/api の is_target と同じ）。
fn is_target(target: &str) -> bool {
    parse_page_path(target).is_some()
        || (target.chars().count() <= MAX_TARGET
            && target.len() > "https://".len()
            && target.starts_with("https://")
            && !target.chars().any(|c| c.is_whitespace() || c.is_control()))
}

pub async fn add_review_marks(
    db: Arc<Db>,
    events: Events,
    input: AddReviewMarksInput,
) -> Result<AddReviewMarksOutput, AddReviewMarksError> {
    const OPERATION: &str = "AddReviewMarks";
    let page = page_key(&input.project, &input.key, &input.kind, &input.n).map_err(invalid)?;
    // 判定の形（4 つの字）は定義が見てある。
    if input.verdict.is_some() && page.kind != "review" {
        return Err(invalid("判定を書けるのは、kind が review のページだけ").into());
    }
    let targets = input.targets.as_deref().unwrap_or_default();
    if targets.len() > MAX_TARGETS || !targets.iter().all(|t| is_target(t)) {
        return Err(invalid(
            "targets は、置き場のページの path（/<project>/<key>/<kind>-<n>.html）か https の URL（2048 字まで）を、100 個まで",
        )
        .into());
    }
    match db
        .add_marks(&page, input.verdict.as_deref(), targets, MAX_TARGETS as i64)
        .await
        .map_err(|e| Failure::write(OPERATION, e))?
    {
        Marked::Done(row) => {
            tracing::info!(
                "review {} verdict={} targets={}",
                crate::pages::page_path(page.project, page.key, page.kind, page.n),
                input.verdict.as_deref().unwrap_or("-"),
                targets.len()
            );
            // 答えの記録に直せてから知らせる（直せずに 500 を返す書き込みを、画面に知らせない）。
            let record = record(OPERATION, *row)?;
            events.send(Note::Review(record.clone().into()));
            Ok(record.into())
        }
        Marked::Missing => Err(not_found().into()),
        Marked::Refused => {
            Err(invalid("足すと、ページ 1 枚の見た物が 100 個を超える（何も替えていない）").into())
        }
    }
}
