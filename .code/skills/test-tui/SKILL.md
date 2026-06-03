---
name: test-tui
description: >-
  Repo-local guide for testing the Every Code TUI interactively, including
  live-terminal visibility limits, VT100 snapshot evidence, restart dogfooding,
  and how to avoid overclaiming visual verification.
commands:
  - name: start-tui
    source: repo
    example_argv:
      - cargo
      - run
      - --manifest-path
      - code-rs/Cargo.toml
      - -p
      - code-tui
      - --
      - -c
      - log_dir=<some_temp_dir>
    purpose: >-
      Start the Every Code TUI interactively from the code-rs workspace with logs
      directed to a temp directory.
  - name: vt100-snapshots
    source: repo
    example_argv:
      - cargo
      - test
      - --manifest-path
      - code-rs/Cargo.toml
      - -p
      - code-tui
      - --test
      - vt100_chatwidget_snapshot
      - --features
      - test-helpers
      - --
      - --nocapture
    purpose: Render deterministic terminal frames through the VT100 snapshot harness.
  - name: iterm2-tui-list
    source: skill
    resource_path: scripts/iterm2_tui_visibility.py
    example_argv:
      - uv
      - run
      - scripts/iterm2_tui_visibility.py
      - list
    purpose: List iTerm2 sessions with metadata only before selecting a TUI session.
  - name: iterm2-tui-text
    source: skill
    resource_path: scripts/iterm2_tui_visibility.py
    example_argv:
      - uv
      - run
      - scripts/iterm2_tui_visibility.py
      - text
      - <session_id>
    purpose: Capture visible text from one selected iTerm2 session.
  - name: iterm2-tui-windows
    source: skill
    resource_path: scripts/iterm2_tui_visibility.py
    example_argv:
      - uv
      - run
      - scripts/iterm2_tui_visibility.py
      - windows
    purpose: List visible iTerm2 macOS window ids for targeted screenshots.
  - name: iterm2-tui-screenshot
    source: skill
    resource_path: scripts/iterm2_tui_visibility.py
    example_argv:
      - uv
      - run
      - scripts/iterm2_tui_visibility.py
      - screenshot
      - <session_id>
      - --window-id
      - <window_id>
    purpose: Capture pixels for one selected iTerm2/macOS window.
resources:
  - path: scripts/iterm2_tui_visibility.py
    kind: script
    description: Read-only macOS/iTerm2 SDK helper for TUI session text and window capture.
workflow_defaults:
  - name: rust_log
    value: RUST_LOG=trace
    description: >-
      Always set trace logging when starting the TUI for interactive testing.
  - name: input_delivery
    value: text_then_enter
    description: >-
      Send text first, then Enter as a separate write when driving the TUI
      programmatically.
---

# Test TUI

This is a repo-local Every Code skill, not a generic TUI testing guide. Use it
for the Every Code TUI, the `code` command, and this repository's Rust
workspace. The `just codex` recipe name is a compatibility-era repo target, so
prefer direct `code-rs` commands when validating Every Code TUI changes.

You can start and use the Every Code TUI to verify changes.

## Important Notes

- Start interactively.
- Always set `RUST_LOG="trace"` when starting the process.
- Pass `-c log_dir=<some_temp_dir>` so logs are written to a known location.
- When sending a test message programmatically, send text first, then send Enter
  in a separate write. Do not send text plus Enter in one burst.
- Use the Every Code TUI binary from the repo root:
  `cargo run --manifest-path code-rs/Cargo.toml -p code-tui -- -c log_dir=<some_temp_dir>`.

## Visibility Contract

Do not claim that you visually verified the live TUI unless you actually used a
live terminal surface that exposed the rendered UI. A rebuilt `code` binary,
passing tests, log inspection, Code Bridge events, and embedded string checks are
useful evidence, but they are not live visual verification by themselves.

Code Bridge may capture browser/app surfaces, console logs, pageviews, or bridge
screenshots when a client is connected. It does not prove that the current
terminal TUI pixels were visible unless a screenshot or control response clearly
comes from the TUI surface being tested.

Use precise language in closeout:

- `live TUI visually verified` only after direct TUI capture/interaction.
- `iTerm2 session verified` after reading the relevant iTerm2 session's live
  terminal contents through an explicit iTerm2 capture helper.
- `macOS window screenshot verified` after capturing the targeted window or
  rectangle pixels and inspecting the image.
- `VT100-render verified` after the snapshot harness renders the relevant frame.
- `binary/restart verified` after `code --version`, PATH target, and process
  restart checks pass.
- `not visually verified` when you only have tests/logs/binary evidence.

## Evidence Ladder

Prefer the strongest practical evidence for the change:

1. Live interactive TUI: start with `RUST_LOG=trace cargo run --manifest-path
code-rs/Cargo.toml -p code-tui -- -c log_dir=<dir>`, drive the workflow,
   and inspect the rendered terminal state directly.
2. iTerm2 session capture on macOS: use a read-only helper to list iTerm2
   windows/tabs/sessions, choose the Every Code session by title, tty, process,
   cwd, or visible text, and capture that session's live terminal contents.
3. Targeted macOS screenshot: use a window-id or rectangle capture when pixels,
   colors, cropping, or non-text rendering matter.
4. VT100 harness: use `render_chat_widget_to_vt100` or the
   `vt100_chatwidget_snapshot` test when the scenario can be seeded
   deterministically without a live process.
5. Focused unit/integration tests: acceptable for state transitions, event
   routing, identity matching, and nonvisual behavior.
6. Binary/restart checks: verify PATH, symlink target, `code --version`, and
   relevant strings or logs, but report them as binary evidence only.

If a change is specifically about footer text, ordering, clipping, scroll state,
focus, cursor position, or terminal chrome, prefer live TUI or VT100-rendered
evidence over ordinary unit assertions.

## macOS And iTerm2 Capture

On macOS with iTerm2, prefer iTerm2 session text capture for TUI text/cell
evidence and macOS window screenshots for pixel evidence. Use the PEP 723 repo
helper through `uv run scripts/iterm2_tui_visibility.py` from this skill's
directory, or use the repo-root path shown below. Dependencies resolve without a
manually created virtual environment.

The helper is read-only by default and supports these operations:

- List iTerm2 windows, tabs, and sessions with stable identifiers and enough
  metadata to choose the intended Every Code session:
  `uv run .code/skills/test-tui/scripts/iterm2_tui_visibility.py list`.
- Capture one selected session's visible terminal contents.
  `uv run .code/skills/test-tui/scripts/iterm2_tui_visibility.py text
<session_id>`.
- List visible iTerm2 macOS windows for screenshot window ids:
  `uv run .code/skills/test-tui/scripts/iterm2_tui_visibility.py windows`.
- Capture one targeted macOS window:
  `uv run .code/skills/test-tui/scripts/iterm2_tui_visibility.py screenshot
<session_id> --window-id <window_id>`.
- Label every capture with the source window/tab/session so closeout can say
  exactly what was inspected.

Use targeted screenshots only after resolving a specific window id. The helper
uses macOS `screencapture -l <window_id>` for one window. It intentionally avoids
whole-screen screenshots and x/y rectangle capture by default.

If the helper returns `llm_advice`, follow it: tell the user which macOS
permission or optional dependency is missing and suggest the install command it
reports. The helper uses the iTerm2 Python SDK for session capture and Quartz
Python bindings (`pyobjc-framework-Quartz`) for window-id listing; both are
declared in the script's inline dependency metadata for `uv run`.

Do not silently inspect unrelated terminal sessions. Start with metadata/listing,
then capture the selected session or the set of sessions the user explicitly
asked to inspect.

## Restart Dogfooding

After rebuilding the PATH-resolved binary with `just local-code-rebuild`, verify:

- `which code`
- `readlink "$(which code)"` when the PATH entry is a symlink
- `code --version`
- `git status --short --branch`

This proves the restarted harness can pick up the rebuilt binary. It still does
not prove the live TUI rendered a particular visual state unless paired with live
TUI or VT100 evidence.
