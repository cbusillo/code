# Native Apple Apps Plan (Active)

## Scope (Current)

- Primary target: macOS quality + mirror correctness.
- Principle: TUI and native clients must mirror the same session state.
- Rule: keep primary controls visible and stable (no surprise UI).

## Completed Baseline

- Native shell + transcript + session rail are functional.
- Core session operations are wired: list/create/attach/detach/submit/interrupt.
- Approvals and tool/event cards are present.
- Replay attach now honors high-water semantics (`from_seq`).
- Native stream reducer now normalizes/dedupes sequence ordering.
- Server forwarder high-water filtering has deterministic unit coverage.
- Dev harness now waits for backend readiness and defaults to ownership-safe cleanup.
- Server tests now cover `from_seq` fallback high-water and incremental replay
  payload-size behavior.
- Native reducer tests cover stale `session_attached` acceptance rules.
- Server tests now cover two-client detach/reattach mirror behavior with
  high-water duplicate suppression.
- Native state-store tests now cover stale/out-of-order live events and
  duplicate incremental replay handling.

## Active Plan (Next)

### P0: Mirror Determinism (Must Hold)

- [x] Lock replay/live invariants in tests (`seq` monotonicity,
  `from_seq` semantics, no replay+live duplicates).
- [x] Add deterministic reconnect/reattach integration tests for TUI/native parity.
- [x] Add server coverage for attach replay + high-water edges,
  including `from_seq > 0` payload-size policy.
- [x] Add native state-store tests for stale/out-of-order live events
  and duplicate replay handling.
- [x] Add automated two-client same-session mirror test
  through detach/reattach cycles.

### P1: macOS Product Polish (No Feature Gaps in Core Flow)

- [ ] Align macOS transcript behavior with TUI for dense history,
  long diffs, and approvals.
- [x] Implement keybindings + focus management for compose,
  interrupt, approvals, and settings.
- [ ] Remove/avoid placeholder UX text on primary surfaces.
- [ ] Keep primary controls persistently visible and correctly
  stateful (model/reasoning/sandbox/approval).

### P2: Hardening + Release Readiness

- [ ] Add crash/telemetry basics for native session and transport failures.
- [ ] Security pass for local data and permissions.
- [ ] Define release automation for native artifacts.

## Dev Harness Policy (Current)

- `scripts/dev-native.sh` is for interactive local dev, not shared automation.
- Automation runs must use an isolated backend port to avoid process contention.
- Dev harness cleanup is ownership-aware by default (only kills its own backend).
- Use `--takeover-port` only when you explicitly want to reclaim a port from
  another `code web` process.

## Harness Follow-Ups

- [x] Move backend/app teardown to process-group cleanup to avoid orphaned children.
- [x] Evaluate event-driven watch mode (or slower polling) to reduce CPU churn.

## Validation Gates (Required)

- `swift build --package-path native/CodeNative`
- `xcodebuild -project native/CodeNativeiOS/CodeNativeiOSDemo.xcodeproj`
  `-scheme CodeNativeiOSDemo -destination 'generic/platform=iOS Simulator'`
  `build`
- `./build-fast.sh`
- `./pre-release.sh` now includes native build validation on macOS.

## Fast PR Gates (Recommended)

- `swift test --package-path native/CodeNative`
- `cargo test --manifest-path code-rs/Cargo.toml -p code-app-server`

## Deferred (Pruned From Active Work)

- iPadOS/iOS parity milestones (resume after macOS P0/P1 are stable).
- On-device ML enhancements beyond core session correctness.
