#!/usr/bin/env bash

# probe-rs attach target/thumbv8m.main-none-eabihf/release/ws
# defmt-print -e target/thumbv8m.main-none-eabihf/release/ws tcp --host 192.168.49.39
# defmt-print -e target/thumbv8m.main-none-eabihf/release/ws tcp --host 192.168.1.1
defmt-print -e target/thumbv8m.main-none-eabihf/release/ws tcp --host mypico2w.local
