#!/usr/bin/env python3
"""Publish an already tested image from a trusted GitHub push."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
from collections.abc import Mapping
from pathlib import Path


REPOSITORY = "chrisbennight/mcp-komodo-rs"
IMAGE = f"ghcr.io/{REPOSITORY}"
LOCAL_IMAGE = "mcp-komodo-rs:ci"
VERSION = re.compile(r"v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?")
DIGEST = re.compile(r"sha256:[0-9a-f]{64}\Z")


def read_docker_json(command: list[str], environment: Mapping[str, str]) -> object:
    result = subprocess.run(command, env=environment, check=True, capture_output=True, timeout=60)
    if len(result.stdout) > 65536:
        raise ValueError("image metadata exceeds its supported size")
    try:
        return json.loads(result.stdout)
    except (ValueError, TypeError):
        raise ValueError("image metadata has an unsupported format") from None


def verified_registry_digest(tag: str, local_id: str, environment: Mapping[str, str]) -> str:
    descriptor = read_docker_json(
        ["docker", "buildx", "imagetools", "inspect", tag, "--format", "{{json .Manifest}}"], environment,
    )
    digest = descriptor.get("digest") if isinstance(descriptor, dict) else None
    if not isinstance(digest, str) or not DIGEST.fullmatch(digest):
        raise ValueError("registry did not report a supported manifest digest")
    manifest = read_docker_json(
        ["docker", "buildx", "imagetools", "inspect", f"{IMAGE}@{digest}", "--raw"], environment,
    )
    if not isinstance(manifest, dict) or not isinstance(manifest.get("config"), dict):
        raise ValueError("publication requires a single-platform image manifest")
    if manifest["config"].get("digest") != local_id:
        raise ValueError("published manifest does not identify the tested local image")
    return digest


def publication_tags(environment: Mapping[str, str]) -> list[str]:
    if (
        environment.get("GITHUB_EVENT_NAME") != "push"
        or environment.get("GITHUB_REPOSITORY") != REPOSITORY
    ):
        raise ValueError("publication requires a push to the canonical repository")
    sha = environment.get("GITHUB_SHA", "")
    if not re.fullmatch(r"[0-9a-f]{40}", sha):
        raise ValueError("publication requires a full commit SHA")
    ref = environment.get("GITHUB_REF", "")
    if ref == "refs/heads/main":
        release_tag = "latest"
    elif ref.startswith("refs/tags/"):
        release_tag = ref.removeprefix("refs/tags/")
        if len(release_tag) > 128 or not VERSION.fullmatch(release_tag):
            raise ValueError("release tags must be vMAJOR.MINOR.PATCH with an optional prerelease")
    else:
        raise ValueError("publication requires main or a version tag")
    return [f"{IMAGE}:sha-{sha}", f"{IMAGE}:{release_tag}"]


def publish(environment: Mapping[str, str], expected_image_digest: str) -> str:
    tags = publication_tags(environment)
    if not DIGEST.fullmatch(expected_image_digest):
        raise ValueError("publication requires the tested image configuration digest")
    actor = environment.get("GITHUB_ACTOR", "")
    token = environment.get("GITHUB_TOKEN", "")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_\[\]-]{0,99}", actor) or not token:
        raise ValueError("publication requires a GitHub actor and workflow token")
    # Only docker login receives the token, on stdin. Other subprocesses cannot
    # inherit it, and Docker's credential file is removed when this call exits.
    child_environment = dict(environment)
    child_environment.pop("GITHUB_TOKEN", None)
    with tempfile.TemporaryDirectory(prefix="mcp-komodo-docker-") as config:
        child_environment["DOCKER_CONFIG"] = config
        local_id = read_docker_json(
            ["docker", "image", "inspect", LOCAL_IMAGE, "--format", "{{json .Id}}"], child_environment,
        )
        if not isinstance(local_id, str) or not DIGEST.fullmatch(local_id):
            raise ValueError("local image did not report a supported configuration digest")
        if local_id != expected_image_digest:
            raise ValueError("local publication image differs from the tested image")
        for tag in tags:
            subprocess.run(["docker", "tag", LOCAL_IMAGE, tag], env=child_environment, check=True)
        subprocess.run(
            ["docker", "login", "ghcr.io", "--username", actor, "--password-stdin"],
            input=token, text=True, env=child_environment, check=True,
        )
        for tag in tags:
            subprocess.run(["docker", "push", tag], env=child_environment, check=True)
        digests = {verified_registry_digest(tag, local_id, child_environment) for tag in tags}
        if len(digests) != 1:
            raise ValueError("published tags do not identify the same manifest")
        return digests.pop()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="validate the event without registry access")
    args = parser.parse_args()
    try:
        if args.check:
            for tag in publication_tags(os.environ):
                print(tag)
        else:
            record_path = Path(__file__).resolve().parents[1] / "artifacts/build-record.json"
            with record_path.open("rb") as source:
                raw_record = source.read(65537)
            if len(raw_record) > 65536:
                raise ValueError("build record exceeds its supported size")
            record = json.loads(raw_record)
            if not isinstance(record, dict) or record.get("sourceDirty") is not False or record.get("commit") != os.environ.get("GITHUB_SHA"):
                raise ValueError("build record does not identify the clean publication source")
            expected = record.get("imageConfigDigest")
            if not isinstance(expected, str):
                raise ValueError("build record has no tested image digest")
            digest = publish(os.environ, expected)
            if output := os.environ.get("GITHUB_OUTPUT"):
                with open(output, "a", encoding="utf-8") as destination:
                    destination.write(f"image_digest={digest}\n")
            print(f"Published image: {IMAGE}@{digest}")
    except ValueError as error:
        print(str(error), file=sys.stderr)
        return 1
    except subprocess.CalledProcessError:
        print("Image publication failed; inspect the workflow step before retrying", file=sys.stderr)
        return 1
    except subprocess.TimeoutExpired:
        print("Image verification timed out; publication may have succeeded; inspect before retrying", file=sys.stderr)
        return 1
    except OSError:
        print("Publication input or output is unavailable; inspect the workflow before retrying", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
