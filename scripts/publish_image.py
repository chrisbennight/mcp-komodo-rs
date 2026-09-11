#!/usr/bin/env python3
"""Publish an already tested image from a trusted GitHub push."""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import tempfile
from collections.abc import Mapping


REPOSITORY = "chrisbennight/mcp-komodo-rs"
IMAGE = f"ghcr.io/{REPOSITORY}"
LOCAL_IMAGE = "mcp-komodo-rs:ci"
VERSION = re.compile(r"v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?")


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


def publish(environment: Mapping[str, str]) -> None:
    tags = publication_tags(environment)
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
        subprocess.run(
            ["docker", "image", "inspect", LOCAL_IMAGE],
            env=child_environment, check=True, stdout=subprocess.DEVNULL,
        )
        for tag in tags:
            subprocess.run(["docker", "tag", LOCAL_IMAGE, tag], env=child_environment, check=True)
        subprocess.run(
            ["docker", "login", "ghcr.io", "--username", actor, "--password-stdin"],
            input=token, text=True, env=child_environment, check=True,
        )
        for tag in tags:
            subprocess.run(["docker", "push", tag], env=child_environment, check=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="validate the event without registry access")
    args = parser.parse_args()
    try:
        if args.check:
            for tag in publication_tags(os.environ):
                print(tag)
        else:
            publish(os.environ)
    except ValueError as error:
        print(str(error), file=sys.stderr)
        return 1
    except subprocess.CalledProcessError:
        print("Image publication failed; inspect the workflow step before retrying", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
