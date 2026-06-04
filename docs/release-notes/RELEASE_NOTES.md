## Every Code v0.6.115

This release makes concurrent checkout edits safer when multiple Every Code sessions are active.

### Changes

- Every Code now requires an explicit visible worktree decision before editing a checkout that already has another write-capable session.
- The concurrent-write gate is stricter about invalid or missing decisions, keeping risky write operations blocked until the agent records how it will proceed.

### Install

```bash
gh release download v0.6.115 --repo cbusillo/code
```

Compare: https://github.com/cbusillo/code/compare/v0.6.114...v0.6.115
