#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --release --locked
version=$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')
host=$(rustc -vV | sed -n 's/^host: //p')
name="mcfinder-v${version}-${host}-native"
directory="dist/$name"
mkdir -p "$directory/licenses" "$directory/examples/scripts" "$directory/docs"
cp target/release/mcfinder "$directory/mcfinder"
cp README.md LICENSE "$directory/"
cp examples/*.json examples/backend_client.py examples/sdk.rs examples/sdk_script.rs "$directory/examples/"
cp examples/scripts/*.rhai "$directory/examples/scripts/"
cp docs/ENGINE.md "$directory/docs/"
cp vendor/cubiomes/LICENSE "$directory/licenses/Cubiomes-MIT.txt"
cp vendor/seedforge-patches/LICENSE "$directory/licenses/SeedForge-MIT.txt"
cp vendor/README.md "$directory/licenses/SOURCES.md"
cargo metadata --format-version 1 --offline | python3 scripts/package-licenses.py "$directory/licenses"
cat > "$directory/RELEASE.txt" <<'NOTE'
Native CPU build: optimized for the machine used to build this archive.
GPU profile is detected automatically at runtime.
Run ./mcfinder devices, ./mcfinder search --help, or ./mcfinder versions.
TUI: ./mcfinder search --script examples/scripts/ship-mansion.rhai --experimental --ui tui
Backend: ./mcfinder serve
API documentation: docs/ENGINE.md
NOTE
"$directory/mcfinder" devices | sed -n '/^cpu:/p' >> "$directory/RELEASE.txt"
tar -czf "dist/$name.tar.gz" -C dist "$name"
shasum -a 256 "dist/$name.tar.gz" > "dist/$name.tar.gz.sha256"
echo "dist/$name.tar.gz"

tar -czf "dist/mcfinder-v${version}-source.tar.gz" Cargo.toml Cargo.lock build.rs src vendor examples docs scripts tests README.md LICENSE .gitignore
shasum -a 256 "dist/mcfinder-v${version}-source.tar.gz" > "dist/mcfinder-v${version}-source.tar.gz.sha256"
