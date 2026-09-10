from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]


class WorkflowContractTests(unittest.TestCase):
    def test_external_actions_are_pinned_to_immutable_commits(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "ci.yml").read_text(
            encoding="utf-8"
        )
        action_refs = re.findall(r"^\s*-?\s*uses:\s+([^#\s]+)", workflow, re.MULTILINE)

        self.assertGreaterEqual(len(action_refs), 1)
        for action_ref in action_refs:
            self.assertRegex(action_ref, r"@[0-9a-f]{40}$")

    def test_rust_builder_image_is_pinned_to_an_immutable_digest(self) -> None:
        dockerfile = (ROOT / "Dockerfile").read_text(encoding="utf-8")

        self.assertRegex(
            dockerfile,
            r"(?m)^FROM rust:\$\{RUST_VERSION\}-slim-bookworm@sha256:[0-9a-f]{64} AS builder$",
        )

    def test_image_smoke_passes_fixture_secrets_by_environment_name(self) -> None:
        workflow = (ROOT / "scripts" / "test_image.sh").read_text(
            encoding="utf-8"
        )

        self.assertNotIn("_FILE", workflow)
        self.assertNotIn("docker cp", workflow)
        for command in ["docker create", "docker start"]:
            self.assertIn(command, workflow)
        for variable in [
            "KOMODO_MCP_READ_API_KEY",
            "KOMODO_MCP_READ_API_SECRET",
            "KOMODO_MCP_ADMIN_API_KEY",
            "KOMODO_MCP_ADMIN_API_SECRET",
            "KOMODO_MCP_GATEWAY_BEARER_CURRENT",
        ]:
            self.assertIn(f"-e {variable} \\", workflow)

    def test_only_the_publication_job_has_package_write_permission(self) -> None:
        workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        ordinary, publication = workflow.split("\n  publish:\n")
        self.assertIn("permissions:\n  contents: read\n", ordinary)
        self.assertNotIn("packages: write", ordinary)
        self.assertNotIn("GITHUB_TOKEN", ordinary)
        self.assertIn("github.event_name == 'push'", publication)
        self.assertIn("github.repository == 'chrisbennight/mcp-komodo-rs'", publication)
        self.assertIn("needs: test", publication)
        self.assertIn("packages: write", publication)
        self.assertLess(
            publication.index("bash scripts/test_image.sh"),
            publication.index("GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}"),
        )
        for forbidden in ["pull_request_target", "workflow_run", "self-hosted", "infisical", "cacahuate"]:
            self.assertNotIn(forbidden, workflow)


if __name__ == "__main__":
    unittest.main()
