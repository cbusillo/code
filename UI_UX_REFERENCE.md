# Swift UI/UX Reference (Code Native)

Last validated: February 12, 2026

This file is the design and UX reference for native Apple client work in this
repo. Use it before and during implementation.

## Canonical Sources

- Apple Human Interface Guidelines (HIG)
  - <https://developer.apple.com/design/human-interface-guidelines/>
- Apple UI Design Tips
  - <https://developer.apple.com/design/tips/>
- Apple Design Resources (Figma/Sketch/UI kits)
  - <https://developer.apple.com/design/resources/>
- SwiftUI: What is New
  - <https://developer.apple.com/swiftui/whats-new/>
- WWDC23: Design with SwiftUI
  - <https://developer.apple.com/videos/play/wwdc2023/10115/>
- WWDC23: Build accessible apps with SwiftUI and UIKit
  - <https://developer.apple.com/videos/play/wwdc2023/10036/>
- WWDC25: Optimize SwiftUI performance with Instruments
  - <https://developer.apple.com/videos/play/wwdc2025/306/>

Notes:

- Treat HIG as source of truth when guidance conflicts.
- Prefer current SwiftUI APIs over custom workarounds when possible.

## Quality Bar

- Native first: app should feel like a best-in-class Apple app, not a web port.
- Clarity over cleverness: users should understand hierarchy at a glance.
- Visual rhythm: spacing, sizing, and alignment must be consistent.
- Strong information architecture: primary action should always be obvious.
- Accessibility is required quality, not a follow-up task.
- Smoothness is UX: avoid stutters, hitches, and redraw jank.

## Practical Design Rules

- Use one clear primary action per area.
- Keep secondary actions visually quieter than primary actions.
- Avoid giant controls unless they are truly primary and contextual.
- Use readable line lengths for transcript/content surfaces.
- Prefer meaningful labels over technical IDs in primary UI text.
- Move technical identifiers to subtitles, metadata, or debug views.
- Use platform-standard components before custom controls.
- Keep color system restrained; do not rely on color alone for meaning.

## Typography And Spacing

- Use semantic text styles (`.title`, `.headline`, `.body`, `.caption`).
- Use monospaced font only for code-like or diagnostic content.
- Keep body copy readable at normal zoom and default accessibility size.
- Keep tap/click targets comfortably large and consistent.
- Use an explicit spacing scale and reuse it (for example: 4/8/12/16/24).

## Interaction And Motion

- Show immediate feedback for actions (press, submit, reconnect, errors).
- Keep transitions subtle and meaningful.
- Avoid layout jumps while streaming content.
- Preserve user context during updates (selection, scroll, focus).
- Ensure keyboard interactions are complete for all primary workflows.

## Accessibility Checklist

- All controls have accessible labels and sensible traits.
- Keyboard-only navigation works for every primary flow.
- Focus order is predictable and stable.
- Contrast is sufficient in all states.
- Dynamic Type does not break core layouts.
- Voice features provide clear state feedback and interruption behavior.

## Performance Checklist

- No heavy work on the main thread during streaming updates.
- Large transcript views do not re-render unnecessarily.
- State updates are scoped to affected UI regions.
- Startup, reconnect, and session switch feel responsive.

## Pre-Merge UX Gate (Required)

- Compare against HIG principles for touched surfaces.
- Validate in real app flow, not just previews.
- Validate keyboard and accessibility behavior.
- Validate visual hierarchy at default and smaller window sizes.
- Validate empty, loading, error, and dense-data states.
- Validate at least one long transcript + one multi-session workflow.

## Repo-Specific Guidance (Code Native)

- Mirror correctness comes first: never sacrifice session integrity for visuals.
- Names in primary UI should be user-comprehensible.
- Avoid exposing UUID-heavy text as primary labels.
- Keep composer controls compact, legible, and clearly prioritized.
- Keep settings grouped by user goals, not implementation details.

## Suggested Workflow For UI Changes

- Start from a small visual target (one surface, one interaction).
- Implement with semantic SwiftUI primitives first.
- Test live with `pnpm dev:native`.
- Fix layout and naming before adding more features.
- Run `./build-fast.sh` before finalizing.

---

## Component-Level UX Standards

### Information Hierarchy

Every surface must have a clear primary/secondary/tertiary read at a glance:

- **Primary**: user messages, assistant responses, pending approval prompts.
- **Secondary**: tool calls, reasoning blocks, activity events.
- **Tertiary**: timestamps, token counts, session metadata.

Enforce this with opacity, size, and weight—not color alone. Technical strings
(UUIDs, raw paths) belong in secondary or detail layers, never as primary text.

### Transcript Ergonomics

- Cards should be scannable without reading every word: use subtle card
  backgrounds (`TranscriptCardStyle`) plus role-specific border tints.
- Streaming inserts must not produce layout jumps. Grow the scroll region
  downward; never reflow content above the viewport.
- Density settings (`TranscriptDensity`) must be respected uniformly across
  all card types, including exec and approval cards.
- Code blocks get monospaced font and syntax tint; prose gets `.body` with
  comfortable line-height (`lineSpacing` from density setting).
- Keep readable line lengths: 60–80 characters is the target for prose regions.
  Constrain max width on large screens rather than stretching content edge-to-edge.

### Activity And Task Visualization

- Ongoing work must have a visible, non-intrusive activity indicator tied to
  the active task (spinner, progress pulse, or animated ellipsis).
- Activity events (`taskActivityByStartItemID`) should be visually subordinate
  to the result card they belong to—same card, quieter style.
- Exec command cards: show command + status (running / exit code) prominently;
  stdout/stderr in a collapsible secondary region.
- Long-running tasks: show elapsed time in the card footer, updated live.
- Completed tasks: drop the spinner immediately; show exit code and duration.

### Approval Flows

- An approval card must be the most visually prominent item in the viewport
  when it appears. Do not let it compete with assistant prose above it.
- Use the `.approval` card style (amber tint) consistently. Never use the same
  visual treatment for non-approval content.
- The three choices (Yes / Yes for session / No) must be large enough to tap
  on iOS and click precisely on macOS. Labels must be unambiguous English.
- After a decision, collapse the approval card to a compact summary line.
  Do not remove it entirely—users need to audit what was approved.
- Keyboard: the primary approval action must be reachable without a mouse.

### Motion And Feedback

- Transitions should be `easeOut` with duration 0.18–0.25 s for card inserts.
  Do not animate layout shifts caused by streaming text growth.
- New session / thread switch: brief cross-fade of the transcript area only;
  sidebar and top bar stay static.
- Connection state changes (connecting → connected, reconnect) must be
  communicated via the status line, not full-screen overlays.
- Button presses and sends must produce immediate visual feedback (opacity
  flash or brief scale). Never leave user action feeling unacknowledged.

### Spacing And Typography Scale

Use only values from the explicit scale: **4 / 8 / 12 / 16 / 24 / 32**.

| Context | Value |
|---|---|
| Icon-to-label gap | 8 |
| Card internal padding (compact) | 10 |
| Card internal padding (comfortable) | 14 |
| Section separation | 16–24 |
| Rail / sidebar item height | 32 |

Typography: semantic SwiftUI text styles everywhere. Monospaced font only for
code, commands, and diagnostic identifiers. Never hard-code point sizes for
body copy.

---

## Screenshot Benchmarking Workflow

The `CodeNativeAutomation` tool captures reproducible screenshots using
accessibility IDs. Run this workflow whenever changing visible UI surfaces.

### Setup (one-time)

```sh
# ensure accessibility is granted to Terminal in System Settings → Privacy
swift run --package-path native/CodeNative CodeNativeAutomation --help
```

### Running A Benchmark

Start the backend and app, then invoke a scenario:

```sh
pnpm dev:native &           # or run CodeNativeApp manually
swift run --package-path native/CodeNative CodeNativeAutomation \
  --scenario native/CodeNative/automation/scenarios/current-window-shot.json
```

Screenshots land under `docs/reference/native-ui/` (path is set inside each
scenario JSON).

### Scenario JSON Reference

```json
[
  { "kind": "wait",       "seconds": 0.4 },
  { "kind": "click",      "id": "rail.new-thread" },
  { "kind": "click",      "title": "Button label" },
  { "kind": "type",       "id": "composer.input", "text": "hello" },
  { "kind": "screenshot", "path": "docs/reference/native-ui/SCENARIO.png" },
  { "kind": "dump_tree",  "maxDepth": 3 }
]
```

`id` matches `accessibilityIdentifier` set in SwiftUI (`.accessibilityIdentifier("rail.new-thread")`).
`title` matches `accessibilityLabel` or button title text.

### Benchmark Scenarios To Maintain

| Scenario file | What it covers |
|---|---|
| `current-window-shot.json` | Default state at startup |
| `new-thread-shot.json` | Empty composer after new thread |
| _(add)_ `approval-pending.json` | Approval card in focus |
| _(add)_ `exec-running.json` | Exec card with live activity |
| _(add)_ `long-transcript.json` | Dense transcript, scroll position stable |
| _(add)_ `settings-open.json` | Settings panel, General tab |

Add new scenarios as new surfaces are built. Keep scenario files in
`native/CodeNative/automation/scenarios/`.

### Visual Diff Review

After capturing screenshots, compare against the previous baseline:

```sh
# quick side-by-side diff (requires ImageMagick)
magick compare -metric PSNR \
  docs/reference/native-ui/BEFORE.png \
  docs/reference/native-ui/AFTER.png \
  docs/reference/native-ui/diff.png
```

Or open both in Preview with Split View for a manual review. The goal is to
catch unintended regressions in spacing, alignment, and card styling—not to
pixel-lock the UI.

### Accessibility ID Conventions

Use dot-namespaced IDs so tree dumps are readable:

| Prefix | Used for |
|---|---|
| `rail.` | Action rail buttons |
| `composer.` | Input area controls |
| `thread.` | Thread list items |
| `card.` | Transcript card regions |
| `approval.` | Approval choice controls |
| `settings.` | Settings panel sections |
