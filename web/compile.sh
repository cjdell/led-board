#!/usr/bin/env bash
set -euo pipefail

deno task build
deno task compress
