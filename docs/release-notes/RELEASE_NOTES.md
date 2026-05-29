## @just-every/code v0.6.107

This release focuses on model freshness, session reliability, and a smoother release path.

### Changes

- Updated the built-in Claude Opus selector to Claude Opus 4.8 and added coverage for dynamic remote GPT model discovery.
- Fixed ChatGPT token refresh, auth fallback, and shutdown completions to reduce stale-auth interruptions.
- Kept archived agent activity, visibility, and status isolated across reconnects and same-repo sessions.
- Split release metadata preparation from final publishing so release PRs stay lightweight while publish runs the full gate.
- Cleaned up Every Code/CODEX compatibility docs, package shim behavior, release preflight paths, and agent/skill configuration docs.

### Install

```bash
gh release download v0.6.107 --repo cbusillo/code
```
