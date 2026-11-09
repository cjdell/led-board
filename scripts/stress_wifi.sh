#!/bin/bash
# Wi-Fi (cyw43) acceptance — same bar as stress.sh: 3 single + 3x4-burst loads
# of the / page, then require one sha256 group, 0 curl failures, and zero
# link/stack trips in the logs. Adds per-load throughput so the divider/buffer
# experiment has numbers.
#
# Board build must be flashed with --features wifi,defmt_tcp,console.
# HOST defaults to led-board.local (mDNS can take ~10s after boot — the
# preflight loop covers it); override for a known IP: HOST=http://192.168.1.50/
# Log capture: defmt-print over the TCP logger (port 19021, see defmt_tcp crate)
# plus the USB console ("bench" CDC, BOOT lines + usb_dbg logs).
set -u
HOST=${HOST:-http://led-board.local/}
DEFMT_HOST=${DEFMT_HOST:-$(echo "$HOST" | sed -E 's#https?://##; s#/.*##; s#:.*##')}
rm -f tmp/single_*.html tmp/burst_*.html tmp/curlfail tmp/wifi_defmt.log tmp/pico.log tmp/wifi_times.txt
pkill -f 'defmt-print' 2>/dev/null; pkill -f 'cat /dev/cu' 2>/dev/null; sleep 1

# Preflight: a failed join (AP fallback at 192.168.1.1) makes every curl hit
# the captive portal page and poisons the sha check — wait for STA HTTP first.
echo "waiting for board at $HOST ..."
for i in $(seq 1 40); do
  if curl -s --max-time 3 -o /dev/null "$HOST"; then echo "board up (attempt $i)"; break; fi
  sleep 3
  if [ "$i" = 40 ]; then echo "ABORT: board not reachable — join failed or wrong HOST"; exit 1; fi
done

if [ -f tmp/ws.elf ] && command -v defmt-print >/dev/null; then
  defmt-print -e tmp/ws.elf tcp --host "$DEFMT_HOST" --port 19021 > tmp/wifi_defmt.log 2>&1 &
  DEFPID=$!
  echo "defmt-print -> tmp/wifi_defmt.log (pid $DEFPID)"
fi
if [ -c /dev/cu.usbmodembench1 ]; then
  cat /dev/cu.usbmodembench1 > tmp/pico.log 2>&1 &
  PICOPID=$!
  echo "pico console -> tmp/pico.log"
fi
sleep 2

get() { curl -s --max-time "$2" "$HOST" -o "$3" -w "%{url_effective} $1 %{size_download}B %{time_total}s\n" | tee -a tmp/wifi_times.txt || { echo "$1: CURL FAILED" | tee -a tmp/wifi_times.txt; echo x >> tmp/curlfail; }; }

for round in 1 2 3; do
  get "R$round single" 30 "tmp/single_$round.html"
  sleep 2
  pids=""
  for i in 1 2 3 4; do
    ( get "R$round burst$i" 60 "tmp/burst_${round}_$i.html" ) &
    pids="$pids $!"
  done
  wait $pids
  sleep 2
done

FAILS=$(cat tmp/curlfail 2>/dev/null | wc -l | tr -d ' ')
[ -z "$FAILS" ] && FAILS=0
kill ${DEFPID:-} ${PICOPID:-} 2>/dev/null; sleep 1
pkill -f 'defmt-print' 2>/dev/null; pkill -f 'cat /dev/cu' 2>/dev/null

echo "=== curl failures (want 0): $FAILS"
echo '=== sha256 groups (want one 15-entry group):'
shasum -a 256 tmp/single_*.html tmp/burst_*.html | awk '{print $1}' | sort | uniq -c
echo '=== sizes:'
ls -l tmp/single_*.html tmp/burst_*.html | awk '{print $5}' | sort | uniq -c
echo '=== throughput per load (kbit/s, median + min):'
awk '{ if ($3 ~ /^[0-9]+B$/) { n=$3+0; t=$4+0; if (t>0) print n*8/t/1000 } }' tmp/wifi_times.txt \
  | sort -n | awk '{a[NR]=$1} END {if (NR) printf "min %.0f  median %.0f  max %.0f  (n=%d)\n", a[1], a[int((NR+1)/2)], a[NR], NR}'
echo '=== guard trips (want 0):'
grep -icE "corrupt|gap|overran|timeout|abort|len inv|link lost|join" tmp/wifi_defmt.log tmp/pico.log 2>/dev/null
