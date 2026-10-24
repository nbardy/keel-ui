#!/bin/sh
# Build the browser starter into kit_web/www/pkg, and with `serve` also serve it on http://localhost:8080.
#
#   kit_web/build.sh          # build only
#   kit_web/build.sh serve    # build, then serve (Ctrl-C stops it)
#
# Needs: the wasm32-unknown-unknown target (rust-toolchain.toml installs it) and the `wasm-bindgen` CLI at EXACTLY the
# version the lock file pins:  cargo install wasm-bindgen-cli --version 0.2.129 --locked
# (No bundler: the page loads the module wasm-bindgen emits. `trunk` would also work but is not required.)
set -eu
cd "$(dirname "$0")/.."

want=$(grep -A1 '^name = "wasm-bindgen"$' Cargo.lock | sed -n 's/^version = "\(.*\)"$/\1/p')
have=$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)
if [ "$have" != "$want" ]; then
  echo "wasm-bindgen CLI is '${have:-missing}', the lock pins $want. Install it:" >&2
  echo "  cargo install wasm-bindgen-cli --version $want --locked" >&2
  exit 1
fi

cargo build --locked --release --target wasm32-unknown-unknown -p kit_web --example starter
target=$(cargo metadata --locked --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
wasm-bindgen --target web --no-typescript --out-dir kit_web/www/pkg \
  "$target/wasm32-unknown-unknown/release/examples/starter.wasm"
echo "built kit_web/www/pkg ($(wc -c < kit_web/www/pkg/starter_bg.wasm) bytes of wasm)"

if [ "${1:-}" = serve ]; then
  echo "serving http://localhost:8080 (needs a browser with WebGPU)"
  exec python3 -m http.server 8080 --bind 127.0.0.1 --directory kit_web/www
fi
