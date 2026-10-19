#!/usr/bin/env python3
"""Generate package-manager metadata from the exact release artifacts."""
import argparse
import hashlib
import json
import re
import tomllib
from pathlib import Path

root = Path(__file__).resolve().parents[1]
p = argparse.ArgumentParser()
p.add_argument("--tag", required=True)
p.add_argument("--assets", type=Path, required=True)
p.add_argument("--output", type=Path, required=True)
group = p.add_mutually_exclusive_group()
group.add_argument("--linux-only", action="store_true", help="Generate Linux Cask metadata for Linux CI")
group.add_argument("--windows-only", action="store_true", help="Generate Scoop metadata for Windows CI")
a = p.parse_args()
version = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?", version) or a.tag != "v" + version:
    raise SystemExit("Tag must equal v + Cargo package version")
url = f"https://github.com/myhades/rustboard/releases/download/{a.tag}"

def digest(name):
    path = a.assets / name
    with path.open("rb") as stream:
        value = hashlib.file_digest(stream, "sha256").hexdigest()
    expected = path.with_name(path.name + ".sha256").read_text().split()[0]
    if value != expected:
        raise SystemExit(f"Checksum mismatch: {name}")
    return value

if not a.windows_only:
    linux_archive = f"rustboard-{version}-linux-x86_64.tar.gz"
    linux = (root / "platforms/linux/packaging/rustboard.rb.in").read_text()
    linux = linux.replace('@VERSION@', version).replace('@SHA256@', digest(linux_archive)).replace('@URL@', f"{url}/{linux_archive}")
    template = linux
    if not a.linux_only:
        arm = digest(f"rustboard-{version}-macos-arm64.tar.gz")
        intel = digest(f"rustboard-{version}-macos-x86_64.tar.gz")
        template = (root / "platforms/macos/packaging/rustboard.rb.in").read_text()
        template = template.replace('  sha256 "@SHA256@"', f'  sha256 arm: "{arm}", intel: "{intel}"')
        template = template.replace('  url "@URL@"', '  arch arm: "arm64", intel: "x86_64"\n  url "' + url + '/rustboard-#{version}-macos-#{arch}.tar.gz"')
        template = template.replace('  depends_on arch: :@ARCH@\n', '').replace('@VERSION@', version)
        # One Cask selects the platform's artifact and vendor lifecycle scripts.
        blocks = []
        for os_name, body in [("macos", template), ("linux", linux)]:
            lines = body.splitlines()[2:-1]
            blocks.append(f"  on_{os_name} do\n" + "\n".join("  " + line for line in lines) + "\n  end")
        template = f'cask "rustboard" do\n  version "{version}"\n' + "\n".join(blocks) + "\nend\n"
    tap = a.output / "Casks"
    tap.mkdir(parents=True, exist_ok=True)
    (tap / "rustboard.rb").write_text(template)
if a.linux_only:
    print(a.output)
    raise SystemExit(0)
archive = f"rustboard-{version}-windows-x64.zip"
scoop = dict(
    version=version,
    description="Native clipboard synchronization with lazy file transfer",
    homepage="https://github.com/myhades/rustboard",
    license="MIT",
    architecture={"64bit": dict(url=f"{url}/{archive}", hash=digest(archive))},
    bin="bin/rustboard.exe",
    pre_install="if ($global) { throw 'Rustboard must be installed per user, without --global' }",
    installer=dict(script='& "$dir\\platforms\\windows\\scripts\\install.ps1" -Source "$dir\\bin" -NoPath'),
    uninstaller=dict(script='if ($cmd -eq "uninstall") { & "$dir\\platforms\\windows\\scripts\\uninstall.ps1" }'),
    notes=["Run rustboard start to initialize and enable the service; then configure/pair.", "Uninstall retains identity and pairing. Scoop manages the CLI shim."])
bucket = a.output / "bucket"
bucket.mkdir(parents=True, exist_ok=True)
(bucket / "rustboard.json").write_text(json.dumps(scoop, indent=2) + "\n")
print(a.output)
