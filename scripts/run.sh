#!/usr/bin/env bash
set -euo pipefail

DELAY_US="${DELAY_US:-500}"
DURATION_SEC="${DURATION_SEC:-600}"
SESSIONS="${SESSIONS:-64}"
PAYLOAD="${PAYLOAD:-64}"
LD_LIBRARY_PATH="${LD_LIBRARY_PATH:-/usr/local/lib64}"

echo "=== Running async-ping (A) ==="
echo "delay_us=${DELAY_US} duration_sec=${DURATION_SEC} sessions=${SESSIONS} payload=${PAYLOAD}"
sudo env LD_LIBRARY_PATH="${LD_LIBRARY_PATH}" \
    ./target/release/async-ping \
    --delay-us "${DELAY_US}" \
    --duration-sec "${DURATION_SEC}" \
    --sessions "${SESSIONS}" \
    --payload "${PAYLOAD}"

echo ""
echo "=== Running raw-ping (B) ==="
sudo env LD_LIBRARY_PATH="${LD_LIBRARY_PATH}" \
    ./target/release/raw-ping \
    --delay-us "${DELAY_US}" \
    --duration-sec "${DURATION_SEC}" \
    --sessions "${SESSIONS}" \
    --payload "${PAYLOAD}"
