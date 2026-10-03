#!/usr/bin/env bash
set -euo pipefail

rust=false
deps=false
python=false
docs=false
image=false
case "${GITHUB_EVENT_NAME:?event is required}" in
  workflow_dispatch|schedule) full=true ;;
  push|pull_request)
    full=false
    if [[ "$GITHUB_EVENT_NAME" == push && "${GITHUB_REF:-}" == refs/tags/* ]]; then full=true; fi
    ;;
  *) echo 'Unsupported CI event' >&2; exit 1 ;;
esac
if [[ "$full" == true ]]; then
  rust=true; deps=true; python=true; docs=true; image=true
else
  [[ "${BASE_SHA:-}" =~ ^[0-9a-f]{40}$ ]] || { echo 'A full base commit is required' >&2; exit 1; }
  changed_files="$(mktemp)"
  trap 'rm -f "$changed_files"' EXIT
  if [[ "$GITHUB_EVENT_NAME" == pull_request ]]; then
    git diff --name-only --no-renames -z "$BASE_SHA...HEAD" >"$changed_files"
  else
    git diff --name-only --no-renames -z "$BASE_SHA" HEAD >"$changed_files"
  fi
  while IFS= read -r -d '' path; do
    case "$path" in
      scripts/ci-scope.sh) rust=true; deps=true; python=true; docs=true; image=true ;;

      .github/workflows/*) rust=true; deps=true; python=true; docs=true; image=true ;;
      Cargo.toml|Cargo.lock|rust-toolchain.toml|rust-toolchain|crates/*/Cargo.toml) rust=true; deps=true; image=true ;;
      .cargo/*) rust=true; image=true ;;
      rustfmt.toml|.rustfmt.toml|clippy.toml|.clippy.toml) rust=true ;;
      crates/*/tests/*|crates/*/benches/*|crates/*/src/*_tests.rs) rust=true ;;
      crates/*/*.md|licenses/*.md) docs=true ;;
      crates/*) rust=true; image=true ;;
      Dockerfile|.dockerignore|scripts/test_image.sh|scripts/scan_image.sh|scripts/record_build.py|scripts/publish_image.py|scripts/package_release_evidence.py|scripts/package_notices.py|licenses/*|LICENSE|THIRD_PARTY_NOTICES.md)
        image=true ;;
      deny.toml) deps=true ;;
    esac
    case "$path" in
      *.md) docs=true ;;
      scripts/check_docs.py) docs=true; python=true ;;
      scripts/*) python=true ;;
    esac
    if [[ ! -e "$path" ]]; then docs=true; fi
  done <"$changed_files"
fi
printf 'rust=%s\ndeps=%s\npython=%s\ndocs=%s\nimage=%s\n' \
  "$rust" "$deps" "$python" "$docs" "$image" >>"${GITHUB_OUTPUT:?output file is required}"
