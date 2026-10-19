#!/bin/sh
set -eu
source_dir=${1:-target/release}
no_path=${2:-}
case "$no_path" in ""|--no-path) ;; *) echo "Unknown option: $no_path" >&2; exit 1 ;; esac
dest="$HOME/.local/lib/rustboard"
mkdir -p "$dest"
for name in rustboard rustboardd; do
    install -m 755 "$source_dir/$name" "$dest/$name.new"
    mv "$dest/$name.new" "$dest/$name"
done
if [ "$no_path" != --no-path ]; then
    mkdir -p "$HOME/.local/bin"
    ln -sfn "$dest/rustboard" "$HOME/.local/bin/rustboard"
fi
printf 'Installed in %s.\n' "$dest"
[ "$no_path" != --no-path ] || exit 0
case ":$PATH:" in
    *":$HOME/.local/bin:"*) ;;
    *) printf 'Add %s/.local/bin to PATH, or run %s/rustboard.\n' "$HOME" "$dest" ;;
esac
