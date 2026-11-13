#!/usr/bin/env bash
set -euo pipefail

# rustup's cargo: the browser build needs the wasm32 target (AGENTS.md trap).
export PATH="$HOME/.cargo/bin:${PATH:-}"
cd "$(dirname "$0")"

deno task wasm
deno task build
deno task compress
