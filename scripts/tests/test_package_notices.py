from __future__ import annotations

import json
from pathlib import Path
import sys
import tempfile
import unittest


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import package_notices


class NoticeTests(unittest.TestCase):
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / "licenses").mkdir()
        (self.root / "licenses/overrides.json").write_text("[]")
        self.package = self.root / "crate"
        self.package.mkdir()
        self.metadata = {
            "resolve": {"nodes": [{"id": "example"}]},
            "packages": [{"id": "example", "name": "example", "version": "1.0.0",
                          "source": "registry+https://github.com/rust-lang/crates.io-index",
                          "license": "MIT", "manifest_path": str(self.package / "Cargo.toml")}],
        }

    def test_root_and_vendored_notices_are_retained_and_hashed(self) -> None:
        (self.package / "LICENSE").write_text("root license")
        (self.package / "vendor").mkdir()
        (self.package / "vendor/NOTICE.txt").write_text("vendor attribution")
        (self.package / "source.rs").write_text("must not be copied")
        index, files = package_notices.collect(self.metadata, self.root)
        self.assertEqual(set(files), {"example-1.0.0/LICENSE", "example-1.0.0/vendor/NOTICE.txt"})
        self.assertEqual(files["example-1.0.0/vendor/NOTICE.txt"], b"vendor attribution")
        self.assertEqual(len(index["packages"][0]["files"]["LICENSE"]["sha256"]), 64)

    def test_missing_notice_fails_instead_of_claiming_complete_coverage(self) -> None:
        with self.assertRaisesRegex(ValueError, "no packaged notices"):
            package_notices.collect(self.metadata, self.root)

    def test_override_requires_the_reviewed_version_and_revision(self) -> None:
        (self.root / "licenses/approved.txt").write_text("reviewed upstream license")
        (self.root / "licenses/overrides.json").write_text(json.dumps([{
            "name": "example", "version": "1.0.0", "vcsCommit": "a" * 40,
            "files": ["licenses/approved.txt"], "source": "https://example.com/license",
        }]))
        vcs = self.package / ".cargo_vcs_info.json"
        vcs.write_text(json.dumps({"git": {"sha1": "b" * 40}}))
        with self.assertRaisesRegex(ValueError, "revision differs"):
            package_notices.collect(self.metadata, self.root)
        vcs.write_text(json.dumps({"git": {"sha1": "a" * 40}}))
        _, files = package_notices.collect(self.metadata, self.root)
        self.assertEqual(files["example-1.0.0/upstream/approved.txt"], b"reviewed upstream license")
        self.metadata["packages"][0]["version"] = "1.0.1"
        (self.package / "LICENSE").write_text("the new version supplies its own notice")
        with self.assertRaisesRegex(ValueError, "stale notice override"):
            package_notices.collect(self.metadata, self.root)

    def test_path_escape_and_symlink_notice_are_rejected(self) -> None:
        (self.root / "outside.txt").write_text("outside package")
        self.metadata["packages"][0]["license_file"] = "../outside.txt"
        with self.assertRaisesRegex(ValueError, "escapes"):
            package_notices.collect(self.metadata, self.root)
        del self.metadata["packages"][0]["license_file"]
        (self.package / "LICENSE").symlink_to(self.root / "outside.txt")
        with self.assertRaisesRegex(ValueError, "escapes"):
            package_notices.collect(self.metadata, self.root)

    def test_archive_names_and_size_are_bounded(self) -> None:
        (self.package / "LICENSE").write_bytes(b"x" * (package_notices.MAX_NOTICE_BYTES + 1))
        with self.assertRaisesRegex(ValueError, "oversized"):
            package_notices.collect(self.metadata, self.root)
        self.metadata["packages"][0]["name"] = "../../outside"
        with self.assertRaisesRegex(ValueError, "archive name"):
            package_notices.collect(self.metadata, self.root)


if __name__ == "__main__":
    unittest.main()
