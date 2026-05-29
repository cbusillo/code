## @just-every/code v0.6.107

This release focuses on model freshness, session reliability, and getting the
release path into better shape ahead of the next model drop.

### Highlights

- Updated the built-in Claude Opus selector to Claude Opus 4.8, including alias
  upgrades for older Opus selector names.
- Added regression coverage for dynamic remote GPT model discovery, so backend
  model slugs such as a future GPT-5.6 can appear without requiring another
  local selector release.
- Fixed ChatGPT auth fallback behavior, shutdown completions, and token refresh
  timing to reduce stale-auth interruptions.
- Tightened session-scoped agent state so archived agent activity, visibility,
  and status stay isolated across reconnects and same-repo sessions.
- Split release metadata preparation from final publishing. Release PRs no
  longer wait on the full preflight and platform matrix, while final publishing
  still runs the full release gate and platform asset builds.
- Cleaned up Every Code/CODEX compatibility docs, package shim behavior,
  release preflight paths, and agent/skill configuration docs.
