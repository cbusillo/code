# Auto Drive Parity Boundary

This note supports [#398](https://github.com/cbusillo/code/issues/398) and the
Codex fork parity ledger. It classifies the pre-#390 Auto Drive evidence from
`fa0c33944f` against current `origin/main` so future slices preserve the user
value without restoring the old coordinator wholesale.

Current `main` does not expose `/auto`, `code exec --auto`, the old Auto Drive
card, the old Auto Drive settings pane, or the old Auto Drive VT100/performance
tests. Stale branch/worktree evidence should not be counted as current coverage.
Current `main` does have Codex-shaped primitives that should host the
replacement: thread goals, plan/review commands, guardian review events, token
accounting, generic dynamic tool rendering, agent delegation, and session
resume/rollout infrastructure.

## Classification

| Old evidence | User value | Classification | Replacement boundary |
| --- | --- | --- | --- |
| `code-rs/code-auto-drive-core/src/controller.rs` | Continue modes, countdown/manual approval, run phases, stop/pause semantics. | Rewrite | Rebuild as a small state machine over current TUI task/session events. Add state-transition fixtures before UI ports. |
| `code-rs/code-auto-drive-core/src/auto_coordinator.rs` | Maintainer-style loop that plans, delegates, runs checks, compacts, and resumes. | Rewrite | Use current Codex thread/session/goal primitives rather than reintroducing a separate coordinator crate as the source of truth. |
| `code-rs/code-auto-drive-core/src/coordinator_router.rs` | Route user status/plan/stop messages while automation is active. | Rewrite | Map to current slash-command/queued-command and goal surfaces; avoid a parallel command router unless fixtures prove it is needed. |
| `code-rs/code-auto-drive-core/src/retry.rs` | Bounded retry, backoff, rate-limit waits, and terminal failure status. | Rewrite | Preserve the retry contract in focused unit tests before wiring it into any long-running coordinator. |
| `code-rs/code-auto-drive-diagnostics/` | Structured completion/QA check before declaring success. | Rewrite | Prefer current review/guardian/check-result primitives; keep a forced-JSON completion fixture only if guardian review does not cover the guarantee. |
| `code-rs/core/src/auto_drive_pid.rs` | Prevent duplicate Auto Drive ownership. | Rewrite | Reuse current session/task ownership and worktree/branch identity guards; add a duplicate-run fixture before adding a PID file. |
| `code-rs/tui/src/bottom_pane/auto_drive_settings_view.rs` | User-facing defaults for agents, review, diagnostics, continue mode, model routing. | Defer | Restore settings only after the coordinator contract exists; favor existing settings panels and Codex config names where possible. |
| `code-rs/tui/src/chatwidget/auto_drive_cards.rs`, `code-rs/tui/src/history_cell/auto_drive.rs`, `vt100_chatwidget_snapshot__auto_drive_*.snap` | Visible run state, action log, countdown, review footer, success/failure status, long-session rendering performance. | Rewrite | Render through current generic TUI/history surfaces unless a dedicated Auto Drive card fixture proves a gap. |
| `code-rs/tui/tests/auto_drive_long_session_perf.rs` | Countdown ticks must not force full-history rerendering. | Port concept | Recreate against current TUI rendering once an Auto Drive status/countdown surface exists. |
| `docs/auto-drive.md`, `docs/settings.md`, `docs/agents.md`, `docs/config.md` | Product promise and operator model. | Rewrite | Keep as intent/status docs while #398 is open; do not claim current main has live Auto Drive commands. |

## First Fixtures

1. Continue-mode state machine: immediate, ten-second, sixty-second, and manual
   modes produce the expected submit/wait/stop transitions without model calls.
2. Goal handling: no-goal/no-history fails cheaply; explicit goals become a
   current thread goal rather than a separate coordinator-only state.
3. Review gate: a coordinator success candidate can route through current
   review/guardian evidence before continuing or finishing.
4. Duplicate ownership: a second Auto Drive start for the same session/worktree
   is rejected or resumes the existing run deterministically.
5. Long-session rendering: countdown/status updates do not rebuild the full
   transcript once the current TUI surface exists.

## Retired For Now

- Do not restore `code-auto-drive-core` and `code-auto-drive-diagnostics` as
  standalone crates just to regain old module boundaries.
- Do not restore old browser-style/history-cell visual architecture for Auto
  Drive unless a focused snapshot proves current generic rendering cannot carry
  the required state.
- Do not restore the old Auto Review proof store as part of Auto Drive; #400
  maps review proof metrics onto current Codex review/guardian primitives.

## Open Questions

- Whether `/auto` should return as a dedicated slash command or whether the
  first usable loop should be expressed as a goal/plan/review mode over existing
  commands.
- Whether `code exec --auto` should become a CLI shorthand for a deterministic
  goal-driven loop, or remain unavailable until TUI and headless behavior share
  one state machine.
- Which settings belong in persisted config versus a per-run prompt, especially
  model routing and agent toggles.
