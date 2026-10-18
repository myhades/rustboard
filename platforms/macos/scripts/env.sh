#!/bin/bash
if ! command -v cargo >/dev/null; then
  export PATH="$HOME/.cargo/bin:$PATH"
fi
command -v cargo >/dev/null || { echo 'Put a Rust stable toolchain on PATH' >&2; return 1; }
