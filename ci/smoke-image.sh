#!/usr/bin/env bash
# image が、空の image の中で起き、置いて読めて、止める合図で終わり、起こし直しても残るかを見る。
# 使い方: ci/smoke-image.sh <image>
set -euo pipefail

image=${1:?使い方: ci/smoke-image.sh <image>}
name=pagetray-smoke
url=http://127.0.0.1:8801

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
  "$url/api/pages?project=ci&key=K-1&kind=result&role=tpm" | tee "${RUNNER_TEMP:-/tmp}/put.json"
grep -q '"n":1' "${RUNNER_TEMP:-/tmp}/put.json"
curl -fsS "$url/api/version" | grep -q '"api":1'
# 止める合図（SIGTERM）で、自分から終わるか（PID 1 は受け口が無いと合図を無視する）。
docker stop "$name"
test "$(docker inspect -f '{{.State.ExitCode}}' "$name")" = 0
docker start "$name"
up
curl -fsS "$url/ci/K-1/result-1.html" | grep -q 'image の中で動く'
curl -fsS "$url/?q=image" | grep -q 'result-1'
echo "image は動く: $image"
