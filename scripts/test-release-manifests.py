#!/usr/bin/env python3
"""Check generated manifests against their release files and checksums."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
import unittest

root = Path(__file__).resolve().parents[1]
version = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]


class ReleaseManifests(unittest.TestCase):
    def setUp(self):
        work = tempfile.TemporaryDirectory(prefix="rustboard-manifests-")
        self.addCleanup(work.cleanup)
        self.assets = Path(work.name) / "assets"
        self.output = Path(work.name) / "manifests"
        self.assets.mkdir()
        self.hashes = {}
        for platform, suffix in [("windows-x64", "zip"), ("linux-x86_64", "tar.gz"),
                                 ("macos-arm64", "tar.gz"), ("macos-x86_64", "tar.gz")]:
            name = f"rustboard-{version}-{platform}.{suffix}"
            data = platform.encode()
            (self.assets / name).write_bytes(data)
            digest = hashlib.sha256(data).hexdigest()
            (self.assets / (name + ".sha256")).write_text(f"{digest}  {name}\n")
            self.hashes[name] = digest

    def generate(self, *options, tag=None):
        return subprocess.run(
            [sys.executable, str(root / "scripts/release-manifests.py"),
             "--tag", tag or f"v{version}", "--assets", str(self.assets),
             "--output", str(self.output), *options],
            text=True, capture_output=True,
        )

    def test_release_contains_all_platforms_and_exact_hashes(self):
        result = self.generate()
        self.assertEqual(result.returncode, 0, result.stderr)
        cask = (self.output / "Casks/rustboard.rb").read_text()
        scoop = json.loads((self.output / "bucket/rustboard.json").read_text())
        self.assertEqual(scoop["version"], version)
        windows = f"rustboard-{version}-windows-x64.zip"
        self.assertEqual(scoop["architecture"]["64bit"]["hash"], self.hashes[windows])
        self.assertEqual(scoop["architecture"]["64bit"]["url"],
                         f"https://github.com/myhades/rustboard/releases/download/v{version}/{windows}")
        for name, digest in self.hashes.items():
            if name != windows:
                self.assertIn(digest, cask)
        self.assertIn("on_macos do", cask)
        self.assertIn("on_linux do", cask)
        self.assertIn("postflight_steps do", cask)
        self.assertNotRegex(cask, r"@[A-Z_]+@")

    def test_platform_checks_need_only_their_own_archive(self):
        for option, platform, output in [
            ("--windows-only", "windows-x64", "bucket/rustboard.json"),
            ("--linux-only", "linux-x86_64", "Casks/rustboard.rb"),
        ]:
            with self.subTest(platform=platform):
                hidden = []
                for path in self.assets.iterdir():
                    if platform not in path.name:
                        destination = path.with_name(path.name + ".hidden")
                        path.rename(destination)
                        hidden.append((path, destination))
                try:
                    result = self.generate(option)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertTrue((self.output / output).is_file())
                finally:
                    for path, destination in hidden:
                        destination.rename(path)

    def test_wrong_tag_missing_archive_and_changed_bytes_are_rejected(self):
        self.assertNotEqual(self.generate(tag="v999.0.0").returncode, 0)
        self.assertFalse(self.output.exists())
        for name in self.hashes:
            with self.subTest(archive=name):
                path = self.assets / name
                original = path.read_bytes()
                path.unlink()
                self.assertNotEqual(self.generate().returncode, 0)
                path.write_bytes(original + b"changed")
                result = self.generate()
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("Checksum mismatch", result.stderr)
                path.write_bytes(original)


if __name__ == "__main__":
    unittest.main()
