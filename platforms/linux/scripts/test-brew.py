#!/usr/bin/env python3
"""Exercise local Cask install, upgrade and uninstall without publishing a release."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib

parser = argparse.ArgumentParser()
parser.add_argument("--brew", default="brew")
args = parser.parse_args()
root = Path(__file__).resolve().parents[3]
brew = Path(shutil.which(args.brew) or args.brew).resolve()
version = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
archive = root / f"target/release-assets/rustboard-{version}-linux-x86_64.tar.gz"
digest = hashlib.sha256(archive.read_bytes()).hexdigest()
template = (root / "target/manifests/Casks/rustboard.rb").read_text()
work = Path(tempfile.mkdtemp(prefix="rustboard-brew-"))
home = work / "home"
home.mkdir()
env = dict(os.environ, HOME=str(home), HOMEBREW_NO_AUTO_UPDATE="1", HOMEBREW_NO_ANALYTICS="1", HOMEBREW_NO_ENV_HINTS="1", HOMEBREW_NO_INSTALL_CLEANUP="1")
env.pop("XDG_CONFIG_HOME", None)
env.pop("RUSTBOARD_PROFILE", None)
tap = "rustboard-test/local"
name = tap + "/rustboard"

def run(*argv, **kwargs):
    return subprocess.run([str(brew), *argv], env=env, check=True, text=True, **kwargs)

def write_cask(revision):
    text = template.replace(f'version "{version}"', f'version "{version}-test.{revision}"')
    import re
    text = re.sub(r'url "[^"\n]+"', f'url "{archive.as_uri()}"', text)
    assert digest in text
    (tap_path / "Casks/rustboard.rb").write_text(text)

prefix = Path(run("--prefix", capture_output=True).stdout.strip())
assert not (prefix / "Caskroom/rustboard").exists(), "Use a Brew installation without Rustboard"
tap_path = prefix / "Library/Taps/rustboard-test/homebrew-local"
assert not tap_path.exists(), "Test tap already exists"
(tap_path / "Casks").mkdir(parents=True)
subprocess.run(["git", "init", "-q", str(tap_path)], check=True)
config = home / ".config/rustboard/default/config.json"
installed = home / ".local/lib/rustboard"
link = prefix / "bin/rustboard"
report = {}
try:
    write_cask(1)
    run("readall", "--os=all", "--arch=all", tap)
    run("install", "--cask", name)
    assert installed.joinpath("rustboard").is_file() and installed.joinpath("rustboardd").is_file()
    assert link.resolve() == installed / "rustboard"
    assert not (home / ".local/bin/rustboard").exists()
    assert not (home / ".config").exists(), "Install initialized user state"
    report["fresh_install_no_initialization"] = True
    # Preserve opaque user data; installation must not parse or rewrite it.
    config.parent.mkdir(parents=True)
    config.write_text('{"installation-test":"preserve exactly"}\n')
    original = config.read_bytes()
    installed.joinpath("rustboardd").write_bytes(b"replace this stale binary")
    write_cask(2)
    run("upgrade", "--cask", name)
    assert config.read_bytes() == original
    assert installed.joinpath("rustboardd").read_bytes()[:4] == b"\x7fELF"
    assert not home.joinpath(".config/systemd/user/rustboard.service").exists()
    assert not home.joinpath(".config/autostart/rustboard.desktop").exists()
    report["upgrade_replaces_files_preserves_config_no_start"] = True
    run("uninstall", "--cask", name)
    assert not link.is_symlink() and not installed.exists()
    assert config.read_bytes() == original
    report["uninstall_removes_cli_preserves_config"] = True
finally:
    if (prefix / "Caskroom/rustboard").exists():
        subprocess.run([str(brew), "uninstall", "--cask", name], env=env)
    shutil.rmtree(tap_path)
    shutil.rmtree(work)
out = root / "test-output/linux"
out.mkdir(parents=True, exist_ok=True)
(out / "brew.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report, indent=2))
