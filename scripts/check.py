#!/usr/bin/env python3
"""Run the automated checks used locally and in CI."""
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parents[1]


def run(*command):
    print("+ " + " ".join(command), flush=True)
    result = subprocess.run(command, cwd=root)
    if result.returncode:
        raise SystemExit(result.returncode)


run("cargo", "fmt", "--all", "--", "--check")
run("cargo", "clippy", "--all-targets", "--locked", "--", "-D", "warnings")
run("cargo", "test", "--locked", "--lib", "--bins", "--tests")
run(sys.executable, "scripts/test-release-manifests.py")
if sys.platform == "win32":
    run("pwsh", "-NoProfile", "-File", "platforms/windows/scripts/test-uninstall.ps1")
else:
    run(sys.executable, "scripts/test-purge.py")
    if sys.platform == "darwin":
        run("bash", "platforms/macos/scripts/test.sh")
