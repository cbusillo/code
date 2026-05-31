## @just-every/code v0.6.110

This release improves skill workflow rendering, concurrent-session awareness, and release workflow behavior in Every Code.

### Changes

- Surface same-checkout concurrent sessions to the model so parallel local work has clearer context.
- Render structured skill workflow metadata and enforce command policies even when commands appear inside shell segments.
- Let release workflows exit cleanly when they determine no release should be published.

### Install

```bash
gh release download v0.6.110 --repo cbusillo/code
```

Compare: https://github.com/cbusillo/code/compare/v0.6.109...v0.6.110
