#!/usr/bin/env python3
"""Bundle validated release evidence and signed attestations for durable archival."""

from __future__ import annotations

import base64
import hashlib
import json
import os
import sys
from pathlib import Path
import zipfile

from publish_image import DIGEST, IMAGE, publication_tags


FILES = (
    "build-record.json", "sbom.cdx.json", "rust-sbom.cdx.json", "vulnerabilities.json",
    "dependency-notices.zip", "dependency-notices.json", "dependency-notices.sha256",
)
MAX_FILE_BYTES = 64 * 1024 * 1024
MAX_TOTAL_BYTES = 160 * 1024 * 1024


def read_file(path: Path, parent: Path) -> bytes:
    if not path.resolve().is_relative_to(parent.resolve()) or not path.is_file():
        raise ValueError("release evidence is missing or outside its permitted directory")
    with path.open("rb") as source:
        data = source.read(MAX_FILE_BYTES + 1)
    if not data or len(data) > MAX_FILE_BYTES:
        raise ValueError("release evidence is empty or oversized")
    return data


def check_subject(bundle: bytes, digest: str) -> None:
    # This checks identity only. Signature verification belongs to the official
    # attestation verifier; the bundle is retained intact for that verification.
    try:
        envelope = json.loads(bundle)["dsseEnvelope"]
        statement = json.loads(base64.b64decode(envelope["payload"], validate=True))
        expected = [{"name": IMAGE, "digest": {"sha256": digest.removeprefix("sha256:")}}]
        if statement["subject"] != expected:
            raise ValueError("attestation subject differs from the published image")
    except (KeyError, TypeError, ValueError):
        raise ValueError("attestation does not identify the published image") from None


def package(root: Path, environment: dict[str, str]) -> Path:
    publication_tags(environment)
    digest = environment.get("PUBLISHED_IMAGE_DIGEST", "")
    if not DIGEST.fullmatch(digest):
        raise ValueError("release evidence requires the verified registry manifest digest")
    artifacts = root / "artifacts"
    if not artifacts.resolve().is_relative_to(root.resolve()):
        raise ValueError("artifact directory escapes repository")
    for name in ("release-evidence.zip", "release-evidence.zip.tmp", "release-evidence.sha256"):
        if not (artifacts / name).resolve().is_relative_to(artifacts.resolve()):
            raise ValueError("release output escapes the artifact directory")
    files = {name: read_file(artifacts / name, artifacts) for name in FILES}
    record = json.loads(files["build-record.json"])
    if record.get("commit") != environment["GITHUB_SHA"] or record.get("sourceDirty") is not False:
        raise ValueError("build record does not identify the clean publication source")
    for name in ("sbom.cdx.json", "rust-sbom.cdx.json", "vulnerabilities.json"):
        if record.get("files", {}).get(name, {}).get("sha256") != hashlib.sha256(files[name]).hexdigest():
            raise ValueError("scan evidence differs from the build record")
    expected_notice = hashlib.sha256(files["dependency-notices.zip"]).hexdigest()
    if files["dependency-notices.sha256"].decode().strip() != f"{expected_notice}  dependency-notices.zip":
        raise ValueError("dependency notice checksum does not match")
    notices = json.loads(files["dependency-notices.json"])
    if notices.get("cargoLockSha256") != hashlib.sha256(read_file(root / "Cargo.lock", root)).hexdigest():
        raise ValueError("dependency notice index belongs to a different lockfile")
    runner_temp = Path(environment["RUNNER_TEMP"])
    for variable, name in [("PROVENANCE_BUNDLE", "provenance.sigstore.json"), ("SBOM_BUNDLE", "sbom.sigstore.json")]:
        data = read_file(Path(environment[variable]), runner_temp)
        check_subject(data, digest)
        files[name] = data
    if sum(map(len, files.values())) > MAX_TOTAL_BYTES:
        raise ValueError("release evidence exceeds its total size bound")
    index = {
        "sourceCommit": environment["GITHUB_SHA"], "sourceRef": environment["GITHUB_REF"],
        "image": f"{IMAGE}@{digest}", "platform": "linux/amd64",
        "files": {name: {"sha256": hashlib.sha256(data).hexdigest()} for name, data in files.items()},
        "verification": "Verify the retained Sigstore bundles with the official attestation verifier; the index itself is unsigned.",
    }
    files["release-record.json"] = (json.dumps(index, indent=2) + "\n").encode()
    archive = artifacts / "release-evidence.zip"
    temporary = artifacts / "release-evidence.zip.tmp"
    try:
        with zipfile.ZipFile(temporary, "w", zipfile.ZIP_DEFLATED) as bundle:
            for name, data in sorted(files.items()):
                info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = 0o100644 << 16
                bundle.writestr(info, data)
        temporary.replace(archive)
    finally:
        temporary.unlink(missing_ok=True)
    checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
    (artifacts / "release-evidence.sha256").write_text(f"{checksum}  release-evidence.zip\n")
    return archive


if __name__ == "__main__":
    try:
        package(Path(__file__).resolve().parents[1], dict(os.environ))
    except (ValueError, KeyError, OSError):
        print("Release evidence validation failed; inspect the workflow artifacts before retrying", file=sys.stderr)
        raise SystemExit(1) from None
    print("Release evidence bundle and checksum prepared")
