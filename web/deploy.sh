#!/usr/bin/env bash
set -euo pipefail

rm -rf public/wasm/*.wsm
cp -rv ../sdk/wasm/*.wsm public/wasm/

deno task generate-manifest

ssh 192.168.49.1 "rm -f /srv/rustagon/apps/*.wsm"
scp -r public/wasm/* 192.168.49.1:/srv/rustagon/apps

deno task build
deno task compress

scp -r dist/* 192.168.49.1:/srv/rustagon/demo
