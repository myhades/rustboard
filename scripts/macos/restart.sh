#!/bin/bash
set -euo pipefail
export RUSTBOARD_PROFILE=default
launchctl kickstart -k "gui/$(id -u)/org.rustboard.agent"
for attempt in {1..50}; do
  if "$HOME/Library/Application Support/Rustboard/bin/rustboard" doctor 2>/dev/null; then exit 0; fi
  sleep 0.2
done
echo 'Agent could not load config or Keychain identity; inspect installed agent.err.log' >&2
exit 1
