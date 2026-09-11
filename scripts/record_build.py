#!/usr/bin/env python3
"""Record source and local image identity alongside unsigned scan evidence."""

import hashlib
import json
from pathlib import Path
import subprocess


def output(*arguments: str) -> str:
    return subprocess.check_output(arguments, text=True).strip()


def main() -> None:
    root = Path(__file__).resolve().parents[1]
    artifacts = root / "artifacts"
    record = {
        "repository": "https://github.com/chrisbennight/mcp-komodo-rs",
        "commit": output("git", "-C", str(root), "rev-parse", "HEAD"),
        "tree": output("git", "-C", str(root), "rev-parse", "HEAD^{tree}"),
        "sourceDirty": bool(output("git", "-C", str(root), "status", "--porcelain")),
        "imageConfigDigest": output("docker", "image", "inspect", "--format", "{{.Id}}", "mcp-komodo-rs:ci"),
        "platform": "linux/amd64",
        "signed": False,
        "files": {},
    }
    for name in ("sbom.cdx.json", "rust-sbom.cdx.json", "vulnerabilities.json"):
        path = artifacts / name
        if path.exists():
            record["files"][name] = {"sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
    artifacts.mkdir(exist_ok=True)
    (artifacts / "build-record.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
