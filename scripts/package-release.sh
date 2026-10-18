#!/bin/sh
# Build a portable executable and archive only that executable.
set -eu
cd "$(dirname "$0")/.."
host=$(rustc -vV | sed -n 's/^host: //p')
cargo build --release --locked --no-default-features --target "$host"
python3 scripts/package.py --target "$host"
