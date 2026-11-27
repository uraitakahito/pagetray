//! メッセージの記録: 置く・書き足す（PutMessage。`PUT /api/projects/{project}/messages/{id}`）と、探す（ListMessages。
//! `GET /api/messages`）と、絞り込みの候補と件数（GetMessageFacets。`GET /api/messages/facets`）。
//!
//! pagetray は、ほかの道具（roster）が配ったメッセージの「誰が誰に何を送り、いつ届いたか」を写して見せるだけ。
//! 配らず、時刻を比べて状態を決めず、期限を持たない（状態は、呼ぶ側が書いた字をそのまま残す）。
//! 値の形（字の長さ・名の形・状態の字）は定義が見てあるので、ここでは定義の形では言えない決まりを見る。
use std::sync::Arc;

use pagetray_sdk::error::{GetMessageFacetsError, ListMessagesError, PutMessageError};
use pagetray_sdk::input::{GetMessageFacetsInput, ListMessagesInput, PutMessageInput};
use pagetray_sdk::model::FacetCount;
use pagetray_sdk::output::{GetMessageFacetsOutput, ListMessagesOutput, PutMessageOutput};

use crate::castlist::Castlist;
use crate::db::{Db, NewMessage, message_from_sender};
use crate::errors::{Failure, PutFailure, internal, invalid};
use crate::events::{Events, Note};
use crate::facets::{Count, MessagePicked};
use crate::pages::{Checked, ask, check, is_dot, stamp_of};
use crate::record;
use crate::search::{LIMIT, MessageQuery};
use crate::token;

/// 頁の印の頭（ほかの検索の印と分ける）。
pub(crate) const TOKEN_KIND: &str = "messages";

/// 一覧の並べ方の名（定義の `MessageSort` の字）。
pub(crate) fn sort_name(oldest: bool) -> &'static str {
    if oldest { "sentAt" } else { "-sentAt" }
}

/// メッセージの記録を 1 件置く・書き足す。
/// 検査の順: 引数と本文（400）→ 付随する id（400）→ castlist（400・503。「送った側」の書き込みだけ）→ DB。
/// 「送った」の記録と「渡した」の記録は、別の process が、別々に・順を問わずに写す。だから、送り手として残すのは
/// 「送った側」の書き込み（状態が `waiting` か `carried`）の物だけで、castlist に聞くのも、その時だけ（ほかの状態の書き込みは、
/// 付随する id が付いていても聞かず、残さない。castlist が止まっていても、渡した時刻の書き足しは通る）。
pub async fn put_message(
    db: Arc<Db>,
    castlist: Option<Castlist>,
    events: Events,
    input: PutMessageInput,
) -> Result<PutMessageOutput, PutMessageError> {
    const OPERATION: &str = "PutMessage";
    // project・key の . と ..・project の api・付随する id の組は、ページを置く時と同じ決まり。
    let Checked { actor, .. } = check(
        &input.project,
        input.key.as_deref().unwrap_or_default(),
        None,
        None,
        (input.actor_issuer.as_ref(), input.actor_ref.as_ref()),
    )
    .map_err(PutFailure::Invalid)?;
    if is_dot(&input.id) {
        return Err(PutFailure::Invalid("id に . と .. は使えない".to_owned()).into());
    }
    let sent_at = stamp_of("sentAt", &input.sent_at).map_err(PutFailure::Invalid)?;
    let delivered_at = input
        .delivered_at
        .as_ref()
        .map(|t| stamp_of("deliveredAt", t))
        .transpose()
        .map_err(PutFailure::Invalid)?;
    let actor = actor.filter(|_| message_from_sender(&input.state));
    let sender = ask(castlist.as_ref(), actor).await?;
    let (created, row) = db
        .put_message(&NewMessage {
            project: &input.project,
            id: &input.id,
            key: input.key.as_deref(),
            heading: &input.heading,
            from: &input.from,
            to: &input.to,
            text: &input.text,
            sent_at: &sent_at,
            state: &input.state,
            delivered_at: delivered_at.as_deref(),
            via: input.via.as_deref(),
            deliveries: input.deliveries.map(i64::from),
            reply_to: input.reply_to.as_deref(),
            sender: sender.as_ref(),
        })
        .await
        .map_err(|e| PutFailure::write(OPERATION, e, "同じ id の記録を、同時に置いた"))?;
    tracing::info!(
        "message {}/{} state={} created={created}",
        row.project,
        row.message_id,
        row.state
    );
    // 答えの記録に直せてから知らせる（直せずに 500 を返す書き込みを、画面に知らせない）。
    let message =
        record::message(row).map_err(|e| PutFailure::Internal(internal(OPERATION, &e)))?;
    events.send(Note::Message(message.clone()));
    Ok(record::put_message_output(
        message,
        if created { 201 } else { 200 },
    ))
}

/// メッセージの記録を探す。
pub async fn list_messages(
    db: Arc<Db>,
    input: ListMessagesInput,
) -> Result<ListMessagesOutput, ListMessagesError> {
    const OPERATION: &str = "ListMessages";
    let oldest = input.sort.as_deref() == Some("sentAt");
    let after = input
        .next_token
        .as_deref()
        .map(|t| {
            token::after_by(t, TOKEN_KIND, "メッセージの検索", sort_name(oldest)).map_err(invalid)
        })
        .transpose()?;
    let query = MessageQuery {
        project: input.project,
        key: input.key,
        to: input.to,
        from: input.from,
        state: input.state,
        heading: input.heading,
        q: input.q,
        oldest,
        after,
        // 1〜1000 は定義が見てある。
        limit: input.max_results.map_or(LIMIT, i64::from),
    };
    let page = db
        .list_messages(&query)
        .await
        .map_err(|e| Failure::db(OPERATION, e))?;
    let next_token = page
        .more
        .then(|| page.rows.last())
        .flatten()
        .map(|last| token::next_by(TOKEN_KIND, sort_name(oldest), &last.sent_at, last.id));
    let messages = page
        .rows
        .into_iter()
        .map(|row| record::message(row).map_err(|e| Failure::data(OPERATION, &e)))
        .collect::<Result<_, _>>()?;
    Ok(ListMessagesOutput {
        messages,
        next_token,
    })
}

/// 状態の字（定義の `MessageState` と同じ）。絞り込みの候補は、この順に出す。
pub(crate) const STATES: [&str; 5] = [
    "waiting",
    "delivered",
    "undeliverable",
    "expired",
    "carried",
];

/// メッセージの記録の、絞り込みの候補と件数（置き場の全部。絞らない）。画面（[`crate::ui::messages`]）と同じ物を読む。
pub async fn get_message_facets(
    db: Arc<Db>,
    input: GetMessageFacetsInput,
) -> Result<GetMessageFacetsOutput, GetMessageFacetsError> {
    const OPERATION: &str = "GetMessageFacets";
    let picked = MessagePicked {
        to: input.to.as_deref(),
        from: input.from.as_deref(),
        key: input.key.as_deref(),
    };
    let mut facets = db
        .message_facets(&picked)
        .await
        .map_err(|e| Failure::db(OPERATION, e))?;
    sort_states(&mut facets.states);
    let counts = |list: Vec<Count>| {
        list.into_iter()
            .map(|c| FacetCount {
                value: c.value,
                count: c.count,
                note: None,
            })
            .collect()
    };
    Ok(GetMessageFacetsOutput {
        states: counts(facets.states),
        to: counts(facets.to),
        from: counts(facets.from),
        keys: counts(facets.keys),
        projects: counts(facets.projects),
    })
}

/// 状態の候補を、決まった順（[`STATES`]）に並べる。
pub(crate) fn sort_states(states: &mut [Count]) {
    states.sort_by_key(|c| STATES.iter().position(|name| *name == c.value));
}
