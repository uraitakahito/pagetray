//! 画面（[`crate::ui`]）が読む、一覧のほかの物: 絞り込みの候補と件数・待ちの数（どれも置き場の全部。絞りに関わらない）と、
//! 見られたページごとの、それを見たレビュー。SQL は今の server（src/db.rs の facets・reviews_of）と同じ。読む接続で読む。
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
