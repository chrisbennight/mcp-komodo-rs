#!/usr/bin/env python3
"""Compare a source contract with an operator-supplied gateway catalog projection."""
from __future__ import annotations

import argparse
import json
from pathlib import Path

MAX_BYTES = 4 * 1024 * 1024
FIELDS = {"risk": str, "side_effects": bool, "pii": bool}


def unique_object(pairs: list[tuple[str, object]]) -> dict:
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON object key")
        result[key] = value
    return result


def load(path: Path) -> dict:
    with path.open("rb") as stream:
        data = stream.read(MAX_BYTES + 1)
    if len(data) > MAX_BYTES:
        raise ValueError("catalog file exceeds the size bound")
    return json.loads(data, object_pairs_hook=unique_object)


def index(document: dict, *, source: bool) -> dict:
    if not isinstance(document, dict) or not isinstance(document.get("tools"), list):
        raise ValueError("catalog must contain a tools array")
    result = {}
    for tool in document["tools"]:
        if not isinstance(tool, dict):
            raise ValueError("tool entry must be an object")
        name = tool.get("name")
        if not isinstance(name, str) or not name.startswith("komodo.") or len(name) > 128:
            raise ValueError("tool name is not a bounded Komodo identity")
        if name in result:
            raise ValueError("duplicate tool identity")
        governance = tool.get("governance")
        if not isinstance(governance, dict):
            raise ValueError("tool governance is missing")
        for field, kind in FIELDS.items():
            if type(governance.get(field)) is not kind:
                raise ValueError("tool classification is missing or malformed")
        if governance["risk"] not in ("low", "medium", "high"):
            raise ValueError("tool risk is unsupported")
        if source:
            if type(tool.get("requiresReview")) is not bool:
                raise ValueError("source approval claim is missing")
        elif (type(governance.get("requires_approval")) is not bool
              or governance.get("requires_approval_known") is not True):
            raise ValueError("gateway approval classification is unknown or malformed")
        result[name] = tool
    if not result:
        raise ValueError("catalog is empty")
    return result


def compare(expected: dict, observed: dict, approval_mode: str) -> list[str]:
    if approval_mode not in ("per_call", "policy_only"):
        raise ValueError("approval mode must be explicit")
    source = index(expected, source=True)
    live = index(observed, source=False)
    errors = []
    if source.keys() != live.keys():
        errors.append(f"coverage differs: missing={len(source.keys() - live.keys())}, unexpected={len(live.keys() - source.keys())}")
    for name in sorted(source.keys() & live.keys()):
        want, got = source[name], live[name]
        for field in FIELDS:
            if want["governance"][field] != got["governance"][field]:
                errors.append(f"{name}: {field} differs")
        review = want["requiresReview"] if approval_mode == "per_call" else False
        if got["governance"]["requires_approval"] != review:
            errors.append(f"{name}: imported approval requirement differs")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("observed", type=Path)
    parser.add_argument("--approval-mode", required=True, choices=("per_call", "policy_only"))
    args = parser.parse_args()
    try:
        errors = compare(load(args.source), load(args.observed), args.approval_mode)
    except (ValueError, OSError):
        parser.exit(2, "invalid or unreadable catalog input\n")
    if errors:
        print("\n".join(errors))
        return 1
    print("catalog coverage and imported classifications match")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
