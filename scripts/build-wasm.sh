#!/usr/bin/env bash
# Build crates/receipt-check to WebAssembly and generate the JS bindings the Worker imports.
#
# Needs: a Rust toolchain with the wasm32-unknown-unknown target, and wasm-bindgen-cli at the
# same version as the wasm-bindgen crate (see crates/receipt-check/Cargo.toml).
# Override the tools with CARGO=... and WASM_BINDGEN=... if they are not on PATH.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cargo="${CARGO:-cargo}"
wasm_bindgen="${WASM_BINDGEN:-wasm-bindgen}"
out="$root/worker/src/wasm"

want="$(sed -n 's/^wasm-bindgen = "=\(.*\)"/\1/p' "$root/crates/receipt-check/Cargo.toml")"
have="$("$wasm_bindgen" --version | awk '{print $2}')"
if [ "$want" != "$have" ]; then
  echo "wasm-bindgen-cli $have does not match the crate's wasm-bindgen $want" >&2
  echo "install it with: cargo install wasm-bindgen-cli --version $want --locked" >&2
  exit 1
fi

"$cargo" build --manifest-path "$root/Cargo.toml" -p receipt-check --lib \
  --target wasm32-unknown-unknown --release
"$wasm_bindgen" --target web --out-dir "$out" --out-name receipt_check \
  "$root/target/wasm32-unknown-unknown/release/receipt_check.wasm"
ls -l "$out"
