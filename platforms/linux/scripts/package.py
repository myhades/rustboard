#!/usr/bin/env python3
"""Package the Linux release binaries and vendor scripts."""
import hashlib
import platform
import tarfile
import tomllib
from pathlib import Path

root = Path(__file__).resolve().parents[3]
if platform.system() != "Linux" or platform.machine() != "x86_64":
    raise SystemExit("Build the Linux x86_64 package on Linux x86_64")
version = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
out = root / "target/release-assets"
out.mkdir(parents=True, exist_ok=True)
archive = out / f"rustboard-{version}-linux-x86_64.tar.gz"
with tarfile.open(archive, "w:gz") as tar:
    for name in ["rustboard", "rustboardd"]:
        tar.add(root / "target/release" / name, arcname="bin/" + name)
    for name in ["install.sh", "uninstall.sh"]:
        tar.add(root / "platforms/linux/scripts" / name, arcname="platforms/linux/scripts/" + name)
    tar.add(root / "LICENSE", arcname="LICENSE")
    tar.add(root / "docs/LINUX.md", arcname="docs/LINUX.md")
with archive.open("rb") as stream:
    digest = hashlib.file_digest(stream, "sha256").hexdigest()
archive.with_name(archive.name + ".sha256").write_text(digest + "  " + archive.name + "\n")
print(archive)
