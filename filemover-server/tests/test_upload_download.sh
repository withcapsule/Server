#!/usr/bin/env bash
# Tests upload via curl, then downloads the file and verifies it matches the original.

HOST="http://localhost:9001"
TEST_FILE="testfile.bin"
DOWNLOADED_FILE="downloaded.bin"

echo "=== Upload/Download Test ==="

# Upload
echo ""
echo "-- Uploading $TEST_FILE --"
UPLOAD_RESPONSE=$( curl -s -X POST "$HOST/curlup" -F "f=@$TEST_FILE" )
echo "Server response: $UPLOAD_RESPONSE"

# Parse file ID from response ("File ID for downloading is 12345")
FILE_ID=$( echo "$UPLOAD_RESPONSE" | grep -oP '(?<=File ID for downloading is )\d+' )

if [ -z "$FILE_ID" ]; then
    echo "FAIL: Could not parse file ID from upload response"
    exit 1
fi

echo "Parsed file ID: $FILE_ID"

# Download
echo ""
echo "-- Downloading file ID $FILE_ID --"
DOWNLOADED="downloaded_$FILE_ID.bin"
curl -s -o "$DOWNLOADED" "$HOST/download/$FILE_ID"

# Verify integrity
echo ""
echo "-- Verifying integrity --"
ORIGINAL_MD5=$( md5sum "$TEST_FILE" | awk '{print $1}' )
DOWNLOADED_MD5=$( md5sum "$DOWNLOADED" | awk '{print $1}' )

echo "Original MD5:   $ORIGINAL_MD5"
echo "Downloaded MD5: $DOWNLOADED_MD5"

if [ "$ORIGINAL_MD5" == "$DOWNLOADED_MD5" ]; then
    echo ""
    echo "PASS: Files match"
else
    echo ""
    echo "FAIL: MD5 mismatch — files differ"
    exit 1
fi

# Cleanup downloaded file
rm -f "$DOWNLOADED"
