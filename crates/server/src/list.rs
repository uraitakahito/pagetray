//! ListPages: ページを探す（`GET /api/pages`）。値の形は定義が見てあるので、ここでは組み合わせと頁の印を見る。
use std::sync::Arc;

use pagetray_sdk::error::ListPagesError;
use pagetray_sdk::input::ListPagesInput;
use pagetray_sdk::output::ListPagesOutput;

use crate::db::{Db, Party, Reply};
use crate::errors::{Failure, invalid};
use crate::record::Record;
use crate::search::{LIMIT, PageQuery};
use crate::token;

const OPERATION: &str = "ListPages";

/// 頁の印の頭（ほかの検索の印と分ける）。
const TOKEN_KIND: &str = "pages";

pub async fn list_pages(
    db: Arc<Db>,
    input: ListPagesInput,
) -> Result<ListPagesOutput, ListPagesError> {
    let query = query(input)?;
    let page = db
        .list(&query)
        .await
        .map_err(|e| Failure::db(OPERATION, e))?;
    let next_token = page
        .more
        .then(|| page.rows.last())
        .flatten()
        .map(|last| token::next(TOKEN_KIND, query.oldest, &last.created_at, last.id));
    let pages = page
        .rows
        .into_iter()
        .map(|row| {
            Record::try_from(row)
                .map(Into::into)
                .map_err(|e| Failure::data(OPERATION, &e))
        })
        .collect::<Result<_, _>>()?;
    Ok(ListPagesOutput { pages, next_token })
}

/// 引数を探す条件にする。
fn query(input: ListPagesInput) -> Result<PageQuery, ListPagesError> {
    // 形（waiting・answered と pm・review・tpm）は定義が見てある。
    let reply = input.reply.as_deref().and_then(Reply::parse);
    let on = input.on.as_deref().and_then(Party::parse);
    let reply = match (reply, on) {
        (None, Some(_)) => {
            return Err(
                invalid("on は reply と一緒に書く（返事の状態を、待ちの相手で絞る）").into(),
            );
        }
        (None, None) => None,
        (Some(reply), on) => Some((reply, on.unwrap_or_default())),
    };
    let oldest = input.sort.as_deref() == Some("createdAt");
    let after = input
        .next_token
        .as_deref()
        .map(|t| token::after(t, TOKEN_KIND, "ページの検索", oldest).map_err(invalid))
        .transpose()?;
    Ok(PageQuery {
        project: input.project,
        key: input.key,
        kind: input.kind,
        role: input.role,
        workspace: input.workspace,
        q: input.q,
        reply,
        verdict: input.verdict,
        actor: input.actor,
        oldest,
        after,
        // 1〜1000 は定義が見てある。
        limit: input.max_results.map_or(LIMIT, i64::from),
    })
}
