---
name: test-tui
description: Guide for testing Every Code TUI interactively
commands:
  - name: start-tui
    source: repo
    example_argv: ["just", "codex", "-c", "log_dir=<some_temp_dir>"]
    purpose: Start the TUI interactively from the repo recipe with logs directed to a temp directory.
workflow_defaults:
  - name: rust_log
    value: RUST_LOG=trace
    description: Always set trace logging when starting the TUI for interactive testing.
  - name: input_delivery
    value: text_then_enter
    description: Send text first, then Enter as a separate write when driving the TUI programmatically.
---

You can start and use Every Code TUI to verify changes.

Important notes:

Start interactively.
Always set RUST_LOG="trace" when starting the process.
Pass `-c log_dir=<some_temp_dir>` argument to have logs written to a specific directory to help with debugging.
When sending a test message programmatically, send text first, then send Enter in a separate write (do not send text + Enter in one burst).
Use `just codex` target to run - `just codex -c ...`
