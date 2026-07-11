#!/usr/bin/env bash
set -euo pipefail

# probe-rs download firmware/blobs/43439A0.bin --binary-format bin --chip RP235x --base-address 0x10200000
picotool load --offset 0x10200000 firmware/blobs/43439A0.bin
