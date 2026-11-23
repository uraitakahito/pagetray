-- レビューの印。呼ぶ側（tpm ask review・tpm report review・tpm ask tpm --review）が書く。
-- 誰の返事を待つか。空は Product Manager（前からあるページは、みな空のまま）。返事の状態（asked_at・answered_at）と組で読む。
alter table pages add column waits_on text check (waits_on in ('review', 'tpm'));
-- レビューの判定（kind が review のページだけが持つ）。roster の語のまま。
alter table pages add column verdict text check (verdict in ('approve', 'concerns', 'reject', 'not-assessed'));
-- ページが見た物（レビューのページ）・見てほしい物（レビューの依頼書）。1 つに 1 行。
-- target は、この置き場のページの path（/<project>/<key>/<kind>-<n>.html）か、https の URL（PR など）。
-- ページを消すと一緒に消える。
create table page_targets(
  page_id    integer not null references pages(id) on delete cascade on update restrict,
  target     text not null,
  primary key(page_id, target)
) strict;
-- 見られたページの側から、そのページを見たレビューを引く。
create index page_targets_target on page_targets(target);
