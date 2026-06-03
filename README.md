# Capsule

Sometimes it's hard to just move 1 file without setting up an SSH connection or FTP server. Suppose I have a VPS and I just need to move 1-2 files and I'm already on the web console via my browser. Imagine how nice it could be to just `curl` a file onto a site and get a download link (and possibly a terminal-rendered QR code!). The server would hold the file for 15 mins minimum, 60 mins maximum, and then delete it. Maximum file size 512MB. Rate limit per IP address would be 2GB per hour worth of transfers.

Tests:
- `cargo test -- --nocapture 2>&1 | tee test_output_3.txt`
