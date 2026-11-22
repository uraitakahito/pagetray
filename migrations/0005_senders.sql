-- ページと file の送り手。置く時に、呼ぶ側が付随する id（actor_issuer・actor_ref）を書けば 1 行足す。
-- castlist（送り手の本当の id と名を持つ、この Mac の中のサーバー）に聞けた時は、その時の答えも残す。
-- 足すだけの表（書き換えも消しもしない）。ページや file を消しても、送り手の行は残る。
create table senders(
  id           integer primary key,
  actor_issuer text not null,    -- 付随する id を出した物（tpm など）
  actor_ref    text not null,    -- 付随する id（<種類>:<値>。claude.session:… など）
  actor_id     text,             -- castlist のアクターの id。castlist に聞かなかった時は空（外部キーは無い。castlist の DB にある）
  actor_name   text,             -- castlist のアクターの名（その時の物）
  actor_kind   text,             -- castlist のアクターの種類（ai・human）
  resolved_at  text,             -- castlist が答えた時刻（答えの as_of）
  created_at   text not null     -- 行を足した時刻（UTC の ISO）
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
-- 前からある行は空（送り手を記録していない）。
alter table pages add column sender_id integer references senders(id) on delete restrict on update restrict;
alter table files add column sender_id integer references senders(id) on delete restrict on update restrict;
create index pages_sender on pages(sender_id);
create index files_sender on files(sender_id);
