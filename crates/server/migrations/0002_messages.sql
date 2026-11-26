-- メッセージの記録（1.1.0）。ほかの道具（roster）が配ったメッセージの「誰が誰に何を送り、いつ届いたか」を写した物。
-- pagetray は見せるだけ（配らない・状態を自分で決めない・期限を持たない）。列の意味は docs/src/db.md。
create table messages(
  id           integer primary key,
  project      text not null,     -- 状態の folder の名（ページの project と同じ）
  message_id   text not null,     -- メッセージの id（呼ぶ側が付ける。project の中で 1 つ）
  key          text,              -- Key（課題の名）。書かずに置けば空
  heading      text not null,     -- メッセージの見出しの名
  from_name    text not null,     -- 送り手の名
  to_name      text not null,     -- 受け手の名
  text         text not null,     -- 本文（字のまま。HTML として解釈しない）
  sent_at      text not null,     -- 送った時刻（UTC・ミリ秒まで・24 字）
  state        text not null check (state in ('waiting', 'delivered', 'undeliverable', 'expired', 'carried')),
  delivered_at text,              -- 届いた時刻（初めに書かれた物を保つ）。まだ無ければ空
  via          text,              -- どの時に渡したかの名
  deliveries   integer,           -- 渡した回数
  reply_to     text,              -- 返事の先のメッセージの id
  created_at   text not null,     -- pagetray が初めて受けた時刻（UTC）
  updated_at   text not null,     -- 最後に書き足された時刻（UTC）
  sender_id    integer references senders(id) on delete restrict on update restrict,
  unique(project, message_id)
) strict;
create index messages_sent on messages(sent_at);
