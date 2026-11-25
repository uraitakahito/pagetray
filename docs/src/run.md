# 動かす

Apple Container（`container` CLI）で、release の image を動かす。`127.0.0.1` だけで待ち受け、LAN にもインターネットにも開かない。

| 物 | 値 |
|---|---|
| image | `ghcr.io/uraitakahito/pagetray:v<version>`（[release](https://github.com/uraitakahito/pagetray/releases) に digest がある。arm64） |
| container | `pagetray`（`127.0.0.1:8801` と `127.0.0.1:8800` → 中の 8080。メモリは Apple Container の下限の 200 MiB、CPU は 1） |
| URL | `http://127.0.0.1:8801/`（一覧の UI。ページは `/<project>/<key>/<kind>-<n>.html`）。`8800` は、前の置き場（Nginx。今は無い）の port。前に置いたページの URL を同じ path で開けるように、同じ container が待ち受ける |
| DB | 名前つきの volume `pagetray-data-v1`（中の `/data/pages.db`。1.0.0 から。0.8.x までの volume `pagetray-data` からは移す: [release を出す](release.md) の「切り替え」）。Mac の folder は mount しない |
| 記録 | 同じ volume の `/data/logs/`（0.7.0 から。合わせて 4 MiB と 64 KiB まで）。下の「落ちた後に調べる」 |
| 設定 | 環境変数 `PAGETRAY_DB`（既定 `/data/pages.db`）・`PAGETRAY_LISTEN`（既定 `0.0.0.0:8080`）。image の既定のままでよい。`PAGETRAY_CASTLIST_URL`（0.6.0 から。既定は無し）と `PAGETRAY_CASTLIST_LINK_URL`（0.6.0 から。既定 `http://127.0.0.1:8802`）は下の「送り手を castlist に聞く」 |

`roster` を使う Mac では、container と volume に触る命令を `roster infra run pagetray -- <命令>` で打つ（この repo の `.claude/roles/config.jsonc` と、roster の設定が、環境 `pagetray` を「TPM に相談」の手続きで宣言している）。下の命令は、その頭を省いて書いてある。使う版は、roster の `.pagetray-version`（roster の cli と合わせてある版）。

## 送り手を castlist に聞く

castlist の居場所は、2 つの環境変数で書く（どちらも `http://<host>:<port>` の形だけ。後ろの `/` は無くてよい）。pagetray は container の中から castlist に聞き、一覧の UI の link は Product Manager が host（pagetray を動かす機械）の browser で開くので、同じ castlist でも届く所が違う。

| 環境変数 | 使う所 | 本番の値 |
|---|---|---|
| `PAGETRAY_CASTLIST_URL` | pagetray が、container の中から castlist に送り手を聞く先。container の中の `127.0.0.1` は container 自身なので、host の port には届かない | `http://castlist.box:8080`。castlist の container `castlist.box` の port 8080 に、名でつなぐ（既定の network では、Apple Container の DNS が domain `box` の名を答える） |
| `PAGETRAY_CASTLIST_LINK_URL` | 一覧の UI の、送り手の名の link（`<値>/actors/<アクターの id>`）の頭。Product Manager が host の browser で開く | `http://127.0.0.1:8802`（既定のまま。castlist は host の `127.0.0.1:8802` で待ち受ける） |

`PAGETRAY_CASTLIST_URL` を書くと、付随する id を付けて置かれたページと file の送り手を、受けたその時に castlist に聞いて残す（[API の仕様](api.md) の「送り手」）。本番では、下の「起こす」の `container run` に `-e PAGETRAY_CASTLIST_URL=http://castlist.box:8080` を足す。

- 書かなければ、castlist に聞かず、付随する id だけを残す（一覧の UI では「未確認」で、link も出ない）。
- 書いた時は、castlist が止まっている・1 秒で答えない間、付随する id を付けた置く要求は `503` で断られる（何も置かない）。付けない要求は前のまま置ける。
- 一覧の UI の送り手の名は、`PAGETRAY_CASTLIST_LINK_URL`（無ければ `http://127.0.0.1:8802`）の `/actors/<アクターの id>` への link になる。聞く先の `PAGETRAY_CASTLIST_URL` からは作らない（聞く先は container の中から届く所で、host の browser から開けるとは限らない）。
- どちらかの形が `http://<host>:<port>` でなければ、サーバーは起きない（`PAGETRAY_CASTLIST_URL` が無くても、書いた `PAGETRAY_CASTLIST_LINK_URL` は見る）。起きた時の log の頭の行の後ろに ` castlist=<聞く先> castlist_link=<link の頭>` が付く。
- castlist へは、つなぐ先に関わらず `Host: 127.0.0.1` で送る（castlist は `Host` が `127.0.0.1`・`localhost` でない要求を断るので、`castlist.box` の名でつないでも、`Host` は変えない）。

## 起こす

```sh
container image pull ghcr.io/uraitakahito/pagetray:v<version>
container image inspect ghcr.io/uraitakahito/pagetray:v<version> | grep -o '"digest" *: *"sha256:[0-9a-f]*"'   # release の digest がこの中にあるか
container volume create pagetray-data-v1                        # 初めての時だけ
container run -d --name pagetray \
  -p 127.0.0.1:8801:8080 \
  -p 127.0.0.1:8800:8080 \
  -v pagetray-data-v1:/data \
  -m 200M -c 1 \
  ghcr.io/uraitakahito/pagetray:v<version>
```

- digest の見方: `container image list` に出る digest は、Apple Container が付ける別の値。release の digest は、`container image inspect` の `variants` の中の `digest` に出る。

> [!WARNING]
> `-p` には必ず `127.0.0.1:` を書く。`-p 8801:8080` と書くと LAN に開く。

- 8801 が使われていたら別の port にする。roster の担当が使う URL は、環境変数 `TPM_PAGES_URL` で合わせる。
- 0.8.x（volume `pagetray-data`）で動かしていた時は、`container volume create` の前に [release を出す](release.md) の「切り替え」をする（新しい volume を作り、写しを migrate で移してから、上の `container run` で起こす）。
- 起きる時に、まだ当てていない移行の SQL（`crates/server/migrations/`）を DB に当てる。

## 確かめる

1. `container list` で `pagetray` が `running`。
2. `lsof -nP -iTCP:8801 -iTCP:8800 -sTCP:LISTEN` が `127.0.0.1:8801` と `127.0.0.1:8800` だけを出す（`*:` で始まる行があれば LAN に開いている）。
3. `curl -s http://127.0.0.1:8801/healthz` と `curl -s http://127.0.0.1:8800/healthz` が `{"status":"ok","version":"<version>"}` を返す（`version` が起こした版。1.0.0 から。0.8.x までは字の `ok` と `GET /api/version`）。
4. `container logs pagetray` の頭の 2 行が、前回の終わり方（`previous: …`。下の「落ちた後に調べる」）と、`pagetray <version> listen=0.0.0.0:8080 db=/data/pages.db` で終わる行（0.7.0 から、どの行も頭に時刻と `INFO` などの重さが付く。castlist に聞く時は、後ろに ` castlist=http://castlist.box:8080 castlist_link=http://127.0.0.1:8802`）。
5. `container stats --no-stream pagetray` のメモリが数 MiB〜20 MiB ほど（初めて起こした直後と、たくさん置いた直後は、OS が file を覚えている分で 50 MiB を超えることがある。起こし直すと戻る）。
6. 一覧の UI を、読むだけの試験で見る（試験のある 0.3.2 から）。本物の browser（この Mac の Chrome）で開き、一覧が出て API の答えと合うこと・新着の知らせにつながること・絞れること・選んだページが本文の枠に出ること・console に誤りが無いことを見る。置き場には何も書かない。pagetray の checkout のある folder で、起こした版の試験を取り出して流す（Node と pnpm が要る。[開発](development.md) の「一覧の UI の試験」）。checkout が無ければ、browser で `http://127.0.0.1:8801/` を開き、一覧が出るのを見る。

   ```sh
   git -C pagetray fetch --tags
   git -C pagetray worktree add --detach "$PWD/pagetray-smoke" v<version>
   (cd pagetray-smoke/e2e && pnpm install --frozen-lockfile && PAGETRAY_E2E_URL=http://127.0.0.1:8801 pnpm test)
   git -C pagetray worktree remove "$PWD/pagetray-smoke"
   ```

## 一覧の UI から file を上げる

人（Product Manager）が、画面の絵などを担当に渡す道。担当は `roster file put` で上げる。

1. browser で Key の画面（`http://127.0.0.1:8801/<project>/<key>/`。一覧で project と Key の候補を 1 つずつ押してもよい）を開く。一覧の列の下に、file の枠（その Key の file）と上げる枠が出る。
2. 上げる枠で file を選ぶか、枠に落とす。受けるのは png・jpg・jpeg・gif・webp・pdf・txt・log・md・csv・json で、1 つ 16 MiB まで（ほかは送る前に断る）。名は、元の名から CLI と同じ作り方で付く（`Screen Shot 1.PNG` → `screen-shot-1.png`）。
3. 見本を見る（図はその図、字の file は頭の 20 行、PDF は名と大きさ）。token・鍵・個人の情報・ほかの人の画面が写っていないかを見て、「秘密（token・鍵・個人の情報・ほかの人の画面）が写っていないか見た」に印を付ける。印を付けるまで「送る」は押せない。
4. 「送る」を押すと、ページから参照する path（`files/screen-shot-1-1.png` など）が出て、file の枠に足される。この path を担当に伝える。

- 一覧の UI から上げた file は、役割 `product-manager` で記録される（[API の仕様](api.md) の「共通」）。
- 画面から消すことはできない。消すのは TPM 担当に頼む。
- 上げるのには script が要る。script が無くても、file の枠は読める。

## DB の写しを取る

**版を上げる前と、volume に触る前**に取る。動かしたまま取れる。

```sh
curl -fsS -o ~/pagetray-$(date +%Y%m%d-%H%M).db http://127.0.0.1:8801/api/backup
sqlite3 ~/pagetray-<timestamp>.db 'select count(*) from pages'    # 開けて、件数が合うか
```

写しには、置かれたページの本文と、上げられた file（0.4.0 から）がみな入っている。file の分だけ大きくなる。この repo にも CI にも置かない。

写しの中の表と列は [DB の作り](db.md) にある。

### 上げてはいけない file を上げてしまった時

秘密（token・鍵・個人の情報など）が写った file を上げてしまったら、TPM 担当に URL を送って消してもらう（`roster file delete <URL> --yes`）。消しても、**消す前に取った写しには残る**。消した後に写しを取り直し、それより前の写し（`~/pagetray-*.db`）を消す。秘密そのもの（token・鍵）は、すぐ替える。

## 写しから戻す

volume を失った・DB が壊れた時。戻す間、ページは置けない。

```sh
container stop pagetray
container run --rm \
  -v pagetray-data-v1:/data -v "<backup-dir>:/in:ro" \
  docker.io/library/rust:1.100.0-alpine \
  sh -c 'rm -f /data/pages.db-wal /data/pages.db-shm && cp /in/<backup-file> /data/pages.db'
container start pagetray
```

- 動かす image は空で `cp` も無いので、file を volume に入れる時だけ、shell のある image を借りる（上は `Containerfile` の build の段と同じ物。何でもよい）。
- 起こしたら「確かめる」をし、`curl -s 'http://127.0.0.1:8801/api/pages?maxResults=1'` で一番新しいページを見る。
- 写しが無ければ、空の volume で起こす。`roster page import` が、控えの folder のページを同じ番号で入れ直す（置いた役割の記録は戻らない）。

## 落ちた後に調べる

0.7.0 から、pagetray は自分の終わり方と、時刻つきの記録を volume の `/data/logs/` に残す。`container logs` は container を起こし直すと頭から書き直されるが、こちらは起こし直しても、版を上げても残る。

| file | 中身 | 上限 |
|---|---|---|
| `server.log`・`server.log.1`〜`server.log.3` | 時刻（UTC）つきの行。`container logs` に出る行と、問い合わせごとの 1 行（method・path・status・かかった時間） | 1 つ 1 MiB、合わせて 4 MiB。超える前に回し、一番古い物を消す |
| `runs.log` | 終わり方の印。起きた・止める合図を受けた・閉じ終えた・panic・誤りで終わった、を 1 行ずつ | 64 KiB。超える前に、古い半分を落とす |
| `alive` | 生きている印の時刻。30 秒ごとに上書きする | 数十 byte |

読むのは `curl -s http://127.0.0.1:8801/api/logs`（止めなくてよい。印と記録を、古い順の字で返す）。pagetray が起きない時は、止めたまま volume を別の container に付けて読む（volume は、動いている container と同時には付けられない）:

```sh
container run --rm -v pagetray-data-v1:/data docker.io/library/rust:1.100.0-alpine \
  sh -c 'cat /data/logs/runs.log; tail -n 200 /data/logs/server.log'
```

起きるたびに、前回の終わり方を 1 行で出す（`container logs pagetray` の頭と、`server.log`）。

| 行 | 前回に起きたこと | 次に見る所 |
|---|---|---|
| `previous: none (first start)` | 印が無い。初めて起きた | — |
| `previous: stopped cleanly (…)` | 止める合図（`container stop` など）を受けて、閉じ終えた | 誰が止めたかは、pagetray からは見えない。機械の側の記録を見る |
| `previous: got a stop signal but did not finish (…)` | 止める合図は受けたが、閉じ終える前に切られた | 同じ |
| `previous: panicked (…)` | pagetray の中の誤り。行に、場所（file と行の番号）と文が入る | その場所と、`server.log` のその時刻の前の行 |
| `previous: failed (…)` | 起きられずに終わった（DB を開けない・移行に落ちた・待ち受けられない）。行に訳が入る | 行の訳 |
| `previous: no stop mark (…; last alive …)` | 合図も来ず、何も書けずに終わった（SIGKILL・メモリ不足・VM ごと切られた）。`last alive` は最後に生きていた時刻で、落ちたのはその後の 30 秒の内 | `server.log` のその時刻の前の行と、機械の側の記録 |

- 4 MiB に入るのは、問い合わせの行でおよそ 2 万行。
- 記録を書けない時（volume が満ちた、など）も pagetray は動く。行は捨て、`container logs` には、書けなくなった時と戻った時に 1 行ずつ出る。
- 「DB の写しを取る」の写しに、記録は入らない。

## 止める・Mac を起こし直した後

- 止めている間、ページは置けず、一覧の UI も開けない（番号をサーバーが決めるため）。止めるのは `container stop pagetray`。DB は volume に残る。
- Mac を起こし直すと止まったままになる。`container system start` の後に `container start pagetray` を打ち、「確かめる」をする。
- volume（`pagetray-data-v1`）は消さない。0.8.x までの volume `pagetray-data` も、戻す時のために切り替えの後も残す（消す日は Product Manager が決める。AI-36 の段 9）。

## 版を上げる

1. [CHANGELOG.md](https://github.com/uraitakahito/pagetray/blob/main/CHANGELOG.md) で、API の版が変わるか・移行の SQL が足されるかを見る（移行が足された版は、当てた後に前の版へ戻せない。0.5.0 がそう: 0.4.2 までの版は、0.5.0 が起きた後の DB では起きない）。0.8.x から 1.0.0 へは、API と移行がみな替わり volume も替わるので、この手順でなく [release を出す](release.md) の「切り替え」をする（戻す時は、v0.8.0 の image と今の volume `pagetray-data` で起こす）。
2. 新しい版の image を pull し、digest を release の値と見比べる。
3. 「DB の写しを取る」をする。
4. `container stop pagetray`・`container delete pagetray` の後、新しい版で「起こす」の `container run` を打ち、「確かめる」をする（volume はそのまま付く）。
5. 駄目なら、`container delete pagetray` の後、前の版で `container run` を打つ（移行が足されていなければ、同じ volume で起きる。足されていたら、3 の写しから戻す）。
6. 古い image は `container image delete` で消す。
