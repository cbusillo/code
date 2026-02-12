# Codex App UI Reference (2026-02-12)

This file captures the two user-provided screenshots shared on
2026-02-12 and distills concrete UI/UX cues for native parity work.

## Source Artifacts

- `docs/reference/raw/codex-app-main-2026-02-12.png` (main app view)
- `docs/reference/raw/codex-app-settings-2026-02-12.png` (settings view)

Note: these screenshots were shared directly in chat. This reference file is
the durable in-repo source of truth for the visual cues we are implementing.

## Main App Cues

- Three-column feel with strong left navigation rail and a centered thread area.
- Left rail includes top-level actions: `New thread`, `Automations`, `Skills`.
- Left rail includes thread groups by repo/workspace.
- Left rail keeps a sticky `Settings` entry at the bottom.
- Dense but calm top bar with model/session identity and quick actions
  (`Open`, `Commit`, diff counts).
- Conversation canvas is intentionally sparse while idle.
- Composer is docked at the bottom with model selector, reasoning/effort
  selector, context toggle (IDE context), mode/permissions controls, and
  voice + send affordances.
- Overall look uses dark translucent surfaces, low-chroma accents, rounded
  corners, and soft separators.

## Settings Cues

- Settings presented as dedicated full-screen surface with left category list.
- Category rail includes: `General`, `Configuration`, `Personalization`,
  `MCP servers`, `Git`, `Environments`, `Worktrees`, `Archived threads`.
- Main panel uses grouped cards with row-based controls.
- Typical control rows pair a label + short description with right-side
  segmented controls, toggles, or pickers.
- Key options visible: default open destination, thread detail density,
  prevent sleep while running, multiline send shortcut behavior,
  follow-up behavior (`Queue` vs `Steer`), theme (`Light`, `Dark`, `System`),
  and opacity/pointer/font options.

## Native App Direction Derived From These Screens

- Preserve the split between conversation workspace and global settings shell.
- Keep high information density without visual noise.
- Prefer compositional SwiftUI sections over deeply nested modal flows.
- Mirror terminology where it helps migration (`Worktrees`, `MCP servers`,
  `Archived threads`, `Steer`).
