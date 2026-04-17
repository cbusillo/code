## @just-every/code v0.6.93

This release improves task visibility, hardens auth persistence, and tightens shell/app-server reliability.

### Changes

- TUI/Core: add task lifecycle visibility routing so task progress is surfaced consistently.
- Auth: atomically persist auth files to prevent partial writes and corrupted credentials.
- Core: route FedRAMP auth and model metadata for correct environment-specific model behavior.
- Shell/Exec: normalize raw shell script handling and preserve scripts plus crash traces in exec output.
- App Server: sync response item schema fixtures to keep protocol integrations stable.

### Install

```bash
npm install -g @just-every/code@latest
code
```

Compare: https://github.com/just-every/code/compare/v0.6.92...v0.6.93
