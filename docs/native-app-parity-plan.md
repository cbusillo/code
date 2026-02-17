# Native App Parity Roadmap

This roadmap is the execution source of truth for bringing the native app to
high-function parity with Every Code TUI and selected Codex Mac app strengths,
while preserving a polished, intentional UX.

## Product Goal

Ship a native app that can handle full daily workflows (plan, execute, review,
approve, recover, and audit) with:

- TUI-level capability coverage for core coding flows.
- Mac-native interaction quality and visual clarity.
- Reliable long-session behavior (history, reconnect, performance).

## Success Criteria (Definition of Success)

- Users can complete end-to-end coding tasks in native without switching to TUI
  for core flows.
- Transcript and activity views remain smooth in long sessions (>20k events).
- Approval and task state are unambiguous and keyboard-friendly.
- Visual quality is consistently high across standard screenshot benchmarks.
- Validation gates pass on every milestone (`swift test`, `./build-fast.sh`,
  screenshot diff checks).

## Scope and Assumptions

### In Scope

- Feature parity for transcript/activity/approvals/exec/session controls.
- Screenshot-driven visual benchmarking workflow.
- Reliability and performance hardening for long-running sessions.
- UX standards covering hierarchy, typography, motion, and spacing.

### Out of Scope

- Full replacement of TUI-only expert/debug tooling in initial milestones.
- Net-new product surfaces unrelated to parity roadmap.

### Assumptions

- Prioritize parity and reliability before net-new exploratory features.
- Favor incremental protocol changes with backward-compatible decoding.
- Treat compiler warnings as failures and keep the build clean at each stage.

## Agent Synthesis (What We Are Parity-Matching)

This plan synthesizes the two planning-agent audits into one execution backlog.

### High-Value Every Code TUI Capabilities To Bring Forward

- Long-session transcript reliability with deterministic replay/pagination.
- Clear task/activity hierarchy for planning, execution, and review loops.
- Strong approval flows (exec/apply patch/request-user-input) with explicit
  state and keyboard reachability.
- Command ergonomics (`/` launch patterns, context insertion, fast review
  loops).
- Git and recovery affordances (diff/snapshot/rollback awareness in context).

### High-Value Codex Mac App Strengths To Preserve/Import

- Native-quality information hierarchy and visual calm under dense activity.
- Session organization and thread ergonomics suitable for large catalogs.
- Voice and accessibility affordances as first-class interaction paths.
- Reliable IDE handoff/file opening with predictable fallbacks.

## Phase 0: Stabilization Pass (Required Before New Parity Surface Area)

Objective: remove transcript/history risk and establish safe baselines.

### Work Items

1. Finalize paged history robustness:
   - no duplicate/skip across attach + history pages
   - reconnect-safe in-flight pagination
   - deterministic `has_more_before` semantics
2. Harden scroll behavior:
   - stable top-anchor prepend
   - prefetch with throttle
   - no visible jump on page prepend
3. Add history telemetry for tuning:
   - page request count/success/latency/slow-page count
4. Establish baseline screenshot suite:
   - transcript idle, long transcript, activity-heavy thread,
     approval pending, disconnected, settings

### Phase 0 Acceptance Criteria

- 30-minute active session with continuous message activity shows no reconnect
  flapping or transcript corruption.
- Upward scrolling can reach start-of-session sentinel for long threads.
- Pagination-induced scroll jump is not perceptible in benchmark scenarios.
- Telemetry is visible and records non-zero values under paging activity.
- Validation gates pass.

## Prioritized Feature Matrix

The detailed matrix lives in `docs/native-app-parity-matrix.md`.

Priority model:

- `P0`: must-have for “native daily-driver” viability.
- `P1`: high-value parity and quality lift.
- `P2`: strategic enhancements and advanced parity.

## Milestone Plan

## Milestone 1 (Auto Drive Ready): Core Workflow Parity Foundation

Goal: ship complete primary workflow loop in native.

### Milestone 1 Scope

- Approval UX parity (exec/apply patch/request-user-input) with strong visual
  prominence and keyboard reachability.
- Task/activity timeline polish with anchored updates and compact hierarchy.
- Command/exec card improvements (live state, duration, exit status, concise
  output preview).
- Session reliability states: `Connected`, `Reconnecting`, `History loading`,
  `History complete`, `Unavailable`.
- Screenshot benchmark workflow and baseline capture integrated into docs.

### Milestone 1 DoD

- P0 items tagged `M1` in the matrix are complete.
- All Milestone 1 acceptance criteria in matrix pass.
- Screenshot benchmark set captured and reviewed with no high-severity visual
  regressions.
- Validation gates pass.

### Auto Drive Execution Brief (Milestone 1)

Use this as kickoff prompt:

```text
Execute Milestone 1 from docs/native-app-parity-plan.md and
docs/native-app-parity-matrix.md. Implement all P0 items marked M1 with tests,
preserve current architecture, and keep transcript performance smooth.
Validate using swift test --package-path native/CodeNative and ./build-fast.sh.
Also run screenshot benchmarking workflow and summarize visual diffs.
```

## Milestone 2 (Auto Drive Ready): High-Value Parity + UX Elevation

Goal: close the largest feature/UX gaps and establish “premium native” quality.

### Milestone 2 Scope

- Slash-command parity surface for highest-frequency commands (native command
  launcher + mapped actions).
- File/context mention ergonomics (`@`-style picker and insertion flow).
- Git workflow parity slice (diff/snapshot timeline visibility and recovery
  affordances).
- Visual quality pass: hierarchy, spacing, typography, and motion consistency
  aligned to benchmark rubric.
- Expanded screenshot suite (dense sessions, approval edge cases, settings
  extremes, reconnect states).

### Milestone 2 DoD

- P0/P1 items tagged `M2` in matrix are complete.
- No unresolved high-severity UX rubric violations.
- Validation gates pass.

Milestone 2 completion proof lives in
`docs/native-app-parity-matrix.md` with all `M2` rows marked `present`
(`PAR-011/012/013/014/015/016/019/022`) and deterministic evidence including
`session-rail-scale` benchmark artifacts.

### Auto Drive Execution Brief (Milestone 2)

Use this as kickoff prompt:

```text
Execute Milestone 2 from docs/native-app-parity-plan.md and
docs/native-app-parity-matrix.md. Implement all P0/P1 items marked M2,
including command launcher parity, mention flow, and git parity slice.
Keep UX standards from the roadmap. Run screenshot benchmark workflow,
swift test --package-path native/CodeNative, and ./build-fast.sh.
```

## Milestone 3: Advanced/Strategic Parity

Goal: close remaining advanced parity and differentiation work.

- Multi-agent orchestration visualization parity (plan/progress/audit cards).
- Browser/screenshot investigative workflows (including side-by-side diffs where
  appropriate).
- Advanced diagnostics and timeline drill-down.

Current progress snapshot:

- PAR-021 (`voice interaction parity hardening`) is complete with deterministic
  `voice-guardrails` benchmark coverage.
- PAR-018 (`browser workflow parity`) is complete with deterministic
  `browser-workflow` benchmark coverage and transcript card state validation.
- PAR-017 (`multi-agent progress visualization`) is complete with deterministic
  `multi-agent-progress` benchmark coverage and coordinator state-card
  validation.

Milestone 3 status: all planned rows (PAR-017/018/021) are now present.

## Dependencies and Sequencing

1. **Protocol and reducer integrity first**
   - session pagination/replay semantics
   - stable ordering/merging guarantees
2. **UI state model second**
   - explicit connection/history/task states
3. **UX polish and command ergonomics third**
   - only after state correctness and reliability are locked

Hard dependency: Milestone 1 starts only after Phase 0 acceptance criteria pass.

## UX and Visual Standards (Non-Negotiable)

### Information Hierarchy

- Primary: user/assistant core conversation and active approvals.
- Secondary: task/activity detail and execution context.
- Tertiary: raw metadata, IDs, and implementation-level traces.

### Transcript Ergonomics

- No jump during prepend/append updates.
- Long content remains readable with consistent wrapping and spacing.
- Activity detail remains attached to parent task context.

### Approval Experience

- Pending approvals visually dominate surrounding non-critical cards.
- Decision controls are keyboard-accessible and explicit.
- Post-decision state collapses to concise durable summary (not disappearance).

### Motion and Spacing

- Motion is meaningful, short, and non-distracting.
- No animation for high-frequency streaming text reflows.
- Use consistent spacing scale; avoid ad-hoc per-view spacing drift.

## Reliability and Performance Guardrails

- No data loss on reconnect and no duplicate replay rows after reattach.
- Bounded payloads for replay/history page transport.
- History page requests throttled; no request storms.
- Render path avoids expensive full-list recomputation on incremental updates.
- Keep typing responsiveness unaffected during history pagination.

## Screenshot Benchmarking Workflow

The workflow is mandatory for UX-affecting changes.

We can run and interact with Every Code TUI, Codex Mac app, and the native app
locally, then capture screenshots for parity benchmarking. This is a required
part of milestone validation, not optional polish.

### Tooling

- Native automation runner:

```bash
swift run --package-path native/CodeNative CodeNativeAutomation --scenario <scenario.json>
```

- Scenario directory:
  `native/CodeNative/automation/benchmarks/`

- Deterministic fixture directory:
  `native/CodeNative/automation/benchmarks/fixtures/`

- One-shot benchmark runner:

```bash
scripts/ux/benchmark-native-ui.sh
```

- Cross-app parity captures:
  - Every Code TUI: launch representative scenario and capture terminal window.
  - Codex Mac app: open matching scenario state and capture app window.
  - Native app: capture with automation scenario runner.
  - Store labeled captures under `docs/reference/native-ui/parity/` using
    stable naming (`<scenario>-tui.png`, `<scenario>-codex-mac.png`,
    `<scenario>-native.png`).

### Benchmark Set (Minimum)

1. `transcript-idle`
2. `transcript-long`
3. `activity-heavy`
4. `approval-pending`
5. `voice-guardrails`
6. `request-user-input`
7. `request-user-input-depth`
8. `request-user-input-error`
9. `command-launcher`
10. `command-launcher-depth`
11. `context-mention`
12. `context-mention-depth`
13. `git-recovery`
14. `ide-integration`
15. `settings-workflow-controls`
16. `session-rail-scale`
17. `disconnected-state`
18. `settings-shell`

### Review Process

1. Capture current screenshots for benchmark set.
2. Capture cross-app parity triplets (`tui`, `codex-mac`, `native`) for at
   least one representative scenario per milestone.
3. Compare against approved baseline.
4. Mark regressions by severity (`high`, `medium`, `low`).
5. Block merge for any unresolved high-severity regression.

## Validation Gates (Explicit)

Every milestone PR (or milestone batch) must pass all gates:

1. **Code gates**
   - `swift test --package-path native/CodeNative`
   - `./build-fast.sh`
2. **Protocol/reducer gates**
   - replay/pagination merge tests updated and passing
3. **UX gates**
   - benchmark screenshot set captured and reviewed
   - cross-app parity capture triplet included for milestone scenario(s)
   - no unresolved high-severity visual regressions
4. **Runtime gates**
   - manual reconnect + history stress scenario sanity pass

## Implementation Start Checklist

Start execution without additional decisions when all are true:

- [ ] Phase 0 acceptance criteria passed
- [ ] Matrix priorities confirmed in docs
- [ ] Benchmark baselines captured
- [ ] Auto Drive milestone brief selected (`M1` or `M2`)
