# 動かす

Apple Container（`container` CLI）で、release の image を動かす。`127.0.0.1` だけで待ち受け、LAN にもインターネットにも開かない。

| 物 | 値 |
|---|---|
| image | `ghcr.io/uraitakahito/pagetray:v<version>`（[release](https://github.com/uraitakahito/pagetray/releases) に digest がある。arm64） |
| container | `pagetray`（`127.0.0.1:8801` と `127.0.0.1:8800` → 中の 8080。メモリは Apple Container の下限の 200 MiB、CPU は 1） |
| URL | `http://127.0.0.1:8801/`（一覧の UI。ページは `/<project>/<key>/<kind>-<n>.html`）。`8800` は、前の置き場（Nginx。今は無い）の port。前に置いたページの URL を同じ path で開けるように、同じ container が待ち受ける |
| DB | 名前つきの volume `pagetray-data`（中の `/data/pages.db`）。Mac の folder は mount しない |
| 設定 | 環境変数 `PAGETRAY_DB`（既定 `/data/pages.db`）・`PAGETRAY_LISTEN`（既定 `0.0.0.0:8080`）。image の既定のままでよい |

roster の `tpm` を使う Mac では、container と volume に触る命令を `tpm infra run pagetray -- <命令>` で打つ（この repo の `.claude/roles/config.jsonc` と、roster の設定が、環境 `pagetray` を「TPM に相談」の手続きで宣言している）。下の命令は、その頭を省いて書いてある。使う版は、roster の `.pagetray-version`（roster の cli と合わせてある版）。

## 起こす

```sh
container image pull ghcr.io/uraitakahito/pagetray:v<version>
container image inspect ghcr.io/uraitakahito/pagetray:v<version> | grep -o '"digest" *: *"sha256:[0-9a-f]*"'   # release の digest がこの中にあるか
container volume create pagetray-data                           # 初めての時だけ
container run -d --name pagetray \
  -p 127.0.0.1:8801:8080 \
  -p 127.0.0.1:8800:8080 \
  -v pagetray-data:/data \
  -m 200M -c 1 \
  ghcr.io/uraitakahito/pagetray:v<version>
```

- digest の見方: `container image list` に出る digest は、Apple Container が付ける別の値。release の digest は、`container image inspect` の `variants` の中の `digest` に出る。

> [!WARNING]
> `-p` には必ず `127.0.0.1:` を書く。`-p 8801:8080` と書くと LAN に開く。

- 8801 が使われていたら別の port にする。roster の担当が使う URL は、環境変数 `TPM_PAGES_URL` で合わせる。
- 起きる時に、まだ当てていない移行の SQL（`migrations/`）を DB に当てる。

## 確かめる

1. `container list` で `pagetray` が `running`。
2. `lsof -nP -iTCP:8801 -iTCP:8800 -sTCP:LISTEN` が `127.0.0.1:8801` と `127.0.0.1:8800` だけを出す（`*:` で始まる行があれば LAN に開いている）。
3. `curl -s http://127.0.0.1:8801/healthz` と `curl -s http://127.0.0.1:8800/healthz` が `ok` を返し、`curl -s http://127.0.0.1:8801/api/version` が、起こした版と `"api":1` を返す。
4. `container logs pagetray` の頭に `pagetray <version> listen=0.0.0.0:8080 db=/data/pages.db` が出ている。
5. `container stats --no-stream pagetray` のメモリが数 MiB〜20 MiB ほど（初めて起こした直後と、たくさん置いた直後は、OS が file を覚えている分で 50 MiB を超えることがある。起こし直すと戻る）。
6. 一覧の UI を、読むだけの試験で見る（試験のある 0.3.2 から）。本物の browser（この Mac の Chrome）で開き、一覧が出て API の答えと合うこと・新着の知らせにつながること・絞れること・選んだページが本文の枠に出ること・console に誤りが無いことを見る。置き場には何も書かない。pagetray の checkout のある folder で、起こした版の試験を取り出して流す（Node と pnpm が要る。[開発](development.md) の「一覧の UI の試験」）。checkout が無ければ、browser で `http://127.0.0.1:8801/` を開き、一覧が出るのを見る。

   ```sh
   git -C pagetray fetch --tags
   git -C pagetray worktree add --detach "$PWD/pagetray-smoke" v<version>
   (cd pagetray-smoke/e2e && pnpm install --frozen-lockfile && PAGETRAY_E2E_URL=http://127.0.0.1:8801 pnpm test)
   git -C pagetray worktree remove "$PWD/pagetray-smoke"
   ```

## 一覧の UI から file を上げる

人（Product Manager）が、画面の絵などを担当に渡す道。担当は `tpm file put` で上げる。

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

秘密（token・鍵・個人の情報など）が写った file を上げてしまったら、TPM 担当に URL を送って消してもらう（`tpm file delete <URL> --yes`）。消しても、**消す前に取った写しには残る**。消した後に写しを取り直し、それより前の写し（`~/pagetray-*.db`）を消す。秘密そのもの（token・鍵）は、すぐ替える。

## 写しから戻す

volume を失った・DB が壊れた時。戻す間、ページは置けない。

```sh
container stop pagetray
container run --rm \
  -v pagetray-data:/data -v "<backup-dir>:/in:ro" \
  docker.io/library/rust:1.100.0-alpine \
  sh -c 'rm -f /data/pages.db-wal /data/pages.db-shm && cp /in/<backup-file> /data/pages.db'
container start pagetray
```

- 動かす image は空で `cp` も無いので、file を volume に入れる時だけ、shell のある image を借りる（上は `Containerfile` の build の段と同じ物。何でもよい）。
- 起こしたら「確かめる」をし、`curl -s 'http://127.0.0.1:8801/api/pages?limit=1'` で一番新しいページを見る。
- 写しが無ければ、空の volume で起こす。roster の `tpm page import` が、控えの folder のページを同じ番号で入れ直す（置いた役割の記録は戻らない）。

## 止める・Mac を起こし直した後

- 止めている間、ページは置けず、一覧の UI も開けない（番号をサーバーが決めるため）。止めるのは `container stop pagetray`。DB は volume に残る。
- Mac を起こし直すと止まったままになる。`container system start` の後に `container start pagetray` を打ち、「確かめる」をする。
- volume（`pagetray-data`）は消さない。

## 版を上げる

1. [CHANGELOG.md](https://github.com/uraitakahito/pagetray/blob/main/CHANGELOG.md) で、API の版が変わるか・移行の SQL が足されるかを見る（移行が足された版は、当てた後に前の版へ戻せない）。
2. 新しい版の image を pull し、digest を release の値と見比べる。
3. 「DB の写しを取る」をする。
4. `container stop pagetray`・`container delete pagetray` の後、新しい版で「起こす」の `container run` を打ち、「確かめる」をする（volume はそのまま付く）。
5. 駄目なら、`container delete pagetray` の後、前の版で `container run` を打つ（移行が足されていなければ、同じ volume で起きる。足されていたら、3 の写しから戻す）。
6. 古い image は `container image delete` で消す。
