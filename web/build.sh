#!/usr/bin/env bash
# Build the browser bundle. wasm-pack 0.15 passes a stale --out-dir flag to
# the current cargo, so we drive the same two steps directly:
#   cargo → target wasm32, then the version-matched wasm-bindgen CLI.
# Serve statically afterwards:  cd web && python3 -m http.server 8080
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release --target wasm32-unknown-unknown --lib --features wasm
WASM_BINDGEN_VERSION=$(grep -A1 'name = "wasm-bindgen"' Cargo.lock | sed -n 's/.*version = "\(.*\)"/\1/p' | head -1)
if ! wasm-bindgen --version 2>/dev/null | grep -q "$WASM_BINDGEN_VERSION"; then
  echo "Installing wasm-bindgen-cli $WASM_BINDGEN_VERSION (matches the crate)..."
  cargo install wasm-bindgen-cli --version "$WASM_BINDGEN_VERSION"
fi
wasm-bindgen --target web --out-dir web/pkg --out-name worm \
  target/wasm32-unknown-unknown/release/worm.wasm
echo ""
echo "Built web/pkg/. Serve with:  cd web && python3 -m http.server 8080"
echo "Then open:                   http://localhost:8080"
