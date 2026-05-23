# Upstream Import And Local Runtime Policy

Every Code owns its product direction, defaults, releases, and UX. Until the
default branch is renamed in #87, `local/cbusillo-overlay` remains the canonical
branch for the local `code` binary on this machine. It imports useful upstream
changes from `just-every/code` while keeping Every Code-specific behavior in the
product branch.

Treat `codex-rs/` as a read-only mirror of `openai/codex`; put editable Rust
changes under `code-rs/`.

Remote map:

- `upstream`: `just-every/code`, the normal import source.
- `origin` / `fork`: `cbusillo/code`, where Every Code branches and tags are
  pushed until the repository identity transition finishes.
- `openai`: reference remote for `openai/codex`; do not use it for routine
  imports or pushes.

## Every Code-Owned Surfaces

- **Remote Inbox:** remote session control, request-user-input forwarding, and
  remote Auto Drive triggers. Keep transport/protocol code isolated from TUI
  event handling where possible.
- **Auto Drive integration:** coordinator UI, Esc semantics, status surfaces,
  and local crash diagnostics. Keep routing in `chatwidget.rs` and `app.rs`
  consistent with `AGENTS.md`.
- **Patch harness:** local validation for changed files, project tool discovery,
  and workspace-aware validator execution.
- **Release workflow:** binary releases and local PATH rebuilds are Every Code
  infrastructure. The `overlay-v*` tag format remains temporary until #87.
- **Model defaults:** local defaults may intentionally differ from upstream, but
  request wire compatibility should use upstream model metadata when available.

## Upstream Sync

Use the import helper from a clean `local/*` branch:

```sh
just local-overlay-update
```

After conflicts are resolved, preserve upstream fixes unless they contradict an
Every Code-owned behavior above. During conflict resolution, prefer upstream
unless the conflict touches one of those owned areas. Keep
`scripts/local/overlay-picks.txt` empty unless a patch intentionally lives
outside the product branch and must be replayed.

## Release Cadence

Cut an Every Code release after every successful upstream import or local hotfix
that should be installed elsewhere.

Temporary tag format, pending #87:

```text
overlay-v<upstream-version>.<fork-patch>
```

Examples:

- `overlay-v0.6.95.1` for the first Every Code release on upstream `0.6.95`
- `overlay-v0.6.95.2` for a second Every Code hotfix on the same upstream base
- `overlay-v0.6.96.1` after the next upstream version bump

Required release gate:

```sh
git status --short --branch
./build-fast.sh
```

Install and smoke-check the local binary before tagging:

```sh
just local-code-rebuild
code --version
code exec -m gpt-5.5 --sandbox read-only --max-seconds 30 "Reply with exactly OK."
```

Run `just local-code-rebuild` after any release-readiness `./build-fast.sh` run:
the fast build can leave the PATH-resolved `code` pointing at a dev-fast binary
that reports `0.0.0`.

## Session Exit Cleanup

Before leaving a local work session, reclaim rebuildable artifacts:

```sh
just local-cleanup-space --apply
```

The cleanup is intentionally aggressive: it removes `codex-rs/target`, legacy
root targets, `code-rs` debug/dev-fast artifacts, all `./build-fast.sh` target
cache buckets, and release dependency cache. It preserves
`code-rs/target/release/code`, which is the PATH-resolved local binary built by
`just local-code-rebuild`.

Run without `--apply` to preview deletions. Use `--keep-current-fast-cache` or
`--keep-release-cache` only when you intentionally want a warmer next build.

Then push the product branch and tag to `origin`, and monitor `Binary Release`:

```sh
git tag overlay-v0.6.96.1
git push origin local/cbusillo-overlay
git push origin overlay-v0.6.96.1
```

Do not push Every Code releases to `upstream` or `openai`.

## Fork Health

Run this before and after upstream syncs:

```sh
scripts/local/fork-health.sh
```

Pay particular attention to changes under `code-rs/tui/src/chatwidget.rs`,
`code-rs/core/src/patch_harness.rs`, model/version files, and release scripts.
Those are the highest-conflict long-term carry points.
