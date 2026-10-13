#!/usr/bin/env python3
"""Compare source and deployed MCP declarations without prescribing gateway policy."""
from __future__ import annotations

import argparse
import json
from pathlib import Path

MAX_BYTES = 4 * 1024 * 1024
FIELDS = {"description": str, "inputSchema": dict, "outputSchema": dict, "annotations": dict}
HINTS = ("readOnlyHint", "destructiveHint", "idempotentHint", "openWorldHint")
OPTIONAL_FIELDS = ("title", "icons", "execution")


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


def index(document: dict) -> dict:
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
        for field, kind in FIELDS.items():
            if type(tool.get(field)) is not kind:
                raise ValueError("MCP declaration is missing or malformed")
        if any(type(tool["annotations"].get(hint)) is not bool for hint in HINTS):
            raise ValueError("standard behavior hints must be explicit booleans")
        result[name] = tool
    if not result:
        raise ValueError("catalog is empty")
    return result


def compare(expected: dict, observed: dict) -> list[str]:
    source = index(expected)
    live = index(observed)
    errors = []
    if source.keys() != live.keys():
        errors.append(f"coverage differs: missing={len(source.keys() - live.keys())}, unexpected={len(live.keys() - source.keys())}")
    for name in sorted(source.keys() & live.keys()):
        want, got = source[name], live[name]
        for field in (*FIELDS, *OPTIONAL_FIELDS):
            if json.dumps(want.get(field), sort_keys=True) != json.dumps(got.get(field), sort_keys=True):
                errors.append(f"{name}: {field} differs")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("observed", type=Path)
    args = parser.parse_args()
    try:
        errors = compare(load(args.source), load(args.observed))
    except (ValueError, OSError):
        parser.exit(2, "invalid or unreadable catalog input\n")
    if errors:
        print("\n".join(errors))
        return 1
    print("catalog coverage and standard MCP declarations match")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
