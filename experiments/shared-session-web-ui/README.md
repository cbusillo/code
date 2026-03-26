# Shared Session Web UI Spike

This spike proves that Every Code's current app-server exposes enough
thread-native RPCs to support a browser-first UI without terminal scraping.

## What it does

- Connects to the Every Code app-server over WebSocket.
- Lists persisted threads with `thread/list`.
- Hydrates stored thread history with `thread/read`.
- Starts a new thread with `thread/start`.
- Resumes stored threads on demand with `thread/resume` before sending prompts.
- Sends prompts with `turn/start`.
- Shows typed turn lifecycle notifications plus raw app-server events
  through `addConversationListener`.

## Run it locally

1. Start the app-server on a WebSocket endpoint:

   ```bash
   cargo run -p code-app-server -- --listen ws://127.0.0.1:8877
   ```

2. Serve this folder as static files:

   ```bash
   python3 -m http.server 4173
   ```

3. Open `http://127.0.0.1:4173/experiments/shared-session-web-ui/`.

   If your browser harness blocks loopback, swap in your machine's LAN address
   instead, for example:

   ```text
   http://192.168.1.x:4173/experiments/shared-session-web-ui/
   ```

The page defaults to `ws://127.0.0.1:8877` and stores the last connection URL,
working directory, and model locally in the browser.

## Current caveat

The browser spike now targets the real thread-native request surface, but live
subscription still rides on `addConversationListener` because listener scoping
and event fan-out are still owned by the legacy conversation subscription rail.

That means this spike intentionally mixes:

- `thread/list`
- `thread/read`
- `thread/start`
- `turn/start`
- `addConversationListener`

Deterministic WebSocket parity coverage now proves the happy-path turn
lifecycle (`turn/started` and `turn/completed`) over the same transport the
browser spike uses.

Loading a stored thread is intentionally passive: the spike uses `thread/read`
to hydrate its transcript without attaching a live listener. When you send the
next prompt to that stored thread, the spike first calls `thread/resume` and
then subscribes for live updates.

The next follow-up is to decide whether browser clients need a first-class
thread-native subscription API or if the existing listener rail remains
sufficient, then deepen interruption/error-path coverage.

## Design notes

- The thread list and passive `thread/read` transcript reader are treated
  as the foundation.
- Live streaming combines typed `turn/*` lifecycle notifications with the
  readable raw event rail.
- Rich parity work such as approvals, command playback, and multi-client
  write coordination should be layered on after the basic list / read /
  start / prompt loop is proven.
