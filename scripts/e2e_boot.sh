#!/usr/bin/env bash
# Boot a CRUSSTY-managed server dir and assert imagine log markers.
# Usage: e2e_boot.sh <server-dir>
set -euo pipefail

DIR="${1:-server}"
RESULT_FILE="e2e-result.txt"
TIMEOUT_SEC="${TIMEOUT_SEC:-240}"

cd "$DIR"

MARKERS=(
  "[imagine] cplugin_init"
  "[imagine]: plan ready"
)

echo "e2e: booting kernel from $DIR (timeout ${TIMEOUT_SEC}s)"

BOOT_START="$(date +%s)"
STATUS="fail"

cleanup() {
  if [ -n "${SERVER_PID:-}" ] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID" 2>/dev/null || true
    sleep 2
    kill -9 "$SERVER_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

if command -v crussty >/dev/null 2>&1; then
  crussty run --nogui > boot.log 2>&1 &
else
  echo "e2e: crussty CLI absent — booting via launcher" > boot.log
  java -Xmx2G -jar launcher/launcher.jar --nogui >> boot.log 2>&1 &
fi
SERVER_PID=$!

while true; do
  NOW="$(date +%s)"
  if [ $((NOW - BOOT_START)) -gt "$TIMEOUT_SEC" ]; then
    echo "e2e: timeout waiting for markers" | tee -a boot.log
    break
  fi
  if ! kill -0 "$SERVER_PID" 2>/dev/null; then
    echo "e2e: server process exited" | tee -a boot.log
    break
  fi
  ALL=1
  for m in "${MARKERS[@]}"; do
    if ! grep -qF "$m" boot.log logs/latest.log 2>/dev/null; then
      ALL=0
      break
    fi
  done
  if [ "$ALL" -eq 1 ]; then
    STATUS="ok"
    echo "e2e: all markers present"
    break
  fi
  sleep 3
done

{
  echo "status: $STATUS"
  echo "markers:"
  for m in "${MARKERS[@]}"; do
    if grep -qF "$m" boot.log logs/latest.log 2>/dev/null; then
      echo "  [x] $m"
    else
      echo "  [ ] $m"
    fi
  done
  echo "--- tail of boot.log ---"
  tail -80 boot.log || true
} > "../$RESULT_FILE"

cat "../$RESULT_FILE"
[ "$STATUS" = "ok" ]
