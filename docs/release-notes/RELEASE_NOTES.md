## @just-every/code v0.6.109

This release improves skill handling, agent reliability, diff rendering, and update behavior across Every Code.

### Changes

- Add manual skill discovery plus skill command policies, with bundled skill paths resolved relative to their skill directory.
- Make agent runs more resilient by retrying transient provider failures and cleaning up stale active slots after cancellation.
- Warn when multiple active sessions share a checkout, reduce repeated auth refresh fallback attempts, and harden app-server websocket transport.
- Improve `/diff` handling for submodules, whitespace paths, dirty markers, nested filters, and repo helper edge cases.
- Preserve the renamed binary identity during update checks so users see the correct command name.

### Install

```bash
gh release download v0.6.109 --repo cbusillo/code
```

Compare: https://github.com/cbusillo/code/compare/v0.6.108...v0.6.109
