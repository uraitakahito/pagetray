# CLI

`pagetray` は、置き場を端末から操作する命令。置き場の[口](api.md)を HTTP で呼ぶ。相手は、この Mac の中（`127.0.0.1`）の置き場だけで、認証も設定の file も無い。

## 入れる

release に、macOS（arm64）用の binary が付いている。`gh` で取り、checksum を確かめてから置く。

```sh
cd "$(mktemp -d)"
gh release download v<version> -R uraitakahito/pagetray -p 'pagetray-v<version>-aarch64-apple-darwin.tar.gz*'
shasum -a 256 -c pagetray-v<version>-aarch64-apple-darwin.tar.gz.sha256
tar -xzf pagetray-v<version>-aarch64-apple-darwin.tar.gz -C ~/.local/bin pagetray
pagetray status
```

- browser で落とすと、macOS が開くのを止める（公証をしていない binary のため）。`gh` か `curl` で取る。
- Rust があれば、source からも入る: `cargo install --git https://github.com/uraitakahito/pagetray --tag v<version> --locked pagetray-cli`。
- 消す時は `rm ~/.local/bin/pagetray`。ほかに置く file は無い。

## 命令

| 命令 | すること |
|---|---|
| `pagetray status` | 置き場が生きているか、サーバーの版、口の版が合っているかを出す |
| `pagetray page list` | ページを絞って探す（新しい順） |
| `pagetray page view <page>` | ページの本文を出す。`--web` で browser に、`-o <file>` で file に |
| `pagetray page put <file>` | ページを置く（`-` は stdin）。置いたページの URL を出す |
| `pagetray page delete <page>…` | ページを消す（戻せない） |
| `pagetray page watch` | ページが置かれるたびに 1 行出す |
| `pagetray browse [<project>[/<key>]]` | 一覧の UI を browser で開く。`-n` は URL を出すだけ |
| `pagetray backup -o <file>` | DB の写し（SQLite の file 1 つ）を取る |
| `pagetray api <path>` | 口を直に呼ぶ。`-X <method>`・`-f <name>=<value>`・`--input <file>` |
| `pagetray completion <shell>` | shell の補完の script を出す（`bash`・`zsh`・`fish` など） |

旗の一覧は、どの命令も `--help` で出る。`<page>` は、ページの URL か、`<project>/<key>/<kind>-<n>` の形で書く。

```sh
pagetray page list --project . --key AI-22          # 今いる folder の project の、Key が AI-22 のページ
pagetray page list -q plan --json=key,title,url      # 題と本文から探し、名指した欄だけを JSON で
pagetray page view http://127.0.0.1:8801/p/AI-22/plan-1.html | less
pagetray page put report.html --project p --key AI-22 --kind result
pagetray backup -o ~/pagetray-$(date +%Y%m%d-%H%M).db
```

### 探す（`page list`）

- 絞る旗: `--project`・`--key`・`--kind`・`--role`・`--workspace`・`-q`（題と本文の字）・`--before`・`--after`。`--oldest` で古い順、`-L` で件数（既定 50）。
- `--project` が無ければ、どの project のページも出す。`--project .` は、今いる folder の path から project の名を作る（英数字と `._-` のほかを `-` にする。skills の `tpm` と同じ作り方）。
- 端末には見出しの付いた表、pipe には tab 区切り（見出しは無い）で出す。`--json` は、口の「ページの記録」に `url` を足した配列。`--json=key,url` のように欄を名指すと、その欄だけを出す。

### 置く（`page put`）

- `--project`（か環境変数 `PAGETRAY_PROJECT`）・`--key`・`--kind` が要る。project を今いる folder から勝手に決めることは無い（置き間違いを防ぐため）。
- 番号は置き場が決める。同じ project・Key・種類に置き直すと次の番号になり、前の版は残る。
- 置く人の役割は `--role`（か `PAGETRAY_ROLE`。既定は `human`）、居場所は `--workspace`。

### 消す（`page delete`）

- 戻せない。まず消すページを一覧で出し、**`--yes` を付けた時だけ**消す。`--yes` が無ければ、何も消さずに終わる（終わりの code は 0）。
- 消す先は、命令に書いた物だけ: ページの名指し（いくつでも）か、`--project <name> [--key <key> [--kind <kind>]]`。環境変数の project は、消す先にならない。
- `-` で始まる project の名（`tpm` の project の名）は、URL で書くか、`--` の後ろに書く: `pagetray page delete --yes -- -Users-me-x/AI-22/plan-1`。
- 消したページの URL は `410` を返し、同じ番号は 2 度使われない。
- **`PAGETRAY_NO_DELETE`**: この環境変数が付いていると（空と `0` のほか）、`page delete` と `api` の `DELETE` は、何もせずに code 1 で止まる。置き場は、だれが消してよいかを見ない。消してよい人を呼ぶ側が決める時に、これを使う（skills の `tpm` は、担当を起こす時に付ける）。

## 設定

旗が環境変数より先、環境変数が既定より先。

| 旗 | 環境変数 | 既定 | 中身 |
|---|---|---|---|
| `--url` | `PAGETRAY_URL` | `http://127.0.0.1:8801` | 置き場の URL（`http://` だけ） |
| `--project` | `PAGETRAY_PROJECT` | 無し | `page list` と `page put` の project |
| `--role` | `PAGETRAY_ROLE` | `human` | `page put` で置く人の役割 |
| — | `PAGETRAY_NO_DELETE` | 無し | 付いていれば、消す命令を止める |
| — | `BROWSER` | macOS の `open` | browser を開く命令 |

環境変数の proxy（`HTTP_PROXY` など）は使わない。

## 終わりの code と、口の版

| code | 訳 |
|---|---|
| `0` | できた |
| `1` | 置き場が断った（`4xx`・`5xx`。無いページ `404`・消したページ `410` を含む）。file に書けない時も |
| `2` | 使い方の誤り |
| `3` | 置き場が応えない |
| `4` | 口の版が合わない |

誤りの訳は stderr に 1 行で出る。stdout には、頼んだ物だけが出る。

`pagetray` は、置き場のどの答えでも header `Pagetray-Api`（[口の版](api.md#版)）を見る。自分の話せる版と違えば、答えを使わずに code 4 で止まる。`pagetray status` が、サーバーの版と口の版を出す。その時は、`pagetray` か置き場のどちらかを上げて、版を合わせる。

## `tpm page` との関わり

skills の `tpm` を使う project では、担当は `tpm page`（put・path・list・delete）を使う。`tpm page` は、project・Key・役割・控えの folder・消してよい人という `tpm` の決まりを持ち、同じ口を呼ぶ。`pagetray` は、その決まりを持たない道具で、`tpm` を通さない場面（人の手元・`tpm` の project でない folder）で使う。どちらで置いたページも、同じ置き場に入り、どちらからも探せる。
