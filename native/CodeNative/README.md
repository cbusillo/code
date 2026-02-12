# CodeNative (M0 Bootstrap)

This directory contains the first native macOS bootstrap client for the
Codex session stream.

## Current Scope

- macOS SwiftUI shell
- Session list from live server
- Read-only transcript streaming for the selected session
- Attach/detach wiring when session selection changes

## Run

From the repo root, start the Rust mirror server:

```bash
cd code-rs
cargo run -p code-cli --bin code -- web --host 127.0.0.1 --port 4317
```

In another terminal, run the native app:

```bash
cd native/CodeNative
swift run CodeNativeApp
```

The default endpoint in the app is `ws://127.0.0.1:4317/ws`.

## Notes

- This is an M0 bootstrap and intentionally read-only.
- Voice features and full approvals/tool parity are planned in later milestones.
