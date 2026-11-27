#!/usr/bin/env bash
# image が、空の image の中で起き、ページと file を置いて読めて、browser から来た書く要求を断り、画面の道は 404 で、
# 止める合図で終わり、起こし直しても残るか、前回の終わり方を次の起動で出すかを見る。終わりに、出した版（v1.0.0）が書いた
# volume を開いて読めるかも見る（ghcr.io の v1.0.0 の image を digest で取る）。
# 使い方: ci/smoke-image.sh <image>
#   置く道は `POST /api/projects/{project}/keys/{key}/…`、一覧の並べ方は `createdAt`、版は `GET /healthz` の JSON、
#   断る答えは restJson1 の形（X-Amzn-Errortype）。
set -euo pipefail

usage='使い方: ci/smoke-image.sh <image>'
if [ "$#" -ne 1 ]; then
  echo "$usage" >&2
  exit 2
fi
image=$1
put_page="api/projects/ci/keys/K-1/kinds/result/pages?role=tpm"
put_file="api/projects/ci/keys/K-1/files?"
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
# 後片付け: container の log を出してから、container と volume を消す（出した版の container $name-prev と、
# その volume をこの image で開いた $name-next も）。
cleanup() {
  for c in "$name" "$name-prev" "$name-next"; do
    if docker inspect "$c" >/dev/null 2>&1; then docker logs "$c" || true; fi
    docker rm -f "$c" >/dev/null 2>&1 || true
  done
  docker volume rm "$name-data" "$name-prev-data" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker run -d --name "$name" -p 127.0.0.1:8801:8080 -v "$name-data:/data" "$image"
up
html=(-H 'Content-Type: text/html')
curl -fsS -X POST "${html[@]}" --data-binary '<title>CI</title><p>image の中で動く</p>' "$url/$put_page" | tee "$tmp/put.json"
# 番号は字。
jq -e '.n == "1" and .path == "/ci/K-1/result-1.html"' "$tmp/put.json" >/dev/null
# 1×1 の PNG を置き、同じ byte が返るか。
echo 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==' \
  | base64 -d >"$tmp/dot.png"
curl -fsS -X POST -H 'Content-Type: image/png' --data-binary "@$tmp/dot.png" \
  "$url/${put_file}name=dot.png&role=tpm" | tee "$tmp/put-file.json"
jq -e '.path == "/ci/K-1/files/dot-1.png"' "$tmp/put-file.json" >/dev/null
curl -fsS -o "$tmp/got.png" "$url/ci/K-1/files/dot-1.png"
cmp "$tmp/dot.png" "$tmp/got.png"
# 版は GetHealth の答え。
curl -fsS "$url/healthz" | jq -e '.status == "ok" and (.version | length > 0)' >/dev/null
# browser から来た書く要求（Origin 付き）は、どれも断る（画面の form の例外は、2.0.0 で画面と一緒に無くした）。
code() { curl -s -D "$tmp/answer-head.txt" -o "$tmp/answer.json" -w '%{http_code}' "$@"; }
png=(-H 'Content-Type: image/png' --data-binary "@$tmp/dot.png")
put="$url/${put_file}name=ui.png&role=tpm&workspace=w:1"
for origin in 'http://127.0.0.1:8800' 'http://localhost:8801' 'https://evil.example' 'null'; do
  test "$(code -X POST -H "Origin: $origin" "${png[@]}" "$put")" = 403
  # 断る答えは、生成した層の誤りと同じ restJson1 の形（header に型の名・本文は JSON の message）。
  tr -d '\r' <"$tmp/answer-head.txt" | grep -qix 'x-amzn-errortype: forbidden'
  jq -e '.message | length > 0' "$tmp/answer.json" >/dev/null
done
test "$(code -X POST -H "Origin: $url" -H 'Sec-Fetch-Site: cross-site' "${png[@]}" "$put")" = 403
test "$(code -X POST -H "Origin: $url" "${html[@]}" --data-binary '<p>x</p>' "$url/$put_page")" = 403
test "$(code -X DELETE -H "Origin: $url" "$url/api/files?project=ci")" = 403
test "$(code -X DELETE -H "Origin: $url" "$url/api/pages?project=ci")" = 403
# 前の一覧の UI の form の形（自分の Origin・same-origin）も、断る。
test "$(code -X POST -H "Origin: $url" -H 'Sec-Fetch-Site: same-origin' "${png[@]}" "$put")" = 403
# 画面は無い（2.0.0 で外した。画面は callboard が持つ）。前の画面の道は 404 で、画面（HTML）を返さない。
for path in / '/?project=ci' /ci/ /ci/K-1/ /messages; do
  test "$(curl -s -D "$tmp/answer-head.txt" -o "$tmp/answer.json" -w '%{http_code}' "$url$path")" = 404
  if tr -d '\r' <"$tmp/answer-head.txt" | grep -qi '^content-type: text/html'; then
    echo "NG: $path が画面を返した" >&2
    exit 1
  fi
done
# Origin の無い要求（CLI・roster・callboard）は、引数の役割で記録される。
test "$(code -X POST "${png[@]}" "$put")" = 201
jq -e '.role == "tpm" and .workspace == "w:1"' "$tmp/answer.json" >/dev/null
# 断った物は、何も置かず、何も消していない。
test "$(curl -fsS "$url/api/pages?project=ci" | jq -r '[.pages[].path] | join(" ")')" = '/ci/K-1/result-1.html'
test "$(curl -fsS "$url/api/files?project=ci&sort=createdAt" | jq -r '[.files[].path] | join(" ")')" \
  = '/ci/K-1/files/dot-1.png /ci/K-1/files/ui-1.png'
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
# 出した版（v1.0.0。digest で固める）が書いた volume を、この image が開いて読めるか（移行の file を替えると、ここで断られる）。
# port は同じ物を使うので、先の container は止める。送り手は書かない（castlist の設定が要らない）。
docker stop "$name" >/dev/null
prev_image=ghcr.io/uraitakahito/pagetray@sha256:612ba0d335e67ca2c91765c58e573b30ab76ace6c844e936099f32cea87ec8cd
docker run -d --name "$name-prev" -p 127.0.0.1:8801:8080 -v "$name-prev-data:/data" "$prev_image"
up
curl -fsS -X POST "${html[@]}" --data-binary '<title>prev</title><p>前の版が書いた</p>' "$url/$put_page" \
  | jq -e '.n == "1" and .path == "/ci/K-1/result-1.html"' >/dev/null
docker stop "$name-prev" >/dev/null
docker rm "$name-prev" >/dev/null
docker run -d --name "$name-next" -p 127.0.0.1:8801:8080 -v "$name-prev-data:/data" "$image"
up
test "$(curl -fsS "$url/api/pages?project=ci" | jq -r '[.pages[].path] | join(" ")')" = '/ci/K-1/result-1.html'
curl -fsS "$url/ci/K-1/result-1.html" | grep -q '前の版が書いた'
echo "image は動く: ${image}"
