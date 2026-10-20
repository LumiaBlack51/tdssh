#!/usr/bin/env bash
set -euo pipefail
binary=${1:-target/release/tdssh}
prefix=${2:-"$HOME/.local/bin"}
[[ -f "$binary" ]] || { echo "Build first: cargo build --release --locked" >&2; exit 1; }
install -d -- "$prefix"
install -m 755 -- "$binary" "$prefix/tdssh"
printf 'Installed %s/tdssh\nAdd this directory to PATH in ~/.bashrc if needed.\n' "$prefix"
