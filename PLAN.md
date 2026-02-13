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

## Active Plan (Next)

### P0: Mirror Determinism (Must Hold)

- [ ] Add deterministic reconnect/reattach tests for TUI/native parity paths.
- [ ] Add server test coverage for attach replay + high-water edge cases.
- [ ] Add native store tests for stale/out-of-order live events and duplicate replay.
- [ ] Validate two-client same-session mirroring under detach/reattach cycles.

### P1: macOS Product Polish (No Feature Gaps in Core Flow)

- [ ] Final macOS transcript parity pass (dense history, long diffs, approvals).
- [ ] Keyboard pass for primary workflows (compose, interrupt, approvals, settings).
- [ ] Remove/avoid placeholder UX text on primary surfaces.
- [ ] Keep controls visible and stateful (model/reasoning/sandbox/approval selectors).

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

## Validation Gates (Required)

- `swift build --package-path native/CodeNative`
- `xcodebuild -project native/CodeNativeiOS/CodeNativeiOSDemo.xcodeproj`
  `-scheme CodeNativeiOSDemo -destination 'generic/platform=iOS Simulator'`
  `build`
- `./build-fast.sh`
- `./pre-release.sh` now includes native build validation on macOS.

## Deferred (Pruned From Active Work)

- iPadOS/iOS parity milestones (resume after macOS P0/P1 are stable).
- On-device ML enhancements beyond core session correctness.
