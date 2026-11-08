#!/bin/bash
# Protocol v2 acceptance: capture both consoles, hammer the / page, then
# check sizes + sha256 equality + zero link-guard trips.
set -u
# Fresh output files: a failed curl may not truncate its -o target, and stale
# files from a previous run silently poison the sha/size checks (seen: 3 failed
# curls + old 460800 files made a 921600 run look green).
rm -f tmp/single_*.html tmp/burst_*.html
pkill -f 'cat /dev/cu' 2>/dev/null; sleep 1
# C6 port renumbers after every hub/power event — resolve it dynamically
# (Pico console carries the 'bench' serial).
C6PORT=$(ls /dev/cu.usbmodem* 2>/dev/null | grep -v bench | head -1)
[ -z "$C6PORT" ] && { echo "no C6 serial port found"; exit 1; }
echo "C6 console: $C6PORT"
cat "$C6PORT" > tmp/c6.log 2>&1 &
C6PID=$!
cat /dev/cu.usbmodembench1 > tmp/pico.log 2>&1 &
PIDPID=$!
echo "booting..."; sleep 18
# IP direct: led-board.local mDNS resolution costs ~5s on this Mac (unrelated to the link)
URL=http://192.168.49.137/
# Preflight: a fallback-AP state (joined=false) makes every curl fail in 7 s
# flat and poisons the whole run — wait for the STA lease to answer.
echo "waiting for C6 station to answer at $URL ..."
for i in $(seq 1 30); do
  if curl -s --max-time 3 -o /dev/null "$URL"; then echo "C6 up (attempt $i)"; break; fi
  sleep 3
  if [ "$i" = 30 ]; then echo "ABORT: C6 not reachable — join failed or AP-only"; pkill -f 'cat /dev/cu' 2>/dev/null; exit 1; fi
done
rm -f tmp/curlfail
for round in 1 2 3; do
  curl -s --max-time 30 $URL -o tmp/single_$round.html -w "R$round single: %{size_download}B %{time_total}s\n" || { echo "R$round single: CURL FAILED"; echo x >> tmp/curlfail; }
  sleep 2
  pids=""
  for i in 1 2 3 4; do
    ( curl -s --max-time 60 $URL -o tmp/burst_${round}_$i.html -w "R$round burst$i: %{size_download}B %{time_total}s\n" || { echo "R$round burst$i: CURL FAILED"; echo x >> tmp/curlfail; } ) &
    pids="$pids $!"
  done
  wait $pids
  sleep 2
done
FAILS=$(cat tmp/curlfail 2>/dev/null | wc -l | tr -d ' ')
[ -z "$FAILS" ] && FAILS=0
kill $C6PID $PIDPID 2>/dev/null; sleep 1
echo "=== curl failures (want 0): $FAILS"
echo '=== sha256 groups (want one 15-entry group):'
shasum -a 256 tmp/single_*.html tmp/burst_*.html | awk '{print $1}' | sort | uniq -c
echo '=== sizes:'
ls -l tmp/single_*.html tmp/burst_*.html | awk '{print $5}' | sort | uniq -c
echo '=== guard trips (want 0):'
grep -icE "corrupt|gap|overran|timeout|abort" tmp/c6.log tmp/pico.log
