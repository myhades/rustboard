#!/bin/bash
# Use a disposable HOME: installed executables must never run during installation.
set -euo pipefail
cd "$(dirname "$0")/../../.."
test_home=$(mktemp -d "${TMPDIR:-/tmp}/rustboard-install.XXXXXX")
trap 'rm -rf "$test_home"' EXIT
HOME="$test_home" bash platforms/macos/scripts/install.sh
[ -x "$test_home/Library/Application Support/Rustboard/bin/rustboard" ]
[ -d "$test_home/Applications/Rustboard.app" ]
[ -L "$test_home/.local/bin/rustboard" ]
[ "$test_home/.local/bin/rustboard" -ef "$test_home/Library/Application Support/Rustboard/bin/rustboard" ]
[ ! -e "$test_home/Library/LaunchAgents" ]
[ ! -e "$test_home/Library/Group Containers" ]
[ ! -e "$test_home/Library/Application Support/Rustboard/agent.out.log" ]
# Reinstallation preserves existing user data and does not create service files.
mkdir -p "$test_home/Library/Group Containers/test/default"
printf '%s' 'preserve-existing-settings' > "$test_home/Library/Group Containers/test/default/config.json"
HOME="$test_home" bash platforms/macos/scripts/install.sh --no-path
[ "$(cat "$test_home/Library/Group Containers/test/default/config.json")" = preserve-existing-settings ]
[ ! -e "$test_home/Library/LaunchAgents" ]
# A package-manager install uses the same binary without a second PATH entry.
HOME="$test_home" python3 platforms/macos/scripts/path.py remove
HOME="$test_home" bash platforms/macos/scripts/install.sh --no-path
[ -x "$test_home/Library/Application Support/Rustboard/bin/rustboard" ]
[ ! -e "$test_home/.local/bin/rustboard" ]
! grep -q 'Rustboard PATH' "$test_home/.zprofile" "$test_home/.bash_profile"
[ "$(cat "$test_home/Library/Group Containers/test/default/config.json")" = preserve-existing-settings ]

echo 'PASS: install/reinstall deploy files and CLI only; no config, service or startup initialization'
