# 用語集

pagetray の文書・画面・code で使う語を、1 か所に並べる。語を足す・替える時は、先にここを直し、文書と code をそれに合わせる。

- **語**: 文書で使う語。かっこの中は、英語の本と code で使う語。
- **code の名**: HTML の id（`#…`）・class（`.…`）・型・欄・命令など。無ければ「—」。
- **使わない言い方**: 前に使っていた語や、ほかの物と紛れる語。文書にも註にも書かない。

[roster](https://github.com/uraitakahito/roster) の `tpm` の語（担当・依頼書・控えなど）は、roster の docs にある。

## 置き場の物

| 語 | code の名 | 意味 | 使わない言い方 |
|---|---|---|---|
| 置き場（store） | — | ページを DB に持ち、番号を付けて返す、このサーバーのこと | pagestore |
| サーバー（server） | `pagetray-server` | 置き場の本体。container の中で動く binary | — |
| CLI | `pagetray` | 置き場を端末から操作する命令 | — |
| ページ（page） | `pages` | 置き場に置く HTML 1 枚。置き直すと、別の番号の別のページになる | Artifact |
| 本文（body） | `body` | ページの HTML そのもの | — |
| ページの記録（page record） | `PageRecord` | ページ 1 枚の、本文を除いた欄（project・Key・種類・番号・題・役割・workspace・時刻・大きさ・path） | メタデータ |
| project | `project` | ページをまとめる、いちばん上の名。`tpm` は、project の folder の path から作る | — |
| Key | `key` | 課題の名（`AI-22` など）。project の下でページをまとめる | 鍵 |
| 種類（kind） | `kind` | ページの種類の名（`plan`・`result` など）。名の一覧は、呼ぶ側が決める | type |
| 番号（number） | `n` | project・Key・種類ごとに、置き場が 1 から付ける数。消した番号は 2 度使わない | version |
| 役割（role） | `role` | ページを置いた人の役割の名（`tpm`・`human` など）。画面の見出しは「担当者（役割）」 | — |
| workspace | `workspace` | ページを置いた人の居場所の ID | — |
| file | `files` | ページから参照する file（画像・PDF・字の file）1 つ。置き直すと、別の番号の別の file になる | — |
| 消した番号（removed number） | `removed`・`files_removed` | 消したページと file の番号。本文は持たない。同じ番号を 2 度使わないためと、消した URL に `410` を返すために残す | — |
| 全文の索引（full-text index） | `pages_fts` | ページの題と本文の字を、3 字ずつに切って引けるようにした索引（SQLite の FTS5）。探す欄の語は、ここで引く | — |
| 移行（migration） | `migrations/` | DB の表を作る・変える SQL。サーバーが起きる時に、まだ当てていない物を番号の順に当てる。当てた物は変えない | — |
| 写し（backup） | `pagetray backup` | DB をそのまま写した、SQLite の file 1 つ | バックアップ |

## API

| 語 | code の名 | 意味 | 使わない言い方 |
|---|---|---|---|
| API | `/api/…` | 置き場が、呼ぶ側に向けて開けている HTTP の受け先の全体 | 口 |
| エンドポイント（endpoint） | — | API の受け先の 1 つ 1 つ（`POST /api/pages` など） | 口 |
| API の仕様（API specification） | `docs/src/api.md` | 呼ぶ側が頼ってよい形を書いた章 | 口の約束 |
| API の版（API version） | `API_VERSION` | API の形を、前からある呼ぶ側が困るように変えた時だけ上げる数。どの答えにも header `Pagetray-Api` で付く | 口の版 |
| 呼ぶ側（caller） | — | API を呼ぶ物（CLI・roster の `tpm page` など） | クライアント |

## 一覧の UI

区画の名は、**働き ＋ 形**（帯・列・枠）で付ける。位置（上・左・右）では呼ばない（狭い画面では、区画が縦に積まれるため）。区画を足す時も、同じ形で名を付け、ここに 1 行足す。

| 語 | code の名 | 意味 | 使わない言い方 |
|---|---|---|---|
| 一覧の UI（list UI） | `templates/index.html` | browser で開く、置き場の画面の全体 | — |
| 操作の帯（toolbar） | `#toolbar` | 画面のいちばん上の横の帯。題・探す欄・並び・新着の印がある | ヘッダー |
| 絞り込みの列（filter column） | `#filters` | 役割・workspace・種類・Key・project の候補と件数が並ぶ列。候補を押すと、一覧の列が絞られる | サイドメニュー |
| 一覧の列（list column） | `#list` | ページの行が並ぶ列。行を押すと、そのページが本文の枠に出る | リスト |
| 本文の枠（viewer） | `#viewer` | 選んだページの path と本文を、1 枚だけ出す枠 | 右の枠 |
| Key の画面（Key view） | `KeyView` | project と Key を 1 つずつ選んだ一覧の UI（`/<project>/<key>/` など）。file の枠と上げる枠が足される | — |
| file の枠（file box） | `#files` | Key の画面で、一覧の列の下に出る枠。その Key の file が、図の見本・名・大きさ・上げた人・時刻・ページから参照する path と並ぶ | — |
| 上げる枠（upload box） | `#upload` | Key の画面で、file の枠の下に出る枠。file を選ぶか落とし、見本を見て、見た印を付けてから送る | — |
| 見た印（reviewed check） | `reviewed` | 上げる枠の「秘密が写っていないか見た」の check。付けるまで送れない | — |
| 絞り込み（filter） | `FilterGroup` | 一覧の列に出す行を、条件で減らすこと。条件は URL に出る | フィルター |
| 候補（choice） | `Choice` | 絞り込みの列に出る、押せる値の 1 つ（値と件数） | facet |
| 行（row） | `.rows` | 一覧の列の中の、ページ 1 枚分（時刻・Key・種類と番号・題・役割・workspace） | — |
| 並び（sort order） | `sort` | 一覧の列の行の順。新しい順か、古い順 | ソート |
| 新着の印（new-arrival mark） | `#fresh` | 開いている間にページが置かれると、一覧の列と絞り込みの列は押さずに新しくなる。一覧の列を下へ送っている時は、操作の帯にこの印が出て、押すと一覧の列の頭へ送る。「続きを出す」で古い方を見ている時は、押すと読み直す。数えるのは、今の絞り込みで一覧の列に入ったページだけ | 通知 |
| 未読の印（unread mark） | `.unread` | まだ本文の枠に出していない、後から置かれたページの行に付く印。browser が覚える | — |
