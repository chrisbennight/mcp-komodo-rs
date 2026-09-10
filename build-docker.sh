#!/usr/bin/env bash
# Local image build for smoke testing. CI owns authenticated publication.
set -euo pipefail

cd "$(dirname "$0")"

image_tag="${TAG:-komodo-mcp-rs:dev}"
exec docker build \
  --tag "$image_tag" \
  --progress=plain \
  "$@" \
  .
