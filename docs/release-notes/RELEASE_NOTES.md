## @just-every/code v0.6.96

This release adds first-class goal workflows and tightens a few high-impact UX and reliability edges across the app server, TUI, and core runtime.

### Changes
- Goals: add persistent thread goals with `/goal` controls, status UI, pause and unpause actions, token budgets, and automatic continuation across app-server, core, and TUI flows.
- TUI: keep slash command popup columns stable while scrolling so command descriptions stop shifting horizontally.
- App Server: restore the persisted model provider on thread resume so resumed encrypted conversations stay on the correct endpoint.
- Updates: wait for npm registry readiness before prompting npm or Bun installs to upgrade.
- Core: bypass managed network proxying for explicitly escalated commands and fix Bedrock GPT-5.4 reasoning levels to avoid provider-side failures.

### Install
```bash
npm install -g @just-every/code@latest
code
```

Compare: https://github.com/just-every/code/compare/v0.6.95...v0.6.96
