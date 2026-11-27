# 動かす

Apple Container（`container` CLI）で、release の image を動かす。`127.0.0.1` だけで待ち受け、LAN にもインターネットにも開かない。

| 物 | 値 |
|---|---|
| image | `ghcr.io/uraitakahito/pagetray:v<version>`（[release](https://github.com/uraitakahito/pagetray/releases) に digest がある。arm64） |
| container | `pagetray.box`（1.2.0 から。前の名は `pagetray`。`127.0.0.1:8801` と `127.0.0.1:8800` → 中の 8080。メモリは Apple Container の下限の 200 MiB、CPU は 1） |
| URL | `http://127.0.0.1:8801`（API とページの本文。ページは `/<project>/<key>/<kind>-<n>.html`）。`8800` は、前の置き場（Nginx。今は無い）の port。前に置いたページの URL を同じ path で開けるように、同じ container が待ち受ける |
| DB | 名前つきの volume `pagetray-data-v1`（中の `/data/pages.db`。1.0.0 から。0.8.x の DB は、切り替えの時に移した: [release を出す](release.md) の「切り替え（済み）」）。Mac の folder は mount しない |
| 記録 | 同じ volume の `/data/logs/`（0.7.0 から。合わせて 4 MiB と 64 KiB まで）。下の「落ちた後に調べる」 |
| 設定 | 環境変数 `PAGETRAY_DB`（既定 `/data/pages.db`）・`PAGETRAY_LISTEN`（既定 `0.0.0.0:8080`）。image の既定のままでよい。`PAGETRAY_CASTLIST_URL`（0.6.0 から。既定は無し）は下の「送り手を castlist に聞く」 |

画面（ページの一覧とメッセージの一覧）は、2.0.0 から callboard の `http://127.0.0.1:8803/pages/`・`/messages` にある（前の画面の道 `http://127.0.0.1:8801/` などは `404`）。Product Manager が file を上げるのも、callboard のページの一覧の Key の画面から。

`roster` を使う Mac では、container と volume に触る命令を `roster infra run pagetray -- <命令>` で打つ（この repo の `.claude/roles/config.jsonc` と、roster の設定が、環境 `pagetray` を「TPM に相談」の手続きで宣言している）。下の命令は、その頭を省いて書いてある。使う版は、roster の `.pagetray-version`（roster の cli と合わせてある版）。

## 送り手を castlist に聞く

castlist の居場所は、環境変数 `PAGETRAY_CASTLIST_URL` で書く（`http://<host>:<port>` の形だけ。後ろの `/` は無くてよい）。pagetray は、container の中から castlist に聞く。

| 環境変数 | 使う所 | 本番の値 |
|---|---|---|
| `PAGETRAY_CASTLIST_URL` | pagetray が、container の中から castlist に送り手を聞く先。container の中の `127.0.0.1` は container 自身なので、host の port には届かない | `http://castlist.box:8080`。castlist の container `castlist.box` の port 8080 に、名でつなぐ（既定の network では、Apple Container の DNS が domain `box` の名を答える） |

`PAGETRAY_CASTLIST_URL` を書くと、付随する id を付けて置かれたページと file の送り手を、受けたその時に castlist に聞いて残す（[API の仕様](api.md) の「送り手」）。本番では、下の「起こす」の `container run` に `-e PAGETRAY_CASTLIST_URL=http://castlist.box:8080` を足す。

- 書かなければ、castlist に聞かず、付随する id だけを残す（ページの記録の `sender` は `issuer`・`ref` だけ）。
- 書いた時は、castlist が止まっている・1 秒で答えない間、付随する id を付けた置く要求は `503` で断られる（何も置かない）。付けない要求は前のまま置ける。
- 形が `http://<host>:<port>` でなければ、サーバーは起きない。起きた時の log の頭の行の後ろに ` castlist=<聞く先>` が付く。
- 1.x で足していた `-e PAGETRAY_CASTLIST_LINK_URL=…` は、2.0.0 から読まない（残しても、形の違う値でも起きる）。起こす命令から外してよい。
- castlist へは、つなぐ先に関わらず `Host: 127.0.0.1` で送る（castlist は `Host` が `127.0.0.1`・`localhost` でない要求を断るので、`castlist.box` の名でつないでも、`Host` は変えない）。

## 起こす

```sh
container image pull ghcr.io/uraitakahito/pagetray:v<version>
container image inspect ghcr.io/uraitakahito/pagetray:v<version> | grep -o '"digest" *: *"sha256:[0-9a-f]*"'   # release の digest がこの中にあるか
container volume create pagetray-data-v1                        # 初めての時だけ
container run -d --name pagetray.box \
  -p 127.0.0.1:8801:8080 \
  -p 127.0.0.1:8800:8080 \
  -v pagetray-data-v1:/data \
  -m 200M -c 1 \
  ghcr.io/uraitakahito/pagetray:v<version>
```

- container の名は `pagetray.box`（1.2.0 から、この本はこの名で書く）。同じ network の container（画面を出す callboard）が、名 `pagetray.box` で pagetray に届く（Apple Container の DNS は、domain `box` の付いた名だけを答える。`pagetray` の名では引けない）。host からの URL（`http://127.0.0.1:8801`）は変わらない。前の名 `pagetray` で動かしている container は、下の「版を上げる」の時に、前の名で止めて消してから、この名で起こす（volume はそのまま付く）。
- digest の見方: `container image list` に出る digest は、Apple Container が付ける別の値。release の digest は、`container image inspect` の `variants` の中の `digest` に出る。

> [!WARNING]
> `-p` には必ず `127.0.0.1:` を書く。`-p 8801:8080` と書くと LAN に開く。

- 8801 が使われていたら別の port にする。roster の担当が使う URL は、環境変数 `ROSTER_PAGES_URL` で合わせる。
- 起きる時に、まだ当てていない移行の SQL（`crates/server/migrations/`）を DB に当てる。

## 確かめる

1. `container list` で `pagetray.box` が `running`。
2. `lsof -nP -iTCP:8801 -iTCP:8800 -sTCP:LISTEN` が `127.0.0.1:8801` と `127.0.0.1:8800` だけを出す（`*:` で始まる行があれば LAN に開いている）。
3. `curl -s http://127.0.0.1:8801/healthz` と `curl -s http://127.0.0.1:8800/healthz` が `{"status":"ok","version":"<version>"}` を返す（`version` が起こした版）。
4. `container logs pagetray.box` の頭の 2 行が、前回の終わり方（`previous: …`。下の「落ちた後に調べる」）と、`pagetray <version> listen=0.0.0.0:8080 db=/data/pages.db` で終わる行（0.7.0 から、どの行も頭に時刻と `INFO` などの重さが付く。castlist に聞く時は、後ろに ` castlist=http://castlist.box:8080`）。
5. `container stats --no-stream pagetray.box` のメモリが数 MiB〜20 MiB ほど（初めて起こした直後と、たくさん置いた直後は、OS が file を覚えている分で 50 MiB を超えることがある。起こし直すと戻る）。

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
container stop pagetray.box
container run --rm \
  -v pagetray-data-v1:/data -v "<backup-dir>:/in:ro" \
  docker.io/library/rust:1.100.0-alpine \
  sh -c 'rm -f /data/pages.db-wal /data/pages.db-shm && cp /in/<backup-file> /data/pages.db'
container start pagetray.box
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

起きるたびに、前回の終わり方を 1 行で出す（`container logs pagetray.box` の頭と、`server.log`）。

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

- 止めている間、ページは置けず（番号をサーバーが決めるため）、本文も開けない。止めるのは `container stop pagetray.box`。DB は volume に残る。
- Mac を起こし直すと止まったままになる。`container system start` の後に `container start pagetray.box` を打ち、「確かめる」をする。
- volume（`pagetray-data-v1`）は消さない。

## 版を上げる

1. [CHANGELOG.md](https://github.com/uraitakahito/pagetray/blob/main/CHANGELOG.md) で、API の形が替わるか・移行の SQL が足されるかを見る（移行が足された版は、当てた後に前の版へ戻せない。1.0.0 から後の server は、自分より新しい版が移行を足した DB では起きない）。
2. 新しい版の image を pull し、digest を release の値と見比べる。
3. 「DB の写しを取る」をする。
4. `container stop pagetray.box`・`container delete pagetray.box` の後、新しい版で「起こす」の `container run` を打ち、「確かめる」をする（volume はそのまま付く）。前の名 `pagetray` で動かしている時は、止める・消すを前の名で打つ（起こすのは、新しい名 `pagetray.box`）。
5. 駄目なら、`container delete pagetray.box` の後、前の版で `container run` を打つ（移行が足されていなければ、同じ volume で起きる。足されていたら、3 の写しから戻す）。
6. 古い image は `container image delete` で消す。
