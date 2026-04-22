## @just-every/code v0.6.94

This release expands TUI workflows, broadens plugin support, and tightens core runtime behavior across providers and sandboxing.

### Changes

- TUI: add `/side` conversations and improve resume context with clearer parent thread status and titles.
- TUI: let you change reasoning level temporarily, show bash mode, and queue slash or shell input while commands run.
- Plugins: refresh `/plugins` with a tabbed marketplace, inline enablement toggles, and broader manifest/source support.
- Core: handle image generation outputs with higher-detail resizing defaults and add a built-in Amazon Bedrock provider.
- Sandbox: improve Windows exec support and make permission globs and profile intersections behave more reliably.

### Install

```bash
npm install -g @just-every/code@latest
code
```

Compare: https://github.com/just-every/code/compare/v0.6.93...v0.6.94
