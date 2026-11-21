-- ページから参照する file（画像・PDF・字の file）。本文はそのまま持つ。
create table files(
  id         integer primary key,
  project    text not null,      -- 状態の folder の名（URL の 1 段目）
  key        text not null,      -- 課題の鍵
  name       text not null,      -- 名の語（拡張子と番号を除いた所）
  ext        text not null,      -- 拡張子（受ける物の一覧は crates/api が持つ）
  n          integer not null,   -- 名と拡張子ごとの番号
  type       text not null,      -- 返す時の Content-Type（拡張子から決まる）
  size       integer not null,   -- byte の数
  sha256     text not null,      -- 中身の SHA-256（16 進の小文字）
  role       text not null,      -- 置いた担当の役割（わからなければ unknown）
  workspace  text not null,      -- 置いた担当の workspace の ID（わからなければ空）
  created_at text not null,      -- UTC の ISO の時刻
  body       blob not null,      -- 置かれた中身をそのまま
  unique(project, key, name, ext, n)
) strict;
create index files_created on files(created_at);
-- 消した file の番号。中身は持たない。同じ番号を 2 度使わないためと、消した file の URL に 410 を返すため。
create table files_removed(
  project    text not null,
  key        text not null,
  name       text not null,
  ext        text not null,
  n          integer not null,
  removed_at text not null,      -- UTC の ISO の時刻
  primary key(project, key, name, ext, n)
) strict;
