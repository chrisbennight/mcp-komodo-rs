from __future__ import annotations

import subprocess
import sys
import unittest
from pathlib import Path
from unittest.mock import patch


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import publish_image


class PublicationTests(unittest.TestCase):
    def environment(self, **overrides: str) -> dict[str, str]:
        return {
            "GITHUB_EVENT_NAME": "push",
            "GITHUB_REPOSITORY": "chrisbennight/mcp-komodo-rs",
            "GITHUB_SHA": "a" * 40,
            "GITHUB_REF": "refs/heads/main",
            "GITHUB_ACTOR": "release-bot[bot]",
            "GITHUB_TOKEN": "test-only-workflow-token",
            **overrides,
        }

    def test_main_and_version_tags_have_separate_aliases(self) -> None:
        image = "ghcr.io/chrisbennight/mcp-komodo-rs"
        for ref, expected in [
            ("refs/heads/main", "latest"),
            ("refs/tags/v0.2.0", "v0.2.0"),
            ("refs/tags/v0.2.0-rc.1", "v0.2.0-rc.1"),
        ]:
            with self.subTest(ref=ref):
                tags = publish_image.publication_tags(self.environment(GITHUB_REF=ref))
                self.assertEqual(tags, [f"{image}:sha-{'a' * 40}", f"{image}:{expected}"])

    def test_invalid_context_is_rejected_before_any_docker_call(self) -> None:
        cases = [
            {"GITHUB_EVENT_NAME": event}
            for event in ["pull_request", "pull_request_target", "workflow_dispatch", "workflow_run", ""]
        ] + [
            {"GITHUB_REPOSITORY": "contributor/mcp-komodo-rs"},
            {"GITHUB_SHA": "short"},
            {"GITHUB_SHA": "$(command)"},
            {"GITHUB_REF": "refs/heads/feature"},
            {"GITHUB_REF": "refs/tags/latest"},
            {"GITHUB_REF": "refs/tags/v01.2.3"},
            {"GITHUB_REF": "refs/tags/v1.2.3+metadata"},
            {"GITHUB_REF": "refs/tags/v1.2.3-" + "x" * 128},
            {"GITHUB_ACTOR": "--username=other"},
            {"GITHUB_TOKEN": ""},
        ]
        for override in cases:
            with self.subTest(override=override), patch.object(publish_image.subprocess, "run") as run:
                with self.assertRaises(ValueError):
                    publish_image.publish(self.environment(**override))
                run.assert_not_called()

    def test_token_uses_stdin_only_and_auth_file_is_removed(self) -> None:
        calls = []
        configs = []

        def docker(command, **kwargs):
            calls.append((command, kwargs))
            config = Path(kwargs["env"]["DOCKER_CONFIG"])
            configs.append(config)
            self.assertTrue(config.is_dir())
            self.assertNotIn("GITHUB_TOKEN", kwargs["env"])
            self.assertNotIn("test-only-workflow-token", " ".join(command))
            if command[1] == "login":
                self.assertEqual(kwargs["input"], "test-only-workflow-token")
                self.assertEqual(command[-1], "--password-stdin")
                (config / "config.json").write_text("test-only-credential-file")
            else:
                self.assertNotIn("input", kwargs)
            return subprocess.CompletedProcess(command, 0)

        with patch.object(publish_image.subprocess, "run", side_effect=docker):
            publish_image.publish(self.environment())
        self.assertEqual([command[1] for command, _ in calls], ["image", "tag", "tag", "login", "push", "push"])
        self.assertTrue(all(not config.exists() for config in configs))

    def test_failed_push_is_not_retried_and_credentials_are_cleaned(self) -> None:
        pushed = []
        configs = []

        def docker(command, **kwargs):
            configs.append(Path(kwargs["env"]["DOCKER_CONFIG"]))
            if command[1] == "push":
                pushed.append(command[-1])
                raise subprocess.CalledProcessError(1, command)
            return subprocess.CompletedProcess(command, 0)

        with patch.object(publish_image.subprocess, "run", side_effect=docker):
            with self.assertRaises(subprocess.CalledProcessError):
                publish_image.publish(self.environment())
        self.assertEqual(len(pushed), 1)
        self.assertTrue(all(not config.exists() for config in configs))


if __name__ == "__main__":
    unittest.main()
