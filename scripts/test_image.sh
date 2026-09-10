#!/usr/bin/env bash
# Build from public sources and smoke only the isolated liveness endpoint.
set -euo pipefail
cd "$(dirname "$0")/.."

fixture="$(mktemp -d)"
container_name="mcp-komodo-smoke-${fixture##*/}"
container_created=0
cleanup() {
  local status=$?
  if [ "$container_created" = 1 ]; then
    if ! docker rm -f "$container_name" >/dev/null; then
      echo "Failed to remove the image smoke container" >&2
      status=1
    fi
  fi
  rm -rf "$fixture"
  exit "$status"
}
trap cleanup EXIT

syntax="$(sed -n 's/^# syntax=//p' Dockerfile)"
if [ -z "$syntax" ]; then
  echo "Dockerfile syntax source is missing" >&2
  exit 1
fi
printf '# syntax=%s\n\nFROM scratch\nCOPY . /context\n' "$syntax" >"$fixture/Dockerfile"
printf '.*\n' >"$fixture/.dockerignore"
printf 'ignored\n' >"$fixture/.ignored"
printf 'included\n' >"$fixture/included"
docker build --check --progress=plain \
  --build-arg 'BUILDKIT_DOCKERFILE_CHECK=error=true' "$fixture"

docker build --tag mcp-komodo-rs:ci --progress=plain \
  --label org.opencontainers.image.source=https://github.com/chrisbennight/mcp-komodo-rs \
  --label org.opencontainers.image.licenses=MIT .

# Explicit sentinels prevent an inherited operator environment from reaching
# the smoke container. No network is available and no host port is published.
export KOMODO_MCP_READ_API_KEY=smoke-read-key
export KOMODO_MCP_READ_API_SECRET=smoke-read-secret
export KOMODO_MCP_ADMIN_API_KEY=smoke-admin-key
export KOMODO_MCP_ADMIN_API_SECRET=smoke-admin-secret
export KOMODO_MCP_GATEWAY_BEARER_CURRENT=0123456789abcdef0123456789abcdef
export KOMODO_MCP_IDENTITY_JWKS_URL=http://127.0.0.1:65533/jwks
export KOMODO_MCP_IDENTITY_ISSUER=https://gateway.test
export KOMODO_MCP_IDENTITY_ACTOR=gateway.test
container_created=1
docker create --name "$container_name" --network none \
  --read-only --cap-drop ALL --security-opt no-new-privileges \
  -e KOMODO_MCP_READ_API_KEY \
  -e KOMODO_MCP_READ_API_SECRET \
  -e KOMODO_MCP_ADMIN_API_KEY \
  -e KOMODO_MCP_ADMIN_API_SECRET \
  -e KOMODO_MCP_GATEWAY_BEARER_CURRENT \
  -e KOMODO_MCP_IDENTITY_JWKS_URL \
  -e KOMODO_MCP_IDENTITY_ISSUER \
  -e KOMODO_MCP_IDENTITY_ACTOR \
  mcp-komodo-rs:ci >/dev/null
docker start "$container_name" >/dev/null
for _ in $(seq 1 30); do
  if docker exec "$container_name" /komodo-mcp-rs --healthcheck >/dev/null 2>&1; then
    exit 0
  fi
  sleep 1
done
echo "Image liveness check did not become healthy" >&2
docker logs "$container_name"
exit 1
