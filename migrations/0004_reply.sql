-- Product Manager の返事が要るページの状態。呼ぶ側（tpm ask pm・tpm answer）が書く。
-- 前からあるページは、どちらも空（返事を求めていない）。
alter table pages add column asked_at text;     -- 返事待ちにした時刻（UTC の ISO）。返事を求めていなければ空
alter table pages add column answered_at text;  -- 答えた時刻（UTC の ISO）。まだ答えていなければ空
-- 返事待ち（求めて、まだ答えていない）のページだけの索引。
create index pages_waiting on pages(asked_at) where asked_at is not null and answered_at is null;
