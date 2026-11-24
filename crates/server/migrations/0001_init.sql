-- 新しい server の表（AI-36）。今の server の migrations/0001〜0006 を当てた後の作りと同じ物を、1 つで作る
-- （表・列とその順・型・空を許すか・主キー・一意・索引・外のキー・check・trigger・全文検索の表。
-- crates/server/tests/db.rs が、今の migrations を当てた DB と突き合わせる）。列の意味は docs/src/db.md。

-- ページと file の送り手。置く時に、呼ぶ側が付随する id（actorIssuer・actorRef）を書けば 1 行足す。
-- castlist に聞けた時は、その時の答えも残す。足すだけの表（書き換えも消しもしない）。ページや file を消しても残る。
create table senders(
  id           integer primary key,
  actor_issuer text not null,    -- 付随する id を出した物（tpm など）
  actor_ref    text not null,    -- 付随する id（<種類>:<値>）
  actor_id     text,             -- castlist のアクターの id。castlist に聞かなかった時は空（外部キーは無い。castlist の DB にある）
  actor_name   text,             -- castlist のアクターの名（その時の物）
  actor_kind   text,             -- castlist のアクターの種類（ai・human）
  resolved_at  text,             -- castlist が答えた時刻（答えの as_of）
  created_at   text not null     -- 行を足した時刻（UTC）
) strict;
create index senders_actor on senders(actor_id);
create trigger senders_no_update before update on senders
begin
  select raise(abort, 'append-only: senders');
end;
create trigger senders_no_delete before delete on senders
begin
  select raise(abort, 'append-only: senders');
end;

create table pages(
  id          integer primary key,
  project     text not null,     -- 状態の folder の名（URL の 1 段目）
  key         text not null,     -- Key（課題の名）
  kind        text not null,     -- brief・confirm・proposal・plan・result・issue …（名の一覧は呼ぶ側が持つ）
  n           integer not null,  -- 種類ごとの番号
  title       text not null,     -- <title> から取る
  role        text not null,     -- 置いた担当の役割
  workspace   text not null,     -- 置いた担当の workspace の ID（わからなければ空）
  created_at  text not null,     -- 置いた時刻（UTC・ミリ秒まで・24 字）
  body        blob not null,     -- 置かれた HTML をそのまま
  asked_at    text,              -- 返事待ちにした時刻。返事を求めていなければ空
  answered_at text,              -- 答えた時刻。まだ答えていなければ空
  sender_id   integer references senders(id) on delete restrict on update restrict,
  waits_on    text check (waits_on in ('review', 'tpm')),  -- 誰の返事を待つか。空は Product Manager
  verdict     text check (verdict in ('approve', 'concerns', 'reject', 'not-assessed')),  -- レビューの判定
  unique(project, key, kind, n)
) strict;
create index pages_created on pages(created_at);
-- 返事待ち（求めて、まだ答えていない）のページだけの索引。
create index pages_waiting on pages(asked_at) where asked_at is not null and answered_at is null;
create index pages_sender on pages(sender_id);
-- rowid は pages.id。3 字ずつの索引なので日本語にも効く。
create virtual table pages_fts using fts5(title, text, tokenize='trigram');

-- 消したページの番号。本文は持たない。同じ番号を 2 度使わないためと、消したページの URL に 410 を返すため。
create table removed(
  project    text not null,
  key        text not null,
  kind       text not null,
  n          integer not null,
  removed_at text not null,      -- 消した時刻（UTC）
  primary key(project, key, kind, n)
) strict;

-- ページが見た物・見てほしい物。1 つに 1 行。ページを消すと一緒に消える。
create table page_targets(
  page_id    integer not null references pages(id) on delete cascade on update restrict,
  target     text not null,
  primary key(page_id, target)
) strict;
-- 見られたページの側から、そのページを見たレビューを引く。
create index page_targets_target on page_targets(target);

-- ページから参照する file（画像・PDF・字の file）。本文はそのまま持つ。
create table files(
  id         integer primary key,
  project    text not null,      -- 状態の folder の名（URL の 1 段目）
  key        text not null,      -- Key（課題の名）
  name       text not null,      -- 名の語（拡張子と番号を除いた所）
  ext        text not null,      -- 拡張子
  n          integer not null,   -- 名と拡張子ごとの番号
  type       text not null,      -- 返す時の Content-Type（拡張子から決まる）
  size       integer not null,   -- byte の数
  sha256     text not null,      -- 中身の SHA-256（16 進の小文字）
  role       text not null,      -- 置いた担当の役割
  workspace  text not null,      -- 置いた担当の workspace の ID（わからなければ空）
  created_at text not null,      -- 置いた時刻（UTC・ミリ秒まで・24 字）
  body       blob not null,      -- 置かれた中身をそのまま
  sender_id  integer references senders(id) on delete restrict on update restrict,
  unique(project, key, name, ext, n)
) strict;
create index files_created on files(created_at);
create index files_sender on files(sender_id);

-- 消した file の番号。中身は持たない。
create table files_removed(
  project    text not null,
  key        text not null,
  name       text not null,
  ext        text not null,
  n          integer not null,
  removed_at text not null,      -- 消した時刻（UTC）
  primary key(project, key, name, ext, n)
) strict;
