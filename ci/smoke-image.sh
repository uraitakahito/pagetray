#!/usr/bin/env bash
# image が、空の image の中で起き、ページと file を置いて読めて、止める合図で終わり、起こし直しても残るかを見る。
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
# 止める合図（SIGTERM）で、自分から終わるか（PID 1 は受け口が無いと合図を無視する）。
docker stop "$name"
test "$(docker inspect -f '{{.State.ExitCode}}' "$name")" = 0
docker start "$name"
up
curl -fsS "$url/ci/K-1/result-1.html" | grep -q 'image の中で動く'
curl -fsS "$url/?q=image" | grep -q 'result-1'
rm -f "$tmp/got.png"
curl -fsS -o "$tmp/got.png" "$url/ci/K-1/files/dot-1.png"
cmp "$tmp/dot.png" "$tmp/got.png"
echo "image は動く: $image"
