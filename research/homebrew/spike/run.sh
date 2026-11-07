#!/bin/bash
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
IMAGE=ghcr.io/homebrew/brew:latest
export $(grep -E '^CLOUDFLARE_R2_(ENDPOINT|ACCESS_KEY_ID|SECRET_ACCESS_KEY)=' "$HERE/../../../.env" | tr -d '"' | xargs)

podman rm -f spike-pub spike-con >/dev/null 2>&1 || true
podman run -d --name spike-pub -v "$HERE/scripts:/scripts:ro,Z" -e CLOUDFLARE_R2_ENDPOINT -e CLOUDFLARE_R2_ACCESS_KEY_ID -e CLOUDFLARE_R2_SECRET_ACCESS_KEY "$IMAGE" sleep infinity >/dev/null
podman run -d --name spike-con -v "$HERE/scripts:/scripts:ro,Z" "$IMAGE" sleep infinity >/dev/null

podman exec spike-pub bash /scripts/publisher.sh init
podman exec spike-con bash /scripts/consumer.sh install
podman exec spike-pub bash /scripts/publisher.sh release 1.1
podman exec spike-con bash /scripts/consumer.sh upgrade
podman exec spike-pub bash /scripts/publisher.sh netprobe || true
