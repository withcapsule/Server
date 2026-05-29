#!/usr/bin/env bash
# Tests that the rate limiter returns 429 when upload routes are hammered.
# Upload routes are limited to 2 req/s, global to 20 req/s.

HOST="http://localhost:9001"
TEST_FILE="testfile.bin"

echo "=== Rate Limit Test ==="

# --- Upload route rate limit ---
echo ""
echo "-- Hammering /curlup (limit: 2 req/s) with 10 rapid requests --"

PASS_COUNT=0
RATE_LIMITED_COUNT=0

for i in $( seq 1 10 ); do
    STATUS=$( curl -s -o /dev/null -w "%{http_code}" -X POST "$HOST/curlup" -F "f=@$TEST_FILE" )
    echo "  Request $i: HTTP $STATUS"
    if [ "$STATUS" == "200" ]; then
        (( PASS_COUNT++ ))
    elif [ "$STATUS" == "429" ]; then
        (( RATE_LIMITED_COUNT++ ))
    fi
done

echo ""
echo "  Allowed: $PASS_COUNT  |  Rate limited (429): $RATE_LIMITED_COUNT"

if [ "$RATE_LIMITED_COUNT" -gt 0 ]; then
    echo "  PASS: Rate limiter triggered on upload route"
else
    echo "  FAIL: No requests were rate limited — limiter may not be working"
fi

# --- Global rate limit ---
echo ""
echo "-- Hammering /ping (global limit: 20 req/s) with 30 rapid requests --"

PASS_COUNT=0
RATE_LIMITED_COUNT=0

for i in $( seq 1 30 ); do
    STATUS=$( curl -s -o /dev/null -w "%{http_code}" "$HOST/ping" )
    echo "  Request $i: HTTP $STATUS"
    if [ "$STATUS" == "200" ]; then
        (( PASS_COUNT++ ))
    elif [ "$STATUS" == "429" ]; then
        (( RATE_LIMITED_COUNT++ ))
    fi
done

echo ""
echo "  Allowed: $PASS_COUNT  |  Rate limited (429): $RATE_LIMITED_COUNT"

if [ "$RATE_LIMITED_COUNT" -gt 0 ]; then
    echo "  PASS: Global rate limiter triggered"
else
    echo "  FAIL: No requests were rate limited on /ping"
fi
