## @just-every/code v0.6.95

This release improves model support, approval and permission flows, and execution stability across TUI and remote plugin workflows.

### Changes

- Models: add GPT-5.5 support and refresh model schema plus GPT agent defaults for more reliable model selection.
- ChatGPT: align ChatGPT plan and service-tier behavior, and route backend auth through Codex auth plumbing for more consistent account handling.
- Permissions: ship stable Auto Review controls, stricter approval flows, and persistent permission-profile state across TUI, protocol, and app-server sessions.
- TUI/Exec: fix interrupt and `/review` exit wedges, keep command output visible until streams fully close, and reduce turn interruption hangs.
- Plugins: expand remote plugin support with list/read/install flows, marketplace upgrade plumbing, and workspace-level plugin disable controls.

### Install

```bash
npm install -g @just-every/code@latest
code
```

### Thanks

Thanks to @SteveCoffey for contributions!

Compare: https://github.com/just-every/code/compare/v0.6.94...v0.6.95
