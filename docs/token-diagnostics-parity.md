# Token Diagnostics Parity

This note supports [#401](https://github.com/cbusillo/code/issues/401) and the
Codex fork parity ledger. Current `main` already carries cached input token
counts from the Codex-base token pipeline through app-server replay and TUI
status updates, but the pre-#390 Every Code diagnostic surface also promised a
visible prompt-cache signal.

## Current Coverage

- `thread/tokenUsage/updated` carries `cachedInputTokens` in total and last-turn
  token usage payloads.
- App-server resume/fork replay restores token usage from rollout `TokenCount`
  records, including non-zero cached input fixtures.
- `/status` renders a derived prompt-cache hit rate when cached input tokens are
  non-zero.
- The configurable footer status line supports the `prompt-cache-hit-rate` item,
  which is omitted until cached input tokens are non-zero.

## Remaining Gaps

- The core token model still collapses missing cached-input telemetry and true
  zero cached input into `cached_input_tokens = 0`.
- Schema-level `cached telemetry reported` semantics remain a separate design
  decision before clients can distinguish absent telemetry from a real 0% hit.
- The old rolling-average cache signal is not restored in this slice.
- The old `/limits` overlay and local account-usage history remain separate
  parity decisions from the prompt-cache signal.

## Follow-Up Fixtures

- Provider/parser fixture that proves whether cached-input telemetry was present
  before it reaches `TokenUsage`.
- Protocol fixture for telemetry-present versus telemetry-absent token usage.
- Footer/status-line fixture for a rolling prompt-cache average if that signal
  remains a required Every Code behavior.
