# Capsule Server

The Capsule server is the backend for the project. It accepts uploads, serves downloads, reports file status, and deletes files on request. Files are stored temporarily and cleaned up automatically.

## What it does

- listens on `http://localhost:9001` by default
- stores file metadata in SQLite
- writes uploaded files under `uploads/`
- exposes the HTTP API used by the Web UI, CLI, and Android app
- removes expired files in the background

Main API routes:

- `POST /upload`
- `GET /download/:file_id`
- `GET /status/:file_id`
- `DELETE /delete/:file_id`
- `GET /ping`

There are also HTML form routes used by the hosted Web UI.

## Build and run

```sh
cargo build --release
./target/release/capsule-server
```

For local development:

```sh
cargo run
```

The server binary will start on port `9001`.

## Notes

- uploads are currently limited by the server body limit
- files expire automatically after roughly one hour
- request rate limiting is enabled
- client-side encryption is handled by clients, not by the server

## Tests

Run the full test suite with:

```sh
cargo test -- --nocapture
```

Some tests expect the local server behavior and are written as integration-style checks rather than isolated unit tests.
