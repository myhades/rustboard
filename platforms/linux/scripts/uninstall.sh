#!/bin/sh
set -eu
base="$HOME/.local/lib/rustboard"
config_home=${XDG_CONFIG_HOME:-"$HOME/.config"}
if [ -x "$base/rustboard" ]; then "$base/rustboard" stop; fi
rm -f "$config_home/systemd/user/rustboard.service" "$config_home/autostart/rustboard.desktop"
systemctl --user daemon-reload
if [ "$(readlink "$HOME/.local/bin/rustboard" || true)" = "$base/rustboard" ]; then
    rm "$HOME/.local/bin/rustboard"
fi
rm -f "$base/rustboard" "$base/rustboardd"
rmdir "$base" 2>/dev/null || true
printf 'Uninstalled. Configuration and keyring identity retained.\n'
