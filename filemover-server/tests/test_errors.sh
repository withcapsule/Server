#!/usr/bin/env bash
# Tests that the server returns correct error codes for bad requests.

HOST="http://localhost:9001"

echo "=== Error Handling Test ==="

# Download non-existent file ID
echo ""
echo "-- Download with non-existent ID --"
STATUS=$( curl -s -o /dev/null -w "%{http_code}" "$HOST/download/00000" )
echo "  HTTP $STATUS (expected 404)"
[ "$STATUS" == "404" ] && echo "  PASS" || echo "  FAIL: expected 404, got $STATUS"

# Upload with no file attached
echo ""
echo "-- Upload with no file --"
STATUS=$( curl -s -o /dev/null -w "%{http_code}" -X POST "$HOST/curlup" )
echo "  HTTP $STATUS (expected 400)"
[ "$STATUS" == "400" ] && echo "  PASS" || echo "  FAIL: expected 400, got $STATUS"

# Search for non-existent file ID via download form
echo ""
echo "-- Search for non-existent file ID --"
STATUS=$( curl -s -o /dev/null -w "%{http_code}" -X POST "$HOST/html_download_processor" -F "file_download_field=00000" )
echo "  HTTP $STATUS (expected 500 or 404)"
( [ "$STATUS" == "404" ] || [ "$STATUS" == "500" ] ) && echo "  PASS" || echo "  FAIL: expected 404/500, got $STATUS"

# Ping sanity check
echo ""
echo "-- Ping --"
RESPONSE=$( curl -s "$HOST/ping" )
echo "  Response: $RESPONSE"
echo "$RESPONSE" | grep -q "pong" && echo "  PASS" || echo "  FAIL: expected pong"
