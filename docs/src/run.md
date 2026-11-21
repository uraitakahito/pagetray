# 動かす

Apple Container（`container` CLI）で、release の image を動かす。`127.0.0.1` だけで待ち受け、LAN にもインターネットにも開かない。

| 物 | 値 |
|---|---|
| image | `ghcr.io/uraitakahito/pagetray:v<版>`（[release](https://github.com/uraitakahito/pagetray/releases) に digest がある。arm64） |
| container | `pagetray`（`127.0.0.1:8801` と `127.0.0.1:8800` → 中の 8080。メモリは Apple Container の下限の 200 MiB、CPU は 1） |
| URL | `http://127.0.0.1:8801/`（一覧の UI。ページは `/<project>/<key>/<kind>-<n>.html`）。`8800` は、前の置き場（Nginx。今は無い）の port。前に置いたページの URL を同じ path で開けるように、同じ container が待ち受ける |
| DB | 名前つきの volume `pagetray-data`（中の `/data/pages.db`）。Mac の folder は mount しない |
| 設定 | 環境変数 `PAGETRAY_DB`（既定 `/data/pages.db`）・`PAGETRAY_LISTEN`（既定 `0.0.0.0:8080`）。image の既定のままでよい |

skills の `tpm` を使う Mac では、container と volume に触る命令を `tpm infra run pagetray -- <命令>` で打つ（この repo の `.claude/roles/config.jsonc` と、skills の設定が、環境 `pagetray` を「TPM に相談」の手続きで宣言している）。下の命令は、その頭を省いて書いてある。使う版は、skills の `.pagetray-version`（skills の cli と合わせてある版）。

## 起こす

```sh
container image pull ghcr.io/uraitakahito/pagetray:v<版>
container image inspect ghcr.io/uraitakahito/pagetray:v<版> | grep -o '"digest" *: *"sha256:[0-9a-f]*"'   # release の digest がこの中にあるか
container volume create pagetray-data                           # 初めての時だけ
container run -d --name pagetray \
  -p 127.0.0.1:8801:8080 \
  -p 127.0.0.1:8800:8080 \
  -v pagetray-data:/data \
  -m 200M -c 1 \
  ghcr.io/uraitakahito/pagetray:v<版>
```

- digest の見方: `container image list` に出る digest は、Apple Container が付ける別の値。release の digest は、`container image inspect` の `variants` の中の `digest` に出る。

> [!WARNING]
> `-p` には必ず `127.0.0.1:` を書く。`-p 8801:8080` と書くと LAN に開く。

- 8801 が使われていたら別の port にする。skills の担当が使う URL は、環境変数 `TPM_PAGES_URL` で合わせる。
- 起きる時に、まだ当てていない移行の SQL（`migrations/`）を DB に当てる。

## 確かめる

1. `container list` で `pagetray` が `running`。
2. `lsof -nP -iTCP:8801 -iTCP:8800 -sTCP:LISTEN` が `127.0.0.1:8801` と `127.0.0.1:8800` だけを出す（`*:` で始まる行があれば LAN に開いている）。
3. `curl -s http://127.0.0.1:8801/healthz` と `curl -s http://127.0.0.1:8800/healthz` が `ok` を返し、`curl -s http://127.0.0.1:8801/api/version` が、起こした版と `"api":1` を返す。browser で `http://127.0.0.1:8801/` が一覧を出す。
4. `container logs pagetray` の頭に `pagetray <版> listen=0.0.0.0:8080 db=/data/pages.db` が出ている。
5. `container stats --no-stream pagetray` のメモリが数 MiB〜20 MiB ほど（初めて起こした直後と、たくさん置いた直後は、OS が file を覚えている分で 50 MiB を超えることがある。起こし直すと戻る）。

## DB の写しを取る

**版を上げる前と、volume に触る前**に取る。動かしたまま取れる。

```sh
curl -fsS -o ~/pagetray-$(date +%Y%m%d-%H%M).db http://127.0.0.1:8801/api/backup
sqlite3 ~/pagetray-<日時>.db 'select count(*) from pages'    # 開けて、件数が合うか
```

写しには、置かれたページの本文がみな入っている。この repo にも CI にも置かない。

## 写しから戻す

volume を失った・DB が壊れた時。戻す間、ページは置けない。

```sh
container stop pagetray
container run --rm \
  -v pagetray-data:/data -v "<写しのある folder>:/in:ro" \
  docker.io/library/rust:1.100.0-alpine \
  sh -c 'rm -f /data/pages.db-wal /data/pages.db-shm && cp /in/<写しの file> /data/pages.db'
container start pagetray
```

- 動かす image は空で `cp` も無いので、file を volume に入れる時だけ、shell のある image を借りる（上は `Containerfile` の build の段と同じ物。何でもよい）。
- 起こしたら「確かめる」をし、`curl -s 'http://127.0.0.1:8801/api/pages?limit=1'` で一番新しいページを見る。
- 写しが無ければ、空の volume で起こす。skills の `tpm page import` が、控えの folder のページを同じ番号で入れ直す（置いた役割の記録は戻らない）。

## 止める・Mac を起こし直した後

- 止めている間、ページは置けず、一覧の UI も開けない（番号をサーバーが決めるため）。止めるのは `container stop pagetray`。DB は volume に残る。
- Mac を起こし直すと止まったままになる。`container system start` の後に `container start pagetray` を打ち、「確かめる」をする。
- volume（`pagetray-data`）は消さない。

## 版を上げる

1. [CHANGELOG.md](https://github.com/uraitakahito/pagetray/blob/main/CHANGELOG.md) で、口の版が変わるか・移行の SQL が足されるかを見る（移行が足された版は、当てた後に前の版へ戻せない）。
2. 新しい版の image を pull し、digest を release の値と見比べる。
3. 「DB の写しを取る」をする。
4. `container stop pagetray`・`container delete pagetray` の後、新しい版で「起こす」の `container run` を打ち、「確かめる」をする（volume はそのまま付く）。
5. 駄目なら、`container delete pagetray` の後、前の版で `container run` を打つ（移行が足されていなければ、同じ volume で起きる。足されていたら、3 の写しから戻す）。
6. 古い image は `container image delete` で消す。
