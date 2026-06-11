# Capsule Server

This is the backend to the Capsule file transfer project. The web, command-line, and mobile interfaces all connect to this server for hosting files.

## Instructions
1. Clone the repository
2. Run `cargo build --release`
3. The `capsule-server` binary should be built and available under `target/release/capsule-server`
4. Execute `capsule-server` and the server should run at `http://localhost:9001`
5. Use `capsule server set` to point the CLI to the custom server address.

Tests:
- `cargo test -- --nocapture 2>&1 | tee test_output_3.txt`
