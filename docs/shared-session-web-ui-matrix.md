<!-- markdownlint-disable MD013 -->

# Shared Session Web UI Matrix

This matrix reflects the app-server that is actually implemented in
`code-rs/app-server` today.

Important distinction:

- The app-server now routes the thread-native request surface for read/list/
  start/resume plus the basic `turn/start` path.
- Live browser subscriptions still attach through `addConversationListener`,
  which means the request surface is thread-native but the subscription rail is
  still the older conversation listener mechanism.

## Current implemented surface

| Web UI flow                 | Current implemented RPC / notification                                          | Evidence                                                                                           | Classification | Notes                                                                                                                                                                                                                                   |
| --------------------------- | ------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| WebSocket / stdio handshake | `initialize`                                                                    | `code-rs/app-server/tests/websocket_parity.rs`, `code-rs/app-server/src/message_processor.rs`      | Ready now      | Browser client still initializes once per socket with `clientInfo` and `experimentalApi=true`.                                                                                                                                          |
| List persisted sessions     | `thread/list`                                                                   | `code-rs/app-server/src/code_message_processor.rs`, `code-rs/app-server/tests/binary_smoke.rs`     | Ready now      | Returns `Thread` items with preview, source, cwd, provider, path, and timestamps.                                                                                                                                                       |
| Passive transcript read     | `thread/read`                                                                   | `code-rs/app-server/src/code_message_processor.rs`, `code-rs/app-server/tests/binary_smoke.rs`     | Ready now      | `includeTurns=true` reconstructs persisted turns from rollout history.                                                                                                                                                                  |
| Start a new session shell   | `thread/start`                                                                  | `code-rs/app-server/src/code_message_processor.rs`, `code-rs/app-server/tests/binary_smoke.rs`     | Ready now      | Returns a live thread descriptor plus the resolved runtime config.                                                                                                                                                                      |
| Resume a stored session     | `thread/resume`                                                                 | `code-rs/app-server/src/code_message_processor.rs`, `code-rs/app-server/tests/binary_smoke.rs`     | Ready now      | Path-precedence behavior is covered; resumed threads preload persisted turns.                                                                                                                                                           |
| Branch from a saved thread  | `thread/fork`                                                                   | `code-rs/app-server/src/code_message_processor.rs`, `code-rs/app-server/tests/binary_smoke.rs`     | Ready now      | Forks a thread from the saved rollout history so the browser can checkpoint or branch without inventing a new storage layer.                                                                                                           |
| Send the next prompt        | `turn/start`                                                                    | `code-rs/app-server/src/code_message_processor.rs`                                                 | Ready now      | Base prompt flow works, and the browser now uses supported `turn/start` overrides for model, working directory, and personality steering. Collaboration-mode overrides are still not wired.                                           |
| Steer the active turn       | `turn/steer`                                                                    | `code-rs/app-server/src/code_message_processor.rs`                                                 | Ready now      | Queues follow-up browser guidance against the currently active turn id instead of forcing a brand new turn.                                                                                                                            |
| Stop the active turn        | `turn/interrupt`                                                                | `code-rs/app-server/src/code_message_processor.rs`, `code-rs/app-server/tests/websocket_parity.rs` | Ready now      | Validates `threadId` plus `turnId`, interrupts the active turn, and emits `turn/completed` with `interrupted` status.                                                                                                                 |
| Stream live updates         | `addConversationListener` + `turn/started` + `turn/completed` + `codex/event/*` | `code-rs/app-server/src/code_message_processor.rs`, `code-rs/app-server/tests/websocket_parity.rs` | Ready now      | Typed turn lifecycle notifications now ride alongside the raw event rail for subscribed clients. Live WebSocket coverage now proves both `turn/started` and happy-path `turn/completed` over the same transport the browser spike uses. |

## Open constraints and intentional boundaries

| Desired target flow                        | Current state                                                                                                                     | Classification               | Why it matters                                                                                              |
| ------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------- | ---------------------------- | ----------------------------------------------------------------------------------------------------------- |
| Full typed turn lifecycle on live sessions | Happy-path `turn/started` and `turn/completed` now have live WebSocket coverage, but interrupted/error variants are still shallow | Server semantics follow-up   | The browser can subscribe now, but stop/error handling still needs the same level of end-to-end confidence. |
| Deeper external handoff flows              | Browser-side handoff drafting now exists, but provider-native auth/webhook flows are still out of scope                         | Product follow-up            | The companion can now turn GitHub/Slack/Jira/Linear URLs into thread-ready prompts, but true integrations still need a deployment story. |
| Multi-client live steering rules           | `thread/loaded/list` now lets the browser detect threads attached elsewhere and require explicit takeover before prompting; product direction remains intentionally single-user, not collaborative | Product guardrail            | The browser is optimized for one operator moving between surfaces, not for distinct users co-editing the same live session. |
| Remote browser auth                        | Browser-side connection policy now blocks raw public `ws://` targets, the workspace now spells out the supported loopback/SSH/authenticated-`wss://` topology, non-loopback endpoints require an explicit single-user acknowledgment before connect, and the repo now includes a real loopback smoke script for browser-facing validation; browser auth still depends on an external proxy/VPN layer | Intentional deployment boundary | The supported remote path is now explicit and validated locally, but the actual trust boundary still lives outside the raw app-server and anyone who clears it is effectively acting as the same operator. |

## Architecture call

The browser spike should now target the real thread-native request surface:

1. `thread/list` for inventory.
2. `thread/read` for passive transcript hydration.
3. `thread/start` and `thread/resume` for active sessions.
4. `turn/start` for prompting.
5. `turn/interrupt` for stop/abort controls.
6. `addConversationListener` only as the temporary live subscription rail.

That gives us a staged path:

1. Keep the browser proof on thread-native requests immediately.
2. Deepen live turn lifecycle semantics (`turn/completed`, interruption,
   eventually `turn/interrupt`).
3. Decide whether the browser needs a first-class thread-native subscription API
   or whether the existing listener rail is sufficient.

## Validation notes from this turn

- Direct transport coverage now proves:
  - `thread/list`
  - `thread/read`
  - `thread/start`
  - `thread/resume`
  - `turn/interrupt`
- Persisted thread history reconstruction is covered against a real rollout
  fixture in `code-rs/app-server/tests/binary_smoke.rs`.
- Listener-side event translation now emits typed `turn/started` and
  `turn/completed` notifications for subscribed clients.
- WebSocket parity coverage now proves `thread/list`, `thread/start`,
  `turn/interrupt`, `addConversationListener`, and live `turn/started` / `turn/completed`
  delivery over the same transport the browser spike uses.
- The repo now also includes `scripts/smoke-shared-session-loopback.mjs` for a
  real local browser-path smoke against `code-app-server`, and that smoke was
  run successfully against a live `ws://127.0.0.1:8877` server.

## Browser companion state

- The browser companion now treats ownership more explicitly:
  - settled threads surface `Attach live updates`,
  - remotely running turns surface `Take over here`,
  - prompt send is blocked while another surface is actively running the turn.
- Transcript review now applies local syntax-aware tinting to assistant code
  blocks, tool output, and diff hunks without adding a new highlighting
  dependency.
- Transcript review now also exposes a local review board that groups diffs,
  code blocks, tool output, and failures into a faster artifact pass, with
  per-artifact notes persisted in local browser storage for that thread.
- `/transcript-lab` now renders the real `TranscriptThreadView` against the
  shared fixture so review-board and live-follow changes can be visually
  validated without depending on a particularly rich live app-server thread.
- Live-follow state now tracks both newly appended cells and in-place changes
  to the current live cell, so the transcript can surface backlog chips and a
  blocker-aware sticky jump control even when streaming stays inside one cell.
- The browser companion now also uses raw `codex/event/*` listener traffic to
  quietly re-hydrate the selected live thread, so transcript cells keep up with
  active-turn updates more closely than the old turn-start/turn-complete-only
  path.
- The workspace now exposes a cross-thread unblock inbox so blocked approvals
  and grouped question sets are visible before the correct thread is opened.
- Thread navigation now keeps local unread cues, pinning, and a stronger
  recent-handoff strip so the browser can return to the threads you last
  touched instead of only sorting by server update time.
- When another surface is still mid-turn, `Take over here` now queues browser
  intent instead of immediately trying to seize the session; the browser
  attaches live updates automatically once that turn settles, and reconnect
  preserves the queued takeover state.
- The WebUI now has bounded auto-reconnect on unexpected socket loss and will
  attempt to restore the previously selected thread after reconnect.
- Mobile/browser-away use now has a compact bottom action bar for the selected
  thread, blocker/completion-only browser notifications with deep links,
  foreground resync, optional wake-lock support during live turns, voice input
  in the composer, and a basic PWA install path (manifest + service worker +
  install CTA).
- The composer now also exposes lightweight turn steering for the selected
  thread: model and working-directory overrides are sent through `turn/start`
  and persisted onto the loaded thread before the new turn begins.
- The browser companion now also classifies connection targets before opening a
  socket: loopback `ws://` remains the default, private-network `ws://`
  addresses are marked trusted-LAN-only, and raw remote `ws://` endpoints are
  blocked so the user has to switch to `wss://` or an SSH tunnel story.
- The workspace connection panel now also renders the supported deployment
  shape for the chosen address, and `docs/shared-session-remote-access.md`
  records the operator guidance for loopback-only, SSH-tunneled, trusted-LAN,
  and authenticated-`wss://` proxy access.
- Remote guidance now also states the product boundary directly: this is still
  a single-user companion surface, so anyone who can clear the same tunnel,
  VPN, or authenticated proxy is effectively acting as the same operator.
- The workspace now also enforces that boundary at connect time for non-loopback
  targets: the operator must explicitly acknowledge that trusted-LAN and
  authenticated-`wss://` endpoints are still personal surfaces before the
  browser will connect.
- That same remote-boundary reminder now stays visible after connect inside the
  connected workspace status surfaces, so non-loopback sessions keep their
  single-user trust framing even when the connection settings are collapsed.
- The browser companion now also calls `thread/loaded/list` during inventory
  refresh so it can distinguish `Live` threads from threads merely `Attached
  elsewhere`, and it blocks prompt send on those attached-elsewhere threads
  until the user explicitly takes over.
