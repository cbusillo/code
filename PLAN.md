# Native Apple Apps Plan (macOS / iPadOS / iOS)

## Status

- Active plan: native-first Apple apps.
- Previous WebUI plan is shelved after M1 CP2.
- Web mirror infrastructure remains in-repo as reusable transport/session work.
- Visual reference baseline captured from Codex app screenshots at
  `docs/reference/codex-app-ui-2026-02-12.md`.

## Decision Summary

We are pivoting from browser-first UX to first-class native Apple apps.
The primary near-term differentiator is high-quality voice conversation with
Codex (speech in, speech out), with low-latency UX and strong device
integration.

## Product Goals

- First-class native experience on macOS, iPadOS, and iOS.
- Voice-first interaction that feels natural and interruption-safe.
- Full session fidelity with existing Codex semantics (ordering, replay,
  approvals, interrupts, tool events).
- Multi-session, multi-repo workflows that remain deterministic.
- Beautiful UI and polished UX on every form factor.

## Non-Negotiables

- Preserve strict event ordering metadata end-to-end.
- Keep one canonical session consumer model (no event stealing).
- No transcript loss on reconnect/reattach.
- Treat warnings as failures for shipped code.
- Keep release flow GitHub-based (no npm distribution for this line of work).

## Architecture Direction

### Core Runtime

- Keep Rust session/runtime core as the source of truth.
- Continue to use the session hub patterns built in `code-rs/app-server`.
- Ensure native clients consume the same attach/replay semantics.

### Native Client Stack

- UI: SwiftUI (latest stable), platform-native navigation/windowing.
- State: Observation (`@Observable`) + deterministic state reducers where needed.
- Concurrency: Swift Concurrency (`async`/`await`, actors) for streaming safety.
- Audio: `AVAudioEngine` + `AVAudioSession` lifecycle management.
- Speech-to-text: Apple on-device speech path by default when available.
- Text-to-speech: system TTS pipeline with interruption/barge-in support.

### UX Parity Direction (Codex App Spirit)

- Adopt a native shell with a persistent left rail, centered thread workspace,
  and docked composer.
- Mirror settings information architecture with category-first navigation and
  row-based preference cards.
- Keep visual language calm and dense: dark surfaces, subtle separators,
  restrained accents, and strong readability.
- Match terminology where practical (`Worktrees`, `MCP servers`, `Archived
  threads`, `Steer`) to reduce migration friction.

### Transport

- Keep protocol-compatible session transport between native app and Rust core.
- Start with existing local transport patterns (WebSocket/JSON stream), then
  optimize to direct native bridge if needed.

### Data and Persistence

- Persist local UI state (window layout, recent sessions, voice preferences).
- Add durable conversation replay/checkpoint storage in later milestones.

## Scope Split

### In Scope (Now)

- macOS app as primary first-class client.
- Voice loop MVP: hold-to-talk + streaming STT + spoken assistant replies.
- Core session operations: list/create/attach/detach/submit/interrupt.

### Deferred (Explicitly)

- Continuing WebUI feature expansion beyond current shelved checkpoint.
- Full offline local LLM replacement for cloud Codex reasoning.
- Cross-platform desktop targets outside Apple ecosystems.

## Milestones

### M0: Native Bootstrap

- [x] Create Apple app workspace and project structure.
- [x] Define shared protocol/client module for session transport.
- [x] Render session list + transcript read-only from live session stream.
- [ ] Verify ordering/replay contract with deterministic tests.

### M1: Voice Conversation MVP (macOS)

- [x] Push-to-talk and tap-to-stop capture controls.
- [x] Streaming transcription UX with partial/final text distinction.
- [x] Spoken assistant output with user barge-in interruption.
- [x] Session-safe turn control (submit/interrupt) from voice or keyboard.
- [x] Accessibility pass for voice controls and focus behavior.

### M2: Full Session Controls (macOS)

- [x] Approvals flows (exec/patch) with parity to existing semantics.
- [x] Tool/event cards with readable hierarchy and rich formatting.
- [x] Multi-session and multi-repo management UX.
- [x] Reconnect and replay resilience under network/process restarts.

### M2.5: macOS UX Parity Pass

- [x] Implement Codex-style shell layout (rail/workspace/composer hierarchy).
- [x] Implement Codex-style settings IA and row-card controls.
- [x] Add polish pass for spacing, typography, iconography, and motion.
- [x] Validate keyboard/accessibility parity for all top-level interactions.

### M3: iPadOS First-Class App

- [ ] Adaptive split layout for transcript + controls.
- [ ] Pencil/keyboard-friendly composer + command shortcuts.
- [ ] Voice loop parity with macOS where platform permits.

### M4: iOS Companion

- [ ] Compact session monitoring and quick-reply actions.
- [ ] Voice ask/reply loop optimized for handheld use.
- [ ] Interrupt/approve actions with minimal-friction UI.

### M5: On-Device ML Enhancements

- [ ] Add on-device intent router for voice command shortcuts.
- [ ] Add on-device summarization for long transcript compression.
- [ ] Evaluate local model hooks for low-risk assistive tasks
      (not replacing core coding reasoning path).

### M6: Hardening and Release

- [ ] Crash/telemetry instrumentation and failure analytics.
- [ ] Security review (keychain, permissions, local data protection).
- [ ] GitHub release automation for native app artifacts.

## Commit Checkpoints

- [x] CP-N1: Native shell + streaming transcript attached to live session.
- [x] CP-N2: Voice input pipeline stable with partial/final transcript UX.
- [x] CP-N3: Voice output + interruption semantics verified.
- [x] CP-N4: Session controls parity (attach/detach/submit/interrupt/approvals).
- [x] CP-N4.5: macOS visual + settings parity baseline complete.
- [ ] CP-N5: iPadOS parity baseline.
- [ ] CP-N6: iOS companion baseline.

## Reuse From Shelved Web Work

- Reuse: session hub single-consumer model and fanout/replay semantics.
- Reuse: attach/detach protocol and reconnect correctness logic.
- Reuse: ordering metadata handling and regression test patterns.
- Pause: browser-specific UI enhancements unless needed for debugging tools.

## Validation Strategy

- Rust completion gate remains `./build-fast.sh` from repo root.
- Add native CI gates when app targets are introduced.
- Maintain targeted regression tests for reconnect and ordering invariants.

## Risks

- Audio edge cases (device route changes, interruptions, background state).
- Drift between native UX polish and protocol-level correctness.
- Over-scoping on-device ML beyond what is reliable on current hardware.

## Acceptance Criteria

- Users can converse with Codex naturally via voice on macOS.
- Session correctness remains deterministic under reconnect and replay.
- Core controls and approvals are reliable across Apple clients.
- App feels production-grade in performance, visual quality, and usability.
