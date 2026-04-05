## @just-every/code v0.6.92

This release improves cross-platform reliability, remote session behavior, and core model/tooling correctness.

### Changes

- TUI: enable Wayland clipboard image paste so screenshot paste works reliably on Linux Wayland sessions.
- Core: align model/provider behavior by syncing remote model parity and updating Copilot/GPT-5.4 mini agent flags.
- Remote: forward `--cd` consistently across remote start/resume/fork/list flows so session directory targeting works as expected.
- Core/TUI: fix MCP tool listing for hyphenated server names and prevent stale `/copy` output after commentary-only turns.
- Platform: improve macOS stability by preventing sandbox HTTP-client panics and filtering malloc diagnostics from composer input.

### Install

```bash
npm install -g @just-every/code@latest
code
```

Compare: https://github.com/just-every/code/compare/v0.6.91...v0.6.92
