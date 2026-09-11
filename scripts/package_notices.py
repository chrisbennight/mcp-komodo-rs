#!/usr/bin/env python3
"""Package dependency notices from the locked Linux Cargo graph, including build/test dependencies."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import re
import subprocess
import zipfile


ROOT = Path(__file__).resolve().parents[1]
TARGET = "x86_64-unknown-linux-gnu"
NOTICE_NAME = re.compile(r"^(license|licence|notice|copying|copyright)(?:$|[._-])", re.I)
PACKAGE_PART = re.compile(r"[A-Za-z0-9][A-Za-z0-9._+-]{0,127}\Z")
MAX_NOTICE_BYTES = 2 * 1024 * 1024
MAX_TOTAL_BYTES = 64 * 1024 * 1024


def contained_file(root: Path, relative: str) -> bytes:
    path = root / relative
    if Path(relative).is_absolute() or not path.resolve().is_relative_to(root.resolve()):
        raise ValueError("notice path escapes its package")
    if not path.is_file() or path.stat().st_size > MAX_NOTICE_BYTES:
        raise ValueError("notice file is missing or oversized")
    data = path.read_bytes()
    if not data or len(data) > MAX_NOTICE_BYTES:
        raise ValueError("notice file is empty or oversized")
    return data


def collect(metadata: dict, root: Path) -> tuple[dict, dict[str, bytes]]:
    overrides = json.loads((root / "licenses/overrides.json").read_text())
    overrides = {(item["name"], item["version"]): item for item in overrides}
    resolved = {node["id"] for node in metadata["resolve"]["nodes"]}
    packages = [p for p in metadata["packages"] if p["id"] in resolved and p["source"]]
    entries = []
    files = {}
    total = 0
    for package in sorted(packages, key=lambda p: (p["name"], p["version"])):
        name, version = package["name"], package["version"]
        if not PACKAGE_PART.fullmatch(name) or not PACKAGE_PART.fullmatch(version):
            raise ValueError("invalid package archive name")
        if package["source"] != "registry+https://github.com/rust-lang/crates.io-index":
            raise ValueError("notice collection requires a reviewed crates.io source")
        if not package["license"]:
            raise ValueError(f"missing declared license for {name} {version}")
        package_root = Path(package["manifest_path"]).parent
        paths = {p.relative_to(package_root).as_posix() for p in package_root.rglob("*")
                 if NOTICE_NAME.match(p.name) and not p.is_dir()}
        if package.get("license_file"):
            paths.add(package["license_file"])
        notice_files = {path: contained_file(package_root, path) for path in sorted(paths)}
        entry = {"name": name, "version": version, "declaredLicense": package["license"], "files": {}}
        override = overrides.get((name, version))
        if override:
            vcs = json.loads(contained_file(package_root, ".cargo_vcs_info.json"))
            if vcs["git"]["sha1"] != override["vcsCommit"]:
                raise ValueError(f"notice override revision differs for {name} {version}")
            entry["override"] = override
            for path in override["files"]:
                notice_files[f"upstream/{Path(path).name}"] = contained_file(root, path)
        if not notice_files:
            raise ValueError(f"no packaged notices for {name} {version}; review an exact-version override")
        for relative, data in notice_files.items():
            archive_path = f"{name}-{version}/{relative}"
            if any(part in ("..", "") for part in Path(archive_path).parts):
                raise ValueError("invalid notice archive path")
            total += len(data)
            if total > MAX_TOTAL_BYTES:
                raise ValueError("notice archive exceeds its size bound")
            files[archive_path] = data
            entry["files"][relative] = {"sha256": hashlib.sha256(data).hexdigest()}
        entries.append(entry)
    return {"target": TARGET, "scope": "resolved Cargo graph including build and test dependencies", "packages": entries}, files


def main() -> None:
    artifacts = ROOT / "artifacts"
    if not artifacts.resolve().is_relative_to(ROOT.resolve()):
        raise ValueError("artifact directory escapes repository")
    artifacts.mkdir(exist_ok=True)
    for name in ("dependency-notices.zip", "dependency-notices.zip.tmp",
                 "dependency-notices.json", "dependency-notices.sha256"):
        (artifacts / name).unlink(missing_ok=True)
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--manifest-path", str(ROOT / "Cargo.toml"),
        "--locked", "--all-features", "--filter-platform", TARGET, "--format-version", "1",
    ], cwd=ROOT))
    index, files = collect(metadata, ROOT)
    index["cargoLockSha256"] = hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest()
    index_bytes = (json.dumps(index, indent=2) + "\n").encode()
    archive = artifacts / "dependency-notices.zip"
    temporary = artifacts / "dependency-notices.zip.tmp"
    try:
        with zipfile.ZipFile(temporary, "w", zipfile.ZIP_DEFLATED) as bundle:
            for name, data in sorted({**files, "index.json": index_bytes}.items()):
                info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = 0o100644 << 16
                bundle.writestr(info, data)
        temporary.replace(archive)
    finally:
        temporary.unlink(missing_ok=True)
    (artifacts / "dependency-notices.json").write_bytes(index_bytes)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    (artifacts / "dependency-notices.sha256").write_text(f"{digest}  dependency-notices.zip\n")
    print(f"Packaged notices for {len(index['packages'])} dependencies")


if __name__ == "__main__":
    main()
