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
    let next_token = page.more.then(|| page.rows.last()).flatten().map(|last| {
        token::encode(&[
            TOKEN_KIND,
            sort_name(query.oldest),
            &last.created_at,
            &last.id.to_string(),
        ])
    });
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
        .map(|t| after(t, oldest))
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

fn sort_name(oldest: bool) -> &'static str {
    if oldest { "createdAt" } else { "-createdAt" }
}

/// 頁の印を読む（前の頁の最後の行の、置いた時刻と id）。崩れた物・ほかの検索の物・sort の違う物は 400。
/// ほかの条件は印に結ばない（castlist の ListActors と同じ）。
fn after(token: &str, oldest: bool) -> Result<(String, i64), ListPagesError> {
    let parts = token::read(token, TOKEN_KIND, 4, "ページの検索").map_err(invalid)?;
    let [_, sort, created_at, id] = &parts[..] else {
        return Err(invalid(token::BROKEN).into());
    };
    if sort != sort_name(oldest) {
        return Err(invalid("nextToken は、同じ sort の答えの物を渡す").into());
    }
    let id = id.parse().map_err(|_| invalid(token::BROKEN))?;
    Ok((created_at.clone(), id))
}
