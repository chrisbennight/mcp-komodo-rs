#!/usr/bin/env python3
"""Measure static catalog bytes, model-tokenizer tokens, and binary export latency."""

import argparse
import importlib.metadata
import json
import statistics
import subprocess
import time
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--model", default="gpt-4o-mini-2024-07-18")
    args = parser.parse_args()
    import tiktoken  # Optional measurement tool; not a service dependency.

    encoding = tiktoken.encoding_for_model(args.model)
    binary = str(args.binary.resolve(strict=True))
    rows = []
    for profile in ("full", "status", "read-only", "operations"):
        samples = []
        raw = None
        for _ in range(11):
            started = time.perf_counter_ns()
            output = subprocess.run(
                [binary, "--emit-tools-json", "--tool-profile", profile],
                env={}, capture_output=True, timeout=15, check=True,
            )
            samples.append((time.perf_counter_ns() - started) / 1_000_000)
            assert not output.stderr
            if raw is not None:
                assert raw == output.stdout, "catalog must be immutable"
            raw = output.stdout
        catalog = json.loads(raw)
        text = raw.decode("utf-8")
        metadata = json.dumps(
            [{"name": tool["name"], "description": tool.get("description", "")}
             for tool in catalog["tools"]], separators=(",", ":"), ensure_ascii=False,
        )
        rows.append({
            "profile": profile, "tools": len(catalog["tools"]),
            "catalogBytes": len(raw), "catalogTokens": len(encoding.encode_ordinary(text)),
            "nameDescriptionBytes": len(metadata.encode("utf-8")),
            "nameDescriptionTokens": len(encoding.encode_ordinary(metadata)),
            "exportMedianMilliseconds": statistics.median(samples),
        })
    print(json.dumps({
        "model": args.model, "encoding": encoding.name,
        "tokenizerVersion": importlib.metadata.version("tiktoken"),
        "tokenScope": "Exact tokenizer counts for exported text; excludes message framing, billing, and gateway wrappers",
        "latencyScope": "Local process startup, schema initialization, serialization, and output; excludes network",
        "nameDescriptionScope": "Local projection illustrating progressive discovery; not a measured gateway response",
        "profiles": rows,
    }, indent=2))


if __name__ == "__main__":
    main()
