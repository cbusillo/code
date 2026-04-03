## @just-every/code v0.6.91

This release improves auth flexibility, TUI/session reliability, and build performance.

### Changes

- Core: support command-backed provider auth and dynamic bearer token sources for more flexible login flows.
- TUI: fix zellij redraw/composer rendering plus resume picker and review-follow-up stale state issues for smoother sessions.
- App Server: fix `/status` accuracy by showing fork source correctly and preventing stale rate-limit data in active sessions.
- Windows: improve shell startup reliability with PowerShell fallback paths and longer startup timeouts on slower runners.
- Core: reduce `codex-core` compile times substantially by moving key execution paths to native async handlers.

### Install

```bash
npm install -g @just-every/code@latest
code
```

### Thanks

Thanks to @owenlin0 for contributions!

Compare: https://github.com/just-every/code/compare/v0.6.90...v0.6.91
