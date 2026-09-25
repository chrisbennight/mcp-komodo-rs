from __future__ import annotations

import subprocess
import json
import sys
import unittest
from pathlib import Path
from unittest.mock import patch


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import publish_image


class PublicationTests(unittest.TestCase):
    local_id = "sha256:" + "b" * 64
    registry_digest = "sha256:" + "c" * 64

    def metadata(self, command):
        if command[1] == "image":
            return json.dumps(self.local_id).encode()
        if command[1] == "buildx":
            if command[-1] == "--raw":
                return json.dumps({"config": {"digest": self.local_id}}).encode()
            return json.dumps({"digest": self.registry_digest}).encode()
        return b""

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
                    publish_image.publish(self.environment(**override), self.local_id)
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
            return subprocess.CompletedProcess(command, 0, stdout=self.metadata(command))

        with patch.object(publish_image.subprocess, "run", side_effect=docker):
            digest = publish_image.publish(self.environment(), self.local_id)
        self.assertEqual(digest, self.registry_digest)
        self.assertNotEqual(digest, self.local_id)
        self.assertEqual([command[1] for command, _ in calls], ["image", "tag", "tag", "login", "push", "push", "buildx", "buildx", "buildx", "buildx"])
        pinned = [command[4] for command, _ in calls if command[-1] == "--raw"]
        self.assertEqual(pinned, [f"{publish_image.IMAGE}@{self.registry_digest}"] * 2)
        self.assertTrue(all(not config.exists() for config in configs))

    def test_failed_push_is_not_retried_and_credentials_are_cleaned(self) -> None:
        pushed = []
        configs = []

        def docker(command, **kwargs):
            configs.append(Path(kwargs["env"]["DOCKER_CONFIG"]))
            if command[1] == "push":
                pushed.append(command[-1])
                raise subprocess.CalledProcessError(1, command)
            return subprocess.CompletedProcess(command, 0, stdout=self.metadata(command))

        with patch.object(publish_image.subprocess, "run", side_effect=docker):
            with self.assertRaises(subprocess.CalledProcessError):
                publish_image.publish(self.environment(), self.local_id)
        self.assertEqual(len(pushed), 1)
        self.assertTrue(all(not config.exists() for config in configs))

    def test_registry_verification_rejects_wrong_image_and_inconsistent_tags(self) -> None:
        for defect in ("wrong-config", "invalid-digest", "different-tags", "invalid-json", "oversized"):
            calls = []
            descriptors = 0

            def docker(command, **kwargs):
                nonlocal descriptors
                calls.append(command)
                output = self.metadata(command)
                if command[1] == "buildx":
                    if command[-1] == "--raw" and defect == "wrong-config":
                        output = json.dumps({"config": {"digest": "sha256:" + "d" * 64}}).encode()
                    elif command[-1] != "--raw":
                        descriptors += 1
                        if defect == "invalid-digest":
                            output = b'{"digest":"unexpected"}'
                        elif defect == "invalid-json":
                            output = b'bad metadata'
                        elif defect == "oversized":
                            output = b' ' * 65537
                        elif defect == "different-tags" and descriptors == 2:
                            output = json.dumps({"digest": "sha256:" + "e" * 64}).encode()
                return subprocess.CompletedProcess(command, 0, stdout=output)

            with self.subTest(defect=defect), patch.object(publish_image.subprocess, "run", side_effect=docker):
                with self.assertRaises(ValueError):
                    publish_image.publish(self.environment(), self.local_id)
                self.assertEqual(sum(command[1] == "push" for command in calls), 2)

    def test_invalid_local_digest_fails_before_tagging_or_pushing(self) -> None:
        with patch.object(publish_image.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, stdout=b'"bad"')) as run:
            with self.assertRaises(ValueError):
                publish_image.publish(self.environment(), self.local_id)
            self.assertEqual(run.call_count, 1)

    def test_changed_local_image_is_rejected_before_any_registry_write(self) -> None:
        with patch.object(publish_image.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, stdout=json.dumps(self.local_id).encode())) as run:
            with self.assertRaises(ValueError):
                publish_image.publish(self.environment(), "sha256:" + "f" * 64)
            self.assertEqual(run.call_count, 1)


if __name__ == "__main__":
    unittest.main()
