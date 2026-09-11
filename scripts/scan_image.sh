#!/usr/bin/env bash
# Scan only the locally built test image; never give the scanner a Docker socket.
set -euo pipefail
cd "$(dirname "$0")/.."
scan_dir=$(mktemp -d)
scanner_user="$(id -u):$(id -g)"
trap 'rm -rf "$scan_dir"' EXIT
mkdir -p artifacts
rm -f artifacts/sbom.cdx.json artifacts/rust-sbom.cdx.json artifacts/vulnerabilities.json artifacts/build-record.json
scanner='aquasec/trivy:0.74.0@sha256:62b1e65e8869bc4b4c6aa4fa2b21595256c7c2f6018a9d9ad61caf87187c1969'
mkdir "$scan_dir/source"
cp Cargo.lock "$scan_dir/source/Cargo.lock"
docker run --rm --user "$scanner_user" --cap-drop ALL --security-opt no-new-privileges \
  -v "$scan_dir:/scan" "$scanner" fs /scan/source \
  --cache-dir /scan/cache --scanners vuln --format cyclonedx \
  --output /scan/rust-sbom.cdx.json
cp "$scan_dir/rust-sbom.cdx.json" artifacts/rust-sbom.cdx.json
docker save mcp-komodo-rs:ci -o "$scan_dir/image.tar"
docker run --rm --user "$scanner_user" --cap-drop ALL --security-opt no-new-privileges \
  -v "$scan_dir:/scan" "$scanner" image --input /scan/image.tar \
  --cache-dir /scan/cache --scanners vuln --format cyclonedx \
  --output /scan/sbom.cdx.json
cp "$scan_dir/sbom.cdx.json" artifacts/sbom.cdx.json
scan_status=0
docker run --rm --user "$scanner_user" --cap-drop ALL --security-opt no-new-privileges \
  -v "$scan_dir:/scan" "$scanner" image --input /scan/image.tar \
  --cache-dir /scan/cache --scanners vuln --format json \
  --severity HIGH,CRITICAL --exit-code 1 --output /scan/vulnerabilities.json || scan_status=$?
if [ -f "$scan_dir/vulnerabilities.json" ]; then
  cp "$scan_dir/vulnerabilities.json" artifacts/vulnerabilities.json
fi
python3 scripts/record_build.py
exit "$scan_status"
