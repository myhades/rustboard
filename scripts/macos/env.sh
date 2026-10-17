#!/bin/bash
# Source this; standard Rust installation first, optional existing local POC toolchain.
if ! command -v cargo >/dev/null; then
  if [ -x "$HOME/.cargo/bin/cargo" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
  elif [ -x "$HOME/Documents/Codex/Misc/lazy_paste/tooling/cargo/bin/cargo" ]; then
    export CARGO_HOME="$HOME/Documents/Codex/Misc/lazy_paste/tooling/cargo"
    export RUSTUP_HOME="$HOME/Documents/Codex/Misc/lazy_paste/tooling/rustup"
    export PATH="$CARGO_HOME/bin:$PATH"
  else
    echo 'Install Rust stable or put cargo on PATH' >&2; return 1
  fi
fi
