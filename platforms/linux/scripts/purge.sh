#!/bin/sh
set -eu
# Homebrew runs the uninstall hook before this zap hook.
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
secret_tool=${1:-secret-tool}
if message=$("$secret_tool" clear xdg:schema org.rustboard.Identity 2>&1); then
    :
else
    result=$?
    # secret-tool also returns 1, without an error, when no entries match.
    if [ "$result" -ne 1 ] || [ -n "$message" ]; then
        printf '%s\n' "$message" >&2
        exit "$result"
    fi
fi
config_home=${XDG_CONFIG_HOME:-"$HOME/.config"}
case "$config_home" in /*) ;; *) echo 'Config directory must be absolute' >&2; exit 1 ;; esac
rm -rf "$config_home/rustboard"
