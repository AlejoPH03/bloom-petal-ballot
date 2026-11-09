#!/usr/bin/env bash
# Build the route components for a Bloom GitHub source install or a release:
# the [build].command in petal.toml. Same shape as bloom-petal-hyperliquid and
# bloom-petal-polymarket scripts/build.sh. PETAL_REV is the SDK pin in
# petal-build.toml and route/Cargo.toml.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PETAL_REV="ba76944a20248493858bfbc2c5f86e3d859f719a"

if [[ -n "${PETAL_BIN:-}" ]]; then
  "$PETAL_BIN" build --root "$ROOT"
else
  tool_root="$ROOT/target/petal-tool"
  cargo install \
    --git https://github.com/bloom-directory/petal \
    --rev "$PETAL_REV" \
    --locked \
    --root "$tool_root" \
    bloom-petal-cli
  "$tool_root/bin/petal" build --root "$ROOT"
fi
