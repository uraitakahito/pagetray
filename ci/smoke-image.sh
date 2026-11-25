#!/usr/bin/env bash
# image が、空の image の中で起き、ページと file を置いて読めて、browser から来た書く要求を決めたとおりに断り・通し、
# 止める合図で終わり、起こし直しても残るか、前回の終わり方を次の起動で出すかを見る。
# 使い方: ci/smoke-image.sh <image> [current|next]
#   current（既定）: 今の server（0.8.x）の image。置く道は `POST /api/pages?project&key&kind…` など今の形。
#         1.0.0 から CI も release も使わない（段 9 で今の server と一緒に消す）
#   next: 新しい server の image（1.0.0 から Containerfile。Containerfile.next も同じ物）。置く道は
#         `POST /api/projects/{project}/keys/{key}/…`、一覧の並べ方は `createdAt`、版は `GET /healthz` の JSON、
#         断る答えは restJson1 の形（X-Amzn-Errortype）。
#         終わりに切り替えの予行もする: 今の版（v0.8.0。digest で固める）の写しを、この image の migrate で新しい volume に移し、
#         新しい API で読む（docs/src/release.md の「切り替え」の 2 と 3）
set -euo pipefail

usage='使い方: ci/smoke-image.sh <image> [current|next]'
image=${1:?$usage}
mode=${2:-current}
case "$mode" in
  current)
    put_page="api/pages?project=ci&key=K-1&kind=result&role=tpm"
    put_file="api/files?project=ci&key=K-1&"
    sort=created_at
    ;;
  next)
    put_page="api/projects/ci/keys/K-1/kinds/result/pages?role=tpm"
    put_file="api/projects/ci/keys/K-1/files?"
    sort=createdAt
    ;;
  *)
    echo "$usage" >&2
    exit 2
    ;;
esac
name=pagetray-smoke
url=http://127.0.0.1:8801
tmp=${RUNNER_TEMP:-/tmp}

up() {
  for _ in $(seq 1 40); do
    if curl -fsS "$url/healthz"; then return; fi
    sleep 0.5
  done
  return 1
}
# 後片付け: 切り替えの予行の container（今の版の $name-old・移した先の $name-v1）と volume と、写しの folder も消す。
cleanup() {
  for c in "$name" "$name-old" "$name-v1"; do
    if docker inspect "$c" >/dev/null 2>&1; then docker logs "$c" || true; fi
    docker rm -f "$c" >/dev/null 2>&1 || true
    docker volume rm "$c-data" >/dev/null 2>&1 || true
  done
  if [ -n "${backup_dir:-}" ]; then rm -rf "$backup_dir"; fi
}
trap cleanup EXIT

docker run -d --name "$name" -p 127.0.0.1:8801:8080 -v "$name-data:/data" "$image"
up
html=(-H 'Content-Type: text/html')
curl -fsS -X POST "${html[@]}" --data-binary '<title>CI</title><p>image の中で動く</p>' "$url/$put_page" | tee "$tmp/put.json"
# 番号は、今の server は数、新しい server は字。
jq -e '(.n | tostring) == "1" and .path == "/ci/K-1/result-1.html"' "$tmp/put.json" >/dev/null
# 1×1 の PNG を置き、同じ byte が返るか。
echo 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==' \
  | base64 -d >"$tmp/dot.png"
curl -fsS -X POST -H 'Content-Type: image/png' --data-binary "@$tmp/dot.png" \
  "$url/${put_file}name=dot.png&role=tpm" | tee "$tmp/put-file.json"
jq -e '.path == "/ci/K-1/files/dot-1.png"' "$tmp/put-file.json" >/dev/null
curl -fsS -o "$tmp/got.png" "$url/ci/K-1/files/dot-1.png"
cmp "$tmp/dot.png" "$tmp/got.png"
if [ "$mode" = next ]; then
  # 版は GetHealth の答え（今の GET /api/version は無い）。
  curl -fsS "$url/healthz" | jq -e '.status == "ok" and (.version | length > 0)' >/dev/null
else
  curl -fsS "$url/api/version" | grep -q '"api":1'
fi
# browser から来た書く要求（Origin 付き）は断る。通すのは、一覧の UI の form が file を上げる形だけ。
code() { curl -s -D "$tmp/answer-head.txt" -o "$tmp/answer.json" -w '%{http_code}' "$@"; }
png=(-H 'Content-Type: image/png' --data-binary "@$tmp/dot.png")
put="$url/${put_file}name=ui.png&role=tpm&workspace=w:1"
for origin in 'http://127.0.0.1:8800' 'http://localhost:8801' 'https://evil.example' 'null'; do
  test "$(code -X POST -H "Origin: $origin" "${png[@]}" "$put")" = 403
  if [ "$mode" = next ]; then
    # 断る答えは、生成した層の誤りと同じ restJson1 の形（header に型の名・本文は JSON の message）。
    tr -d '\r' <"$tmp/answer-head.txt" | grep -qix 'x-amzn-errortype: forbidden'
    jq -e '.message | length > 0' "$tmp/answer.json" >/dev/null
  fi
done
test "$(code -X POST -H "Origin: $url" -H 'Sec-Fetch-Site: cross-site' "${png[@]}" "$put")" = 403
test "$(code -X POST -H "Origin: $url" "${html[@]}" --data-binary '<p>x</p>' "$url/$put_page")" = 403
test "$(code -X DELETE -H "Origin: $url" "$url/api/files?project=ci")" = 403
test "$(code -X DELETE -H "Origin: $url" "$url/api/pages?project=ci")" = 403
# 一覧の UI の form は通り、引数の役割でなく product-manager で記録される。
test "$(code -X POST -H "Origin: $url" -H 'Sec-Fetch-Site: same-origin' "${png[@]}" "$put")" = 201
jq -e '.role == "product-manager" and .workspace == ""' "$tmp/answer.json" >/dev/null
# Origin の無い要求（CLI・roster）は、前のまま引数の役割で記録される。
test "$(code -X POST "${png[@]}" "$put")" = 201
jq -e '.role == "tpm" and .workspace == "w:1"' "$tmp/answer.json" >/dev/null
# 断った物は、何も置かず、何も消していない。
test "$(curl -fsS "$url/api/pages?project=ci" | jq -r '[.pages[].path] | join(" ")')" = '/ci/K-1/result-1.html'
test "$(curl -fsS "$url/api/files?project=ci&sort=$sort" | jq -r '[.files[].path] | join(" ")')" \
  = '/ci/K-1/files/dot-1.png /ci/K-1/files/ui-1.png /ci/K-1/files/ui-2.png'
# 止める合図（SIGTERM）で、自分から終わるか（PID 1 は受け口が無いと合図を無視する）。
docker stop "$name"
test "$(docker inspect -f '{{.State.ExitCode}}' "$name")" = 0
docker start "$name"
up
# 終わり方の印（volume の logs/）: 止める合図で止めた後の起動は「きれいに止まった」と出す。
# （docker logs を grep -q に pipe でつながない: grep が先に終わると、書く側が SIGPIPE で落ち、pipefail で全体が落ちる）
docker logs "$name" >"$tmp/logs-1.txt" 2>&1
grep -q 'previous: stopped cleanly (' "$tmp/logs-1.txt"
# 合図なしで切った（SIGKILL）後の起動は「停止の印なし」と、最後に生きていた時刻を出す。
docker kill "$name"
docker start "$name"
up
docker logs "$name" >"$tmp/logs-2.txt" 2>&1
grep -q 'previous: no stop mark (.*; last alive 20' "$tmp/logs-2.txt"
# 残した記録は、止めずに読める。起こし直す前の問い合わせの行も、時刻つきで残っている。
curl -fsS "$url/api/logs" >"$tmp/logs.txt"
grep -q ' start version=' "$tmp/logs.txt"
grep -q 'uri=/api/files' "$tmp/logs.txt"
curl -fsS "$url/ci/K-1/result-1.html" | grep -q 'image の中で動く'
curl -fsS "$url/?q=image" | grep -q 'result-1'
rm -f "$tmp/got.png"
curl -fsS -o "$tmp/got.png" "$url/ci/K-1/files/dot-1.png"
cmp "$tmp/dot.png" "$tmp/got.png"
# 切り替えの予行（新しい server だけ）: 今の版の image に今の API で置き、`GET /api/backup` の写しを取り、この image の
# migrate で新しい volume に移し、その volume で起こして新しい API で読む。port は同じ物を使うので、先の container は消す。
if [ "$mode" = next ]; then
  # 消さずに止める（port は空く）。後の段で落ちた時も、cleanup が log を出してから消す。
  docker stop "$name" >/dev/null
  # 今の版は v0.8.0（release の本文の digest で固める。tag を後から替えられても同じ物を取る）。1.0.0 の後は今の版を出さないので、
  # 戻す時に使う版（v0.8.0）のまま替えない。
  old_image=ghcr.io/uraitakahito/pagetray@sha256:82d1a0e6de15c31b4b7a8a377a687adbfdb79b415b1230a41452e600cd7455af
  docker run -d --name "$name-old" -p 127.0.0.1:8801:8080 -v "$name-old-data:/data" "$old_image"
  up
  # 今の API（`POST /api/pages?project&key&kind…`）で、ページ 3 つ（鍵を分ける。1 つはレビュー）と file 1 つを置く。
  old_page() {
    curl -fsS -X POST "${html[@]}" --data-binary "<title>$1</title><p>moved before $1</p>" \
      "$url/api/pages?project=ci&key=$1&kind=$2&role=tpm" >/dev/null
  }
  old_page K-1 result
  old_page K-2 result
  old_page K-1 review
  curl -fsS -X POST -H 'Content-Type: image/png' --data-binary "@$tmp/dot.png" \
    "$url/api/files?project=ci&key=K-1&name=dot.png&role=tpm" >/dev/null
  # 消した番号: ページを 1 つ足して消す（K-1 の result-2）・file を 1 つ足して消す（K-1 の gone-1.png）。
  old_page K-1 result
  curl -fsS -X DELETE "$url/api/pages?project=ci&key=K-1&kind=result&n=2" | jq -e '.removed == ["/ci/K-1/result-2.html"]' >/dev/null
  curl -fsS -X POST -H 'Content-Type: image/png' --data-binary "@$tmp/dot.png" \
    "$url/api/files?project=ci&key=K-1&name=gone.png&role=tpm" >/dev/null
  curl -fsS -X DELETE "$url/api/files?project=ci&key=K-1&name=gone.png&n=1" | jq -e '.removed | length == 1' >/dev/null
  # 返事の状態（K-2 の result-1 をレビューの席の返事待ちに）と、レビューの印（判定と見た物）。
  curl -fsS -X POST "$url/api/pages/reply?project=ci&key=K-2&kind=result&n=1&state=waiting&on=review" \
    | jq -e '.waits_on == "review" and (.asked_at | length > 0)' >/dev/null
  curl -fsS -X POST -H 'Content-Type: application/json' --data '{"verdict":"approve","targets":["/ci/K-1/result-1.html"]}' \
    "$url/api/pages/review?project=ci&key=K-1&kind=review&n=1" | jq -e '.verdict == "approve"' >/dev/null
  backup_dir=$(mktemp -d "$tmp/backup.XXXXXX")
  curl -fsS -o "$backup_dir/pages.db" "$url/api/backup"
  test -s "$backup_dir/pages.db"
  docker stop "$name-old" >/dev/null
  # release.md の 2 の行と同じ形（新しい volume を /data・写しの folder を /in に読むだけで付ける）。0 で終わらなければ落ちる。
  docker run --rm -v "$name-v1-data:/data" -v "$backup_dir:/in:ro" "$image" \
    migrate --from /in/pages.db --to /data/pages.db | tee "$tmp/migrate.txt"
  # 照らしの行はみな ok で、置いた数が移り、終わりの行は「migrated:」。
  if sed '$d' "$tmp/migrate.txt" | grep -v ' ok$'; then
    echo "NG: migrate の照らしに ok でない行がある" >&2
    exit 1
  fi
  for moved in 'pages 3 → 3' 'removed 1 → 1' 'page_targets 1 → 1' 'files 1 → 1' 'files_removed 1 → 1'; do
    grep -qx "$moved digest .* ok" "$tmp/migrate.txt"
  done
  test "$(tail -n 1 "$tmp/migrate.txt")" = 'migrated: /in/pages.db → /data/pages.db'
  # 移した volume で起こし、新しい API で、置いた物が同じ数だけ読め、中身も同じ。
  docker run -d --name "$name-v1" -p 127.0.0.1:8801:8080 -v "$name-v1-data:/data" "$image"
  up
  test "$(curl -fsS "$url/api/pages?project=ci" | jq -r '[.pages[].path] | sort | join(" ")')" \
    = '/ci/K-1/result-1.html /ci/K-1/review-1.html /ci/K-2/result-1.html'
  test "$(curl -fsS "$url/api/files?project=ci&sort=$sort" | jq -r '[.files[].path] | join(" ")')" = '/ci/K-1/files/dot-1.png'
  curl -fsS "$url/ci/K-2/result-1.html" | grep -q 'moved before K-2'
  # 探す索引（pages_fts）も移っている。
  curl -fsS "$url/?q=moved" | grep -q 'result-1'
  rm -f "$tmp/got.png"
  curl -fsS -o "$tmp/got.png" "$url/ci/K-1/files/dot-1.png"
  cmp "$tmp/dot.png" "$tmp/got.png"
  # 返事の状態とレビューの印も移っている。
  curl -fsS "$url/api/pages?project=ci&reply=waiting&on=review" \
    | jq -e '[.pages[] | [.path, .waitsOn, (.askedAt | length > 0)]] == [["/ci/K-2/result-1.html", "review", true]]' >/dev/null
  curl -fsS "$url/api/pages?project=ci&verdict=approve" \
    | jq -e '[.pages[] | [.path, .targets]] == [["/ci/K-1/review-1.html", ["/ci/K-1/result-1.html"]]]' >/dev/null
  # 消したページと file の URL は 410 のまま。次に置くページは、消した番号（2）を使わない。
  test "$(curl -s -o /dev/null -w '%{http_code}' "$url/ci/K-1/result-2.html")" = 410
  test "$(curl -s -o /dev/null -w '%{http_code}' "$url/ci/K-1/files/gone-1.png")" = 410
  curl -fsS -X POST "${html[@]}" --data-binary '<title>after</title>' "$url/$put_page" \
    | jq -e '.n == "3" and .path == "/ci/K-1/result-3.html"' >/dev/null
fi
echo "image は動く（${mode}）: ${image}"
