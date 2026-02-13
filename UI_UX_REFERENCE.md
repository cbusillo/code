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
