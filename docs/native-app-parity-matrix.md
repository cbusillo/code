<!-- markdownlint-disable MD013 -->

# Native App Parity Matrix

This matrix maps Every Code TUI and Codex Mac app capabilities to native app
parity work with explicit priorities, milestone assignment, dependencies,
acceptance criteria, and validation gates.

Legend:

- `Status`: `present`, `partial`, `missing`
- `Priority`: `P0` (must-have), `P1` (high value), `P2` (strategic)
- `Milestone`: `Phase0`, `M1`, `M2`, `M3`

## Matrix Summary

| ID | Capability | Source | Status | Priority | Milestone |
| --- | --- | --- | --- | --- | --- |
| PAR-001 | Stable upward history pagination | Every Code TUI | partial | P0 | Phase0 |
| PAR-002 | Reconnect-safe history + attach behavior | Every Code TUI | partial | P0 | Phase0 |
| PAR-003 | History transport payload safety | Every Code TUI | partial | P0 | Phase0 |
| PAR-004 | Transcript anchor-preserving prepend | Every Code TUI | partial | P0 | Phase0 |
| PAR-005 | Activity/task anchored streaming card | Every Code TUI | partial | P0 | M1 |
| PAR-006 | Approval card prominence + keyboard flow | Every Code TUI | partial | P0 | M1 |
| PAR-007 | Exec card lifecycle clarity | Every Code TUI | partial | P0 | M1 |
| PAR-008 | Explicit connection/history runtime states | Every Code TUI | partial | P0 | M1 |
| PAR-009 | Auto-review summary durability | Every Code TUI | partial | P0 | M1 |
| PAR-010 | Screenshot benchmark harness + baseline | Mac + native | partial | P0 | M1 |
| PAR-011 | Slash command launcher parity (core set) | Every Code TUI | present | P1 | M2 |
| PAR-012 | Mention-style context insertion | Every Code TUI | present | P1 | M2 |
| PAR-013 | Git diff/snapshot recovery surface | Every Code TUI | present | P1 | M2 |
| PAR-014 | Request-user-input parity UI | Every Code TUI | present | P1 | M2 |
| PAR-015 | Settings parity for core workflow controls | TUI + Mac | present | P1 | M2 |
| PAR-016 | IDE integration robustness | Mac + native | present | P1 | M2 |
| PAR-017 | Multi-agent progress visualization | Every Code TUI | missing | P2 | M3 |
| PAR-018 | Browser workflow parity | Every Code TUI | present | P2 | M3 |
| PAR-019 | Visual quality rubric enforcement | Mac-inspired | present | P1 | M2 |
| PAR-020 | Performance guardrails + telemetry | TUI + native | partial | P0 | M1 |
| PAR-021 | Voice interaction parity hardening | Codex Mac | present | P2 | M3 |
| PAR-022 | Session rail/grouping ergonomics at scale | Codex Mac | present | P1 | M2 |
| PAR-023 | Cross-app screenshot parity evidence | TUI + Codex Mac + native | partial | P0 | M1 |

## Detailed Rows

## PAR-001

- Dependencies: session replay protocol.
- Acceptance criteria: user can scroll to start-of-session sentinel on long
  thread with no skipped or duplicate rows.
- Validation gate: reducer tests + manual long-scroll scenario.

## PAR-002

- Dependencies: PAR-001.
- Acceptance criteria: reconnect during page load preserves ordering and does
  not drop visible transcript content.
- Validation gate: reconnect stress scenario + `swift test`.

## PAR-003

- Dependencies: PAR-001.
- Acceptance criteria: attach/page responses stay within configured budgets and
  continued paging still works.
- Validation gate: app-server tests + `./build-fast.sh`.

## PAR-004

- Dependencies: PAR-001.
- Acceptance criteria: no perceptible jump while older pages prepend near top.
- Validation gate: screenshot benchmark + manual top-scroll check.

## PAR-005

- Dependencies: Phase0 complete.
- Acceptance criteria: task-started card continuously aggregates relevant
  background lines and final summary.
- Validation gate: transcript rendering tests + screenshot review.

## PAR-006

- Dependencies: Phase0 complete.
- Acceptance criteria: pending approval card is visually prominent and full
  approve/deny flow is keyboard-accessible.
- Validation gate: approval interaction scenario + `swift test`.

## PAR-007

- Dependencies: PAR-005.
- Acceptance criteria: running state, duration, exit status, and concise output
  preview are always clear.
- Validation gate: execution scenario screenshots + protocol tests.

## PAR-008

- Dependencies: PAR-002.
- Acceptance criteria: deterministic runtime states are shown for connected,
  reconnecting, history loading, history complete, and unavailable.
- Validation gate: reconnect scenario + screenshot benchmark.

## PAR-009

- Dependencies: PAR-005.
- Acceptance criteria: auto-review summaries remain visible as user-facing
  outcomes in task context.
- Validation gate: transcript tests + manual thread inspection.

## PAR-010

- Dependencies: Phase0 complete.
- Acceptance criteria: minimum benchmark suite is runnable and baseline artifacts
  exist for required states.
- Validation gate: benchmark artifact review + docs checklist.

## PAR-011

- Dependencies: M1 complete.
- Acceptance criteria: native command launcher supports highest-frequency
  planning/execution/review commands.
- Validation gate: command launcher tests + UX benchmark.
- Progress: launcher now covers high-frequency slash workflows (`/plan`,
  `/code`, `/solve`, `/review`, `/status`, `/test`, `/diff`, `/undo`,
  `/mention`) with native quick actions for thread/settings/context. Keyboard
  depth now supports Return-select, arrow-key selection movement, Escape-close,
  and deterministic 1-9 quick-pick shortcuts, with benchmark evidence from
  `command-launcher` + `command-launcher-depth` scenarios.

## PAR-012

- Dependencies: PAR-011.
- Acceptance criteria: file/context references are insertable via picker with
  deterministic formatting.
- Validation gate: composer interaction scenario + screenshot review.
- Progress: mention flow now supports punctuation/quoted triggers, ranked
  filtering that prioritizes filename matches, stable quoted-path insertion,
  keyboard-first inline selection (`Tab`, arrow navigation) and context-picker
  keyboard controls (`Return`, arrow navigation, `Esc`, quick 1-9). Benchmarks
  now include `context-mention` + `context-mention-depth` deterministic
  scenarios.

## PAR-013

- Dependencies: PAR-011.
- Acceptance criteria: native exposes diff context and rollback/recovery
  affordances sufficient for daily review loops.
- Validation gate: workflow scenario + docs validation.
- Progress: dedicated diff recovery module and transcript card actions now expose
  copyable review/snapshot/restore/apply commands with changed-file context;
  deterministic evidence captured via `git-recovery` benchmark scenario.

## PAR-014

- Dependencies: PAR-006.
- Acceptance criteria: request-user-input cards support option selection, notes,
  submit, and skip semantics.
- Validation gate: request-user-input scenario + `swift test`.
- Progress: request-user-input cards now include full question fidelity
  (multi-question, option metadata, secret/note fields), guarded action
  handling (send disabled until answers exist, submitted-state lock), explicit
  loading/empty/error fallback states for malformed payloads, and improved
  keyboard/accessibility behavior (Return submit, Escape skip, numeric option
  shortcuts for primary question). Deterministic evidence captured via
  `request-user-input`, `request-user-input-depth`, and
  `request-user-input-error` scenarios.

## PAR-015

- Dependencies: M1 complete.
- Acceptance criteria: required workflow settings are discoverable/editable with
  no context switching.
- Validation gate: settings benchmark + manual checklist.
- Progress: general settings now expose editable workflow defaults (model,
  reasoning, sandbox, approvals, IDE context, default IDE/open destination),
  with persisted normalization and deterministic benchmark evidence via the
  `settings-workflow-controls` scenario.

## PAR-016

- Dependencies: M1 complete.
- Acceptance criteria: IDE picker shows installed apps only, session-scoped
  selection persists, and file opens are reliable with fallback.
- Validation gate: IDE open scenario + manual verification.
- Progress: installed-only IDE availability now uses bundle/name app resolution,
  session IDE preferences are persisted and pruned per thread, and open-in-IDE
  paths report explicit failure/fallback messaging. Deterministic evidence added
  via `ide-integration` benchmark scenario and fixture.

## PAR-017

- Dependencies: M2 complete.
- Acceptance criteria: native surfaces plan/progress/task states for multi-agent
  runs with clear hierarchy.
- Validation gate: workflow scenario + screenshot benchmark.

## PAR-018

- Dependencies: M2 complete.
- Acceptance criteria: browser-related events/artifacts are visible and
  understandable in transcript/activity context.
- Validation gate: browser scenario + regression checks.
- Progress: native transcript now renders dedicated browser workflow cards for
  `web_search_begin`/`web_search_end` and browser MCP tool calls, with explicit
  in-progress/completed/error status chips, readable metadata lines, and
  artifact previews for completed/failed tool results. Browser workflow events
  are now treated as optional activity rows (controlled by the activity toggle)
  and benchmark proof is captured via the deterministic `browser-workflow`
  fixture/scenario.

## PAR-019

- Dependencies: PAR-010.
- Acceptance criteria: no unresolved high-severity visual issues across
  benchmark suite after milestone changes.
- Validation gate: screenshot diff review gate.
- Progress: transcript cards now enforce rubric-consistent hierarchy with
  gradient depth, adaptive contrast, and focus-preserving active states while
  keeping assistant content legible. Request-user-input option shortcuts are
  now safely constrained to single-digit keys (`1`-`9`) to avoid invalid
  key-binding crashes when option counts exceed 9. Deterministic proof comes
  from `scripts/ux/benchmark-native-ui.sh` fixture-backed scenarios including
  `transcript-long`, `activity-heavy`, `approval-pending`,
  `request-user-input-depth`, and `settings-workflow-controls`, plus cross-app
  parity triplets under `docs/reference/native-ui/parity/`.

## PAR-020

- Dependencies: Phase0 complete.
- Acceptance criteria: paging/scroll telemetry is available and typing remains
  responsive during pagination.
- Validation gate: telemetry spot-check + manual typing stress.

## PAR-021

- Dependencies: M2 complete.
- Acceptance criteria: voice capture/playback states remain clear and
  non-disruptive during active transcript streaming and approvals.
- Validation gate: voice interaction scenario + screenshot review.
- Progress: voice capture now applies explicit guardrails for reconnect,
  no-session, approval-pending, and active-stream states; active recordings are
  safely stopped on guard transitions, and auto-submit is blocked whenever a
  guard applies to prevent accidental sends. Interrupt actions now stop both
  capture and speech playback, and auto-submitted captures clear transcript
  state to avoid stale/stuck voice badges. Deterministic evidence is captured
  through the `voice-guardrails` benchmark fixture/scenario, alongside
  regression tests for voice policy decisions.

## PAR-022

- Dependencies: M1 complete.
- Acceptance criteria: thread rail remains fast and legible at large session
  counts with grouping/density controls behaving predictably.
- Validation gate: large-catalog scenario + manual UX checklist.
- Progress: thread rail now uses lazy rendering with explicit rail controls for
  grouping mode, density, and visible-row caps. Repository groups support
  stable collapse/expand behavior with per-group counts, and truncation
  affordances (`show more` / `show all`) keep large catalogs responsive without
  hiding the currently selected thread. Deterministic evidence is captured in
  `session-rail-scale` benchmark fixture/scenario, with updated benchmark gate
  docs and targeted layout tests.

## PAR-023

- Dependencies: PAR-010.
- Acceptance criteria: each milestone includes at least one matched screenshot
  triplet (`tui`, `codex-mac`, `native`) for a representative workflow state.
- Validation gate: parity screenshot artifact review.

## Milestone Rollup

- `Phase0`: PAR-001, PAR-002, PAR-003, PAR-004
- `M1`: PAR-005, PAR-006, PAR-007, PAR-008, PAR-009, PAR-010, PAR-020, PAR-023
- `M2`: PAR-011, PAR-012, PAR-013, PAR-014, PAR-015, PAR-016, PAR-019, PAR-022
- `M3`: PAR-017, PAR-018, PAR-021

## Matrix Operating Rules

- A row is complete only when acceptance criteria and validation gate both pass.
- Any unresolved `P0` row blocks milestone sign-off.
- Any unresolved high-severity screenshot regression blocks sign-off for rows
  with screenshot validation.

<!-- markdownlint-enable MD013 -->
