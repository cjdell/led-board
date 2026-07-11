#!/usr/bin/env bash
set -euo pipefail

picotool partition create partitions.json partition_table.bin
picotool load --offset 0x10000000 partition_table.bin
picotool partition info
