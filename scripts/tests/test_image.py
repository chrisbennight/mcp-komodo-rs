from __future__ import annotations

import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
FAKE_DOCKER = """#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
record = {'args': args}
if args[0] == 'create':
    record['fixture_secrets'] = (
        os.environ['KOMODO_MCP_READ_API_SECRET'] == 'smoke-read-secret'
        and os.environ['KOMODO_MCP_ADMIN_API_SECRET'] == 'smoke-admin-secret'
    )
with open(os.environ['DOCKER_TEST_LOG'], 'a') as log:
    log.write(json.dumps(record) + '\\n')
if args[0] == os.environ.get('DOCKER_TEST_FAIL'):
    sys.exit(1)
"""


class ImageSmokeTests(unittest.TestCase):
    def run_image_check(self, fail: str = ""):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            docker = root / "docker"
            docker.write_text(FAKE_DOCKER)
            docker.chmod(0o755)
            sleep = root / "sleep"
            sleep.write_text("#!/bin/sh\nexit 0\n")
            sleep.chmod(0o755)
            log = root / "docker.jsonl"
            environment = {
                **os.environ,
                "PATH": f"{root}:{os.environ['PATH']}",
                "DOCKER_TEST_LOG": str(log),
                "DOCKER_TEST_FAIL": fail,
                "KOMODO_MCP_READ_API_SECRET": "inherited-test-value",
                "KOMODO_MCP_ADMIN_API_SECRET": "inherited-test-value",
            }
            result = subprocess.run(
                ["bash", str(ROOT / "scripts/test_image.sh")],
                env=environment, capture_output=True, text=True,
            )
            return result, [json.loads(line) for line in log.read_text().splitlines()]

    def test_smoke_has_no_network_and_never_forwards_inherited_secrets(self) -> None:
        result, calls = self.run_image_check()
        self.assertEqual(result.returncode, 0, result.stderr)
        created = next(call for call in calls if call["args"][0] == "create")
        args = created["args"]
        self.assertEqual(args[args.index("--network") + 1], "none")
        self.assertIn("--read-only", args)
        self.assertNotIn("-p", args)
        self.assertTrue(created["fixture_secrets"])
        for index, arg in enumerate(args):
            if arg == "-e":
                self.assertNotIn("=", args[index + 1])
        self.assertEqual(calls[-1]["args"][0:2], ["rm", "-f"])

    def test_unhealthy_image_fails_and_is_removed(self) -> None:
        result, calls = self.run_image_check("exec")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("did not become healthy", result.stderr)
        self.assertEqual(calls[-1]["args"][0:2], ["rm", "-f"])

    def test_cleanup_failure_is_not_reported_as_success(self) -> None:
        result, _ = self.run_image_check("rm")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Failed to remove", result.stderr)

    def test_failed_create_still_attempts_cleanup_of_the_unique_name(self) -> None:
        result, calls = self.run_image_check("create")
        self.assertNotEqual(result.returncode, 0)
        args = next(call["args"] for call in calls if call["args"][0] == "create")
        name = args[args.index("--name") + 1]
        self.assertEqual(calls[-1]["args"], ["rm", "-f", name])


if __name__ == "__main__":
    unittest.main()
