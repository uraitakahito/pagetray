#!/usr/bin/env bash
# image が、空の image の中で起き、ページと file を置いて読めて、browser から来た書く要求を決めたとおりに断り・通し、
# 止める合図で終わり、起こし直しても残るか、前回の終わり方を次の起動で出すかを見る。
# 使い方: ci/smoke-image.sh <image>
set -euo pipefail

image=${1:?使い方: ci/smoke-image.sh <image>}
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
trap 'docker logs "$name" || true; docker rm -f "$name" >/dev/null 2>&1 || true; docker volume rm "$name-data" >/dev/null 2>&1 || true' EXIT

docker run -d --name "$name" -p 127.0.0.1:8801:8080 -v "$name-data:/data" "$image"
up
curl -fsS -X POST --data-binary '<title>CI</title><p>image の中で動く</p>' \
  "$url/api/pages?project=ci&key=K-1&kind=result&role=tpm" | tee "$tmp/put.json"
grep -q '"n":1' "$tmp/put.json"
# 1×1 の PNG を置き、同じ byte が返るか。
echo 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==' \
  | base64 -d >"$tmp/dot.png"
curl -fsS -X POST -H 'Content-Type: image/png' --data-binary "@$tmp/dot.png" \
  "$url/api/files?project=ci&key=K-1&name=dot.png&role=tpm" | tee "$tmp/put-file.json"
grep -q '"path":"/ci/K-1/files/dot-1.png"' "$tmp/put-file.json"
curl -fsS -o "$tmp/got.png" "$url/ci/K-1/files/dot-1.png"
cmp "$tmp/dot.png" "$tmp/got.png"
curl -fsS "$url/api/version" | grep -q '"api":1'
# browser から来た書く要求（Origin 付き）は断る。通すのは、一覧の UI の form が file を上げる形だけ。
code() { curl -s -o "$tmp/answer.json" -w '%{http_code}' "$@"; }
png=(-H 'Content-Type: image/png' --data-binary "@$tmp/dot.png")
put="$url/api/files?project=ci&key=K-1&name=ui.png&role=tpm&workspace=w:1"
for origin in 'http://127.0.0.1:8800' 'http://localhost:8801' 'https://evil.example' 'null'; do
  test "$(code -X POST -H "Origin: $origin" "${png[@]}" "$put")" = 403
done
test "$(code -X POST -H "Origin: $url" -H 'Sec-Fetch-Site: cross-site' "${png[@]}" "$put")" = 403
test "$(code -X POST -H "Origin: $url" --data-binary '<p>x</p>' "$url/api/pages?project=ci&key=K-1&kind=result&role=tpm")" = 403
test "$(code -X DELETE -H "Origin: $url" "$url/api/files?project=ci")" = 403
test "$(code -X DELETE -H "Origin: $url" "$url/api/pages?project=ci")" = 403
# 一覧の UI の form は通り、引数の役割でなく product-manager で記録される。
test "$(code -X POST -H "Origin: $url" -H 'Sec-Fetch-Site: same-origin' "${png[@]}" "$put")" = 201
grep -q '"role":"product-manager","workspace":""' "$tmp/answer.json"
# Origin の無い要求（CLI・tpm）は、前のまま引数の役割で記録される。
test "$(code -X POST "${png[@]}" "$put")" = 201
grep -q '"role":"tpm","workspace":"w:1"' "$tmp/answer.json"
# 断った物は、何も置かず、何も消していない。
test "$(curl -fsS "$url/api/pages?project=ci" | grep -o '"path":"[^"]*"' | tr '\n' ' ')" = '"path":"/ci/K-1/result-1.html" '
test "$(curl -fsS "$url/api/files?project=ci&sort=created_at" | grep -o '"path":"[^"]*"' | tr '\n' ' ')" \
  = '"path":"/ci/K-1/files/dot-1.png" "path":"/ci/K-1/files/ui-1.png" "path":"/ci/K-1/files/ui-2.png" '
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
grep -q 'uri=/api/version' "$tmp/logs.txt"
curl -fsS "$url/ci/K-1/result-1.html" | grep -q 'image の中で動く'
curl -fsS "$url/?q=image" | grep -q 'result-1'
rm -f "$tmp/got.png"
curl -fsS -o "$tmp/got.png" "$url/ci/K-1/files/dot-1.png"
cmp "$tmp/dot.png" "$tmp/got.png"
echo "image は動く: $image"
