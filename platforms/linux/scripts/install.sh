#!/bin/sh
set -eu
source_dir=${1:-target/release}
dest="$HOME/.local/lib/rustboard"
mkdir -p "$dest" "$HOME/.local/bin"
for name in rustboard rustboardd; do
    install -m 755 "$source_dir/$name" "$dest/$name.new"
    mv "$dest/$name.new" "$dest/$name"
done
ln -sfn "$dest/rustboard" "$HOME/.local/bin/rustboard"
printf 'Installed in %s.\n' "$dest"
case ":$PATH:" in
    *":$HOME/.local/bin:"*) ;;
    *) printf 'Add %s/.local/bin to PATH, or run %s/rustboard.\n' "$HOME" "$dest" ;;
esac
