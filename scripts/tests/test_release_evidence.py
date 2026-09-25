from __future__ import annotations

import base64
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import package_release_evidence as evidence


class ReleaseEvidenceTests(unittest.TestCase):
    def fixture(self, root):
        artifacts = root / "artifacts"
        artifacts.mkdir()
        runner = root / "runner"
        runner.mkdir()
        (root / "Cargo.lock").write_bytes(b"synthetic lockfile")
        digest = "sha256:" + "b" * 64
        env = {
            "GITHUB_EVENT_NAME": "push", "GITHUB_REPOSITORY": "chrisbennight/mcp-komodo-rs",
            "GITHUB_SHA": "a" * 40, "GITHUB_REF": "refs/tags/v0.1.0",
            "PUBLISHED_IMAGE_DIGEST": digest, "RUNNER_TEMP": str(runner),
        }
        files = {}
        for name in ("sbom.cdx.json", "rust-sbom.cdx.json", "vulnerabilities.json"):
            data = json.dumps({"synthetic": name}).encode()
            (artifacts / name).write_bytes(data)
            files[name] = {"sha256": hashlib.sha256(data).hexdigest()}
        (artifacts / "build-record.json").write_text(json.dumps({"commit": env["GITHUB_SHA"], "sourceDirty": False, "files": files}))
        (artifacts / "dependency-notices.zip").write_bytes(b"synthetic notice archive")
        notice_hash = hashlib.sha256(b"synthetic notice archive").hexdigest()
        (artifacts / "dependency-notices.sha256").write_text(f"{notice_hash}  dependency-notices.zip\n")
        (artifacts / "dependency-notices.json").write_text(json.dumps({"cargoLockSha256": hashlib.sha256(b"synthetic lockfile").hexdigest()}))
        statement = {"subject": [{"name": evidence.IMAGE, "digest": {"sha256": "b" * 64}}]}
        bundle = json.dumps({"dsseEnvelope": {"payload": base64.b64encode(json.dumps(statement).encode()).decode()}})
        for key in ("PROVENANCE_BUNDLE", "SBOM_BUNDLE"):
            path = runner / (key + ".json")
            path.write_text(bundle)
            env[key] = str(path)
        return env

    def test_bundle_preserves_inputs_and_binds_the_manifest_digest(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            env = self.fixture(root)
            archive = evidence.package(root, env)
            first = archive.read_bytes()
            with zipfile.ZipFile(archive) as bundle:
                self.assertEqual(set(bundle.namelist()), {*evidence.FILES, "provenance.sigstore.json", "sbom.sigstore.json", "release-record.json"})
                record = json.loads(bundle.read("release-record.json"))
                self.assertEqual(record["image"], evidence.IMAGE + "@" + env["PUBLISHED_IMAGE_DIGEST"])
                self.assertEqual(record["sourceCommit"], env["GITHUB_SHA"])
                for name, metadata in record["files"].items():
                    self.assertEqual(metadata["sha256"], hashlib.sha256(bundle.read(name)).hexdigest())
                self.assertEqual(bundle.read("provenance.sigstore.json"), Path(env["PROVENANCE_BUNDLE"]).read_bytes())
            self.assertEqual((root / "artifacts/release-evidence.sha256").read_text(), hashlib.sha256(first).hexdigest() + "  release-evidence.zip\n")
            self.assertEqual(evidence.package(root, env).read_bytes(), first)

    def test_wrong_or_incomplete_evidence_never_produces_a_bundle(self):
        for defect in ("source", "dirty", "scan", "notice", "lock", "subject", "missing", "outside", "output-link", "digest"):
            with self.subTest(defect=defect), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                env = self.fixture(root)
                artifacts = root / "artifacts"
                if defect in ("source", "dirty"):
                    path = artifacts / "build-record.json"
                    record = json.loads(path.read_text())
                    record["commit" if defect == "source" else "sourceDirty"] = "c" * 40 if defect == "source" else True
                    path.write_text(json.dumps(record))
                elif defect == "scan":
                    (artifacts / "sbom.cdx.json").write_bytes(b"different")
                elif defect == "notice":
                    (artifacts / "dependency-notices.zip").write_bytes(b"different")
                elif defect == "lock":
                    (root / "Cargo.lock").write_bytes(b"different")
                elif defect == "subject":
                    env["PUBLISHED_IMAGE_DIGEST"] = "sha256:" + "d" * 64
                elif defect == "missing":
                    (artifacts / "vulnerabilities.json").unlink()
                elif defect == "outside":
                    env["PROVENANCE_BUNDLE"] = str(artifacts / "build-record.json")
                elif defect == "output-link":
                    (artifacts / "release-evidence.zip.tmp").symlink_to(root / "outside")
                else:
                    env["PUBLISHED_IMAGE_DIGEST"] = "not-a-digest"
                with self.assertRaises(ValueError):
                    evidence.package(root, env)
                self.assertFalse((artifacts / "release-evidence.zip").exists())

    def test_size_limit_is_enforced_on_read(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "large"
            with path.open("wb") as output:
                output.truncate(evidence.MAX_FILE_BYTES + 1)
            with self.assertRaises(ValueError):
                evidence.read_file(path, root)


if __name__ == "__main__":
    unittest.main()
