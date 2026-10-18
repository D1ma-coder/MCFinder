#!/bin/sh
# Automatically tune both Rust and Cubiomes for the actual build-machine CPU.
set -eu
cd "$(dirname "$0")/.."
if [ "$#" -ne 0 ]; then
  echo 'Usage: scripts/build-native.sh (CPU profile is automatic)' >&2; exit 1
fi
case "$(uname -m)" in
  arm64|aarch64) cpu_flags='-mcpu=native' ;;
  x86_64|amd64) cpu_flags='-march=native' ;;
  *) echo 'Unsupported native architecture; use cargo build --release --no-default-features.' >&2; exit 1 ;;
esac
export RUSTFLAGS="${RUSTFLAGS:-} -C target-cpu=native"
export CFLAGS="${CFLAGS:-} $cpu_flags"
export CARGO_TARGET_DIR="target/native"
cargo build --release --locked
