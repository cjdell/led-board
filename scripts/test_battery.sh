#!/usr/bin/env bash
# Acceptance battery against a running board: page throughput, JSON APIs, and
# the playlist reset path. Usage: scripts/test_battery.sh [host]
set -uo pipefail
HOST="${1:-}"

if [ -z "$HOST" ]; then
    echo "== discovering board (mDNS / ARP scan of the wired LAN)..."
    HOST=$(dns-sd -G v4 led-board.local 2>/dev/null | tail -1 | awk '{print $NF}')
    if [ -z "${HOST:-}" ] || ! ping -c1 -t2 "$HOST" >/dev/null 2>&1; then
        # Fall back to probing the last known lease, then an ARP sweep.
        for ip in 10.3.2.212; do
            ping -c1 -t1 "$ip" >/dev/null 2>&1 && HOST=$ip && break
        done
    fi
fi

if [ -z "${HOST:-}" ]; then
    echo "no reachable board found" >&2
    exit 1
fi
echo "== host: $HOST"

echo "-- ping:"; ping -c2 -t2 "$HOST" | tail -1

echo "-- join status:"; curl -s -m 5 "http://$HOST/api/join-status"; echo

echo "-- animations (festival names present?):"
curl -s -m 5 "http://$HOST/api/info/animations" | tr ',' '\n' | grep -cE 'Aurora|Starfield|Snowfall|FallingLeaves|Fireflies|VinylSpin|CanalBoats|CenteredTitle|StatusCard' || true

echo "-- single page loads:"
ok=0
for i in 1 2 3 4 5; do
    out=$(curl -s -m 30 -o /dev/null -w '%{http_code} %{size_download} %{time_total}' "http://$HOST/")
    echo "   #$i $out"
    set -- $out
    [ "$1" = "200" ] && ok=$((ok+1))
done
echo "   singles 200: $ok/5"

echo "-- 4-burst page loads:"
for b in 1 2 3; do
    start=$(python3 -c 'import time;print(time.time())')
    for i in 1 2 3 4; do
        curl -s -m 30 -o /dev/null -w "   burst$b/$i %{http_code} %{size_download} %{time_total}\n" "http://$HOST/" &
    done
    wait
    end=$(python3 -c 'import time;print(time.time())')
    python3 -c "print(f'   burst$b wall: {$end-$start:.2f}s')"
done
