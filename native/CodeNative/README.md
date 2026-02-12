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

The native app expects a WebSocket mirror endpoint at
`ws://127.0.0.1:4317/ws`.

From the repo root, start the mirror server:

```bash
./code-rs/target/dev-fast/code web --host 127.0.0.1 --port 4317
```

In another terminal, run the native app:

```bash
cd native/CodeNative
swift run CodeNativeApp
```

The default endpoint in the app is `ws://127.0.0.1:4317/ws`.

## One-command Dev Loop

Use the repo helper to run backend + native app and auto-restart both on file
changes:

```bash
./scripts/dev-native.sh
```

Useful options:

```bash
./scripts/dev-native.sh --host 127.0.0.1 --port 4317
./scripts/dev-native.sh --no-watch
```

## Session Visibility

The mirror server only lists sessions created through this mirror process.
It does not yet auto-import active TUI sessions.

If the sidebar is empty, click `New thread` (or `Create first thread`) to
create a session.

## Notes

- The app requests microphone and speech recognition permission when recording
  starts.
- Voice capture currently uses the system speech recognizer with on-device
  preference enabled.
- Voice transcript state shows listening/partial/final capture feedback.
- This remains an early native milestone and does not yet include
  approval/action parity.
