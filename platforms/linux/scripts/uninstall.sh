#!/bin/sh
set -eu
# Homebrew removes session environment variables before running uninstall hooks.
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
base="$HOME/.local/lib/rustboard"
config_home=${XDG_CONFIG_HOME:-"$HOME/.config"}
if [ -f "$config_home/systemd/user/rustboard.service" ]; then
    if [ -x "$base/rustboard" ]; then "$base/rustboard" stop; fi
    rm "$config_home/systemd/user/rustboard.service"
    systemctl --user daemon-reload
fi
rm -f "$config_home/autostart/rustboard.desktop"
if [ "$(readlink "$HOME/.local/bin/rustboard" || true)" = "$base/rustboard" ]; then
    rm "$HOME/.local/bin/rustboard"
fi
rm -f "$base/rustboard" "$base/rustboardd"
rmdir "$base" 2>/dev/null || true
printf 'Uninstalled. Configuration and keyring identity retained.\n'
