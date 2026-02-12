# CodeNative (M0 Bootstrap)

This directory contains the first native macOS bootstrap client for the
Codex session stream.

## Current Scope

- macOS SwiftUI shell
- Session list from live server
- Read-only transcript streaming for the selected session
- Attach/detach wiring when session selection changes
- Voice controls (hold-to-talk, auto-submit voice text,
  auto-speak assistant replies)
- Composer submit + interrupt turn controls

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

- The app requests microphone and speech recognition permission when recording
  starts.
- Voice capture currently uses the system speech recognizer with on-device
  preference enabled.
- Voice transcript state shows listening/partial/final capture feedback.
- This remains an early native milestone and does not yet include
  approval/action parity.
