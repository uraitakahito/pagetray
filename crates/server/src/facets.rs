//! 画面（[`crate::ui`]）が読む、一覧のほかの物: 絞り込みの候補と件数・待ちの数（どれも置き場の全部。絞りに関わらない）と、
//! 見られたページごとの、それを見たレビュー。SQL は 0.8.x の server と同じ。読む接続で読む。
//! メッセージの一覧の、絞り込みの候補と件数（[`MessageFacets`]）も、ここで読む。
use sqlx::FromRow;

use crate::db::Db;

/// 絞り込みの候補 1 つ（値・件数・添える字）。
#[derive(Debug, FromRow)]
pub struct Count {
    pub value: String,
    pub count: i64,
    pub note: String,
}

/// 絞り込みの候補と待ちの数。DB にある値から作る。
#[derive(Debug)]
pub struct Facets {
    /// 送り手（値は castlist のアクターの id、添える字はその名）。castlist の答えのある送り手だけを、ページの多い順に 100 まで。
    pub senders: Vec<Count>,
    pub roles: Vec<Count>,
    pub workspaces: Vec<Count>,
    pub kinds: Vec<Count>,
    pub keys: Vec<Count>,
    pub projects: Vec<Count>,
    /// Product Manager の返事待ちのページの数。
    pub waiting: i64,
    /// Product Manager が答えたページの数。
    pub answered: i64,
    /// レビュー待ちのページ（レビューの依頼書）の数。
    pub review_waiting: i64,
    /// TPM 担当に上げて、まだ決まっていないページ（レビューのページ）の数。
    pub escalated: i64,
    /// レビューの判定ごとの数。
    pub verdicts: Vec<Count>,
}

/// 見られたページから見た、そのページを見たレビュー 1 つ。
#[derive(Debug, FromRow)]
pub struct ReviewOf {
    /// 見られたページの path。
    pub target: String,
    pub project: String,
    pub key: String,
    pub n: i64,
    pub verdict: Option<String>,
}

/// 返事の待ちの数（待ちの相手ごと。Product Manager の数に、レビューと上げた件を混ぜない）。
#[derive(FromRow)]
struct Replies {
    waiting: i64,
    answered: i64,
    review_waiting: i64,
    escalated: i64,
}

const SENDERS: &str = "select s.actor_id as value, count(*) as count, max(s.actor_name) as note
 from pages p join senders s on s.id = p.sender_id
 where s.actor_id is not null
 group by s.actor_id order by 2 desc, 3 limit 100";
const ROLES: &str = "select role as value, count(*) as count, '' as note
 from pages group by role order by 2 desc, 1";
// workspace の ID だけでは何の居場所かわからないので、その ID で置かれた Key を添える。
const WORKSPACES: &str = "select workspace as value, count(*) as count,
        case when workspace = '' then '' else replace(group_concat(distinct key), ',', ', ') end as note
 from pages group by workspace order by max(created_at) desc limit 100";
const KINDS: &str = "select kind as value, count(*) as count, '' as note
 from pages group by kind order by 2 desc, 1";
const KEYS: &str = "select key as value, count(*) as count, '' as note
 from pages group by key order by max(created_at) desc limit 100";
const PROJECTS: &str = "select project as value, count(*) as count, '' as note
 from pages group by project order by 2 desc, 1";
const REPLIES: &str = "select
        count(*) filter (where waits_on is null and asked_at is not null and answered_at is null) as waiting,
        count(answered_at) filter (where waits_on is null) as answered,
        count(*) filter (where waits_on = 'review' and asked_at is not null and answered_at is null) as review_waiting,
        count(*) filter (where waits_on = 'tpm' and asked_at is not null and answered_at is null) as escalated
 from pages";
const VERDICTS: &str = "select verdict as value, count(*) as count, '' as note
 from pages where verdict is not null group by verdict order by 2 desc, 1";

/// メッセージの一覧で、今絞っている値。候補の 100 件の外でも、候補に入れる（押して外せるように）。
#[derive(Debug, Default)]
pub struct MessagePicked<'a> {
    pub to: Option<&'a str>,
    pub from: Option<&'a str>,
    pub key: Option<&'a str>,
}

/// メッセージの一覧の、絞り込みの候補と件数（どれも置き場の全部。絞りに関わらない）。
#[derive(Debug)]
pub struct MessageFacets {
    /// 状態ごとの数（値は定義の `MessageState` の字）。
    pub states: Vec<Count>,
    /// 受け手・送り手は、多い順に 100 まで。
    pub to: Vec<Count>,
    pub from: Vec<Count>,
    /// Key は、新しく送られた順に 100 まで（Key を書かずに置いた記録は数えない）。
    pub keys: Vec<Count>,
    pub projects: Vec<Count>,
}

const MESSAGE_STATES: &str = "select state as value, count(*) as count, '' as note
 from messages group by state";
const MESSAGE_TO: &str = "select to_name as value, count(*) as count, '' as note
 from messages group by to_name order by 2 desc, 1 limit 100";
const MESSAGE_FROM: &str = "select from_name as value, count(*) as count, '' as note
 from messages group by from_name order by 2 desc, 1 limit 100";
const MESSAGE_KEYS: &str = "select key as value, count(*) as count, '' as note
 from messages where key is not null group by key order by max(sent_at) desc, 1 limit 100";
const MESSAGE_PROJECTS: &str = "select project as value, count(*) as count, '' as note
 from messages group by project order by 2 desc, 1";

impl Db {
    /// 絞り込みの候補と待ちの数（読む接続の 1 つの取引で、同じ時点の物を読む）。
    pub async fn facets(&self) -> Result<Facets, sqlx::Error> {
        let mut tx = self.read().begin().await?;
        let counts = |sql: &'static str| sqlx::query_as::<_, Count>(sql);
        let senders = counts(SENDERS).fetch_all(&mut *tx).await?;
        let roles = counts(ROLES).fetch_all(&mut *tx).await?;
        let workspaces = counts(WORKSPACES).fetch_all(&mut *tx).await?;
        let kinds = counts(KINDS).fetch_all(&mut *tx).await?;
        let keys = counts(KEYS).fetch_all(&mut *tx).await?;
        let projects = counts(PROJECTS).fetch_all(&mut *tx).await?;
        let verdicts = counts(VERDICTS).fetch_all(&mut *tx).await?;
        let replies: Replies = sqlx::query_as(REPLIES).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(Facets {
            senders,
            roles,
            workspaces,
            kinds,
            keys,
            projects,
            waiting: replies.waiting,
            answered: replies.answered,
            review_waiting: replies.review_waiting,
            escalated: replies.escalated,
            verdicts,
        })
    }

    /// メッセージの一覧の、絞り込みの候補と件数（読む接続の 1 つの取引で、同じ時点の物を読む）。
    /// 今絞っている値（`picked`）が候補の 100 件に入らなければ、その値と件数を後ろに足す。
    pub async fn message_facets(
        &self,
        picked: &MessagePicked<'_>,
    ) -> Result<MessageFacets, sqlx::Error> {
        let mut tx = self.read().begin().await?;
        let counts = |sql: &'static str| sqlx::query_as::<_, Count>(sql);
        let states = counts(MESSAGE_STATES).fetch_all(&mut *tx).await?;
        let mut to = counts(MESSAGE_TO).fetch_all(&mut *tx).await?;
        let mut from = counts(MESSAGE_FROM).fetch_all(&mut *tx).await?;
        let mut keys = counts(MESSAGE_KEYS).fetch_all(&mut *tx).await?;
        let projects = counts(MESSAGE_PROJECTS).fetch_all(&mut *tx).await?;
        for (list, column, value) in [
            (&mut to, "to_name", picked.to),
            (&mut from, "from_name", picked.from),
            (&mut keys, "key", picked.key),
        ] {
            let Some(value) = value.filter(|v| !list.iter().any(|c| c.value == *v)) else {
                continue;
            };
            // 列の名は上の 3 つだけ（値は bind する）。
            let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "select count(*) from messages where {column} = ?1"
            )))
            .bind(value)
            .fetch_one(&mut *tx)
            .await?;
            list.push(Count {
                value: value.to_owned(),
                count,
                note: String::new(),
            });
        }
        tx.commit().await?;
        Ok(MessageFacets {
            states,
            to,
            from,
            keys,
            projects,
        })
    }

    /// 見られたページ（path）ごとに、そのページを見たレビューのページ（kind が review。古い順）。
    pub async fn reviews_of(&self, paths: &[String]) -> Result<Vec<ReviewOf>, sqlx::Error> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let paths = serde_json::to_string(paths).unwrap_or_default();
        sqlx::query_as(
            "select t.target, p.project, p.key, p.n, p.verdict
             from page_targets t join pages p on p.id = t.page_id
             where p.kind = 'review' and t.target in (select value from json_each(?1))
             order by p.created_at, p.id",
        )
        .bind(paths)
        .fetch_all(self.read())
        .await
    }
}
