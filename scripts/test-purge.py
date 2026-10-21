#!/usr/bin/env python3
"""Check purge scope and credential failures without accessing the user's keyring."""
import os
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="rustboard-purge-") as tmp:
    work = Path(tmp)
    tools = work / "tools"
    tools.mkdir()
    for platform in ("macos", "linux"):
        home = work / platform
        home.mkdir()
        env = dict(os.environ, HOME=str(home), PATH=str(tools) + ":" + os.environ["PATH"])
        env.pop("XDG_CONFIG_HOME", None)
        if platform == "macos":
            command = tools / "security"
            expected = "delete-generic-password -s org.rustboard.device-identity"
            absent = 44
            data = home / "Library/Group Containers/TEST.org.rustboard.shared"
            other = home / "Library/Group Containers/TEST.other"
            extra = home / "Library/Application Support/Rustboard"
        else:
            command = tools / "secret-tool"
            expected = "clear xdg:schema org.rustboard.Identity"
            absent = 1
            data = home / ".config/rustboard"
            other = home / ".config/other"
            extra = data / "default/logs"
        for path in (data, other, extra):
            path.mkdir(parents=True, exist_ok=True)
        sentinel = data / "config.json"
        sentinel.write_text("identity and peers")
        script = root / f"platforms/{platform}/scripts/purge.sh"
        def mock(body):
            command.write_text('#!/bin/sh\n[ "$*" = "' + expected + '" ] || exit 2\n' + body)
            command.chmod(0o755)
        mock('echo "keyring locked" >&2\nexit 1\n')
        result = subprocess.run(["bash", str(script)], env=env, capture_output=True)
        assert result.returncode != 0 and sentinel.exists(), platform
        # One matching credential, followed by no matches (Mac deletes one at a time).
        marker = home / "credential-deleted"
        mock('if [ ! -e "' + str(marker) + '" ]; then touch "' + str(marker) + '"; exit 0; fi\nexit ' + str(absent) + '\n')
        subprocess.run(["bash", str(script)], env=env, check=True)
        assert marker.exists() and not data.exists() and not extra.exists() and other.exists(), platform
        subprocess.run(["bash", str(script)], env=env, check=True)
        print(f"PASS: {platform} purge scope, credential failure, repeatability")
