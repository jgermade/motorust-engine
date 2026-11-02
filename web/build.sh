#!/usr/bin/env bash
# Builds the engine's web version (its demo, `src/main.rs`) into a folder
# ready to be served (by default, dist/web): the page, the .wasm and the
# JavaScript that loads it.
#
#   web/build.sh [FOLDER]
#
# Needs the wasm target (`rustup target add wasm32-unknown-unknown`) and
# wasm-bindgen-cli at the same version as wasm-bindgen in Cargo.lock:
#
#   cargo install wasm-bindgen-cli --locked --version <Cargo.lock's>
#
# To try it: `python3 -m http.server -d dist/web` and open
# http://localhost:8000.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-$root/dist/web}

cd "$root"
cargo build --release --target wasm32-unknown-unknown --bin motorust-engine
# Where Cargo leaves it: this folder's target/, or a workspace's over it.
target=$(cargo metadata --format-version 1 --no-deps |
  sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')
rm -rf "$out"
mkdir -p "$out"
# Without the functions' names, which weigh almost as much as the code.
wasm-bindgen --target web --no-typescript --remove-name-section --remove-producers-section \
  --out-dir "$out" --out-name motorust_engine \
  "$target/wasm32-unknown-unknown/release/motorust-engine.wasm"
bytes=$(wc -c < "$out/motorust_engine_bg.wasm" | tr -d ' ')
version=$(grep '^version = ' Cargo.toml | head -n 1 | cut -d'"' -f 2)
# The page, told the .wasm's size (for its progress bar) and the version.
sed "s/const WASM_BYTES = 0;/const WASM_BYTES = $bytes;/; s/__VERSION__/$version/g" \
  web/index.html > "$out/index.html"
ls -l "$out"
