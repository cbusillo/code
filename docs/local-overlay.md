# Upstream Import And Local Runtime Policy

Every Code owns its product direction, defaults, releases, and UX. `main` is the
canonical product branch and GitHub default branch for the local `code` binary on
this machine.

The pre-cutover `origin/main` history was archived at
`archive/pre-every-code-main-2026-05-24` before `main` was repointed to the Every
Code product branch. Treat `local/cbusillo-overlay` as a retired branch name;
do not revive it for new work.

Treat `codex-rs/` as a read-only mirror of `openai/codex`; put editable Rust
changes under `code-rs/`.

Remote map:

- `upstream`: `just-every/code`, the normal import source.
- `origin` / `fork`: `cbusillo/code`, where Every Code product branches and tags
  are pushed.
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
  infrastructure. New releases should use `every-code-v*`; legacy `overlay-v*`
  tags remain supported only so existing release history and notes keep working.
- **Model defaults:** local defaults may intentionally differ from upstream, but
  request wire compatibility should use upstream model metadata when available.

## Upstream Sync

Use the import helper from a clean `main` branch:

```sh
just local-upstream-import
```

After conflicts are resolved, preserve upstream fixes unless they contradict an
Every Code-owned behavior above. During conflict resolution, prefer upstream
unless the conflict touches one of those owned areas. Keep
`scripts/local/overlay-picks.txt` empty unless a patch intentionally lives
outside the product branch and must be replayed.

## Release Cadence

Cut an Every Code release after every successful upstream import or local hotfix
that should be installed elsewhere.

Tag format:

```text
every-code-v<upstream-version>.<product-patch>
```

Examples:

- `every-code-v0.6.98.1` for the first Every Code release on upstream `0.6.98`.
- `every-code-v0.6.98.2` for a second hotfix on the same upstream base.
- `every-code-v0.6.99.1` after the next upstream version bump.

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
git tag every-code-v0.6.98.1
git push origin HEAD
git push origin every-code-v0.6.98.1
```

Do not push Every Code releases to `upstream` or `openai`.

## Fork Health

Run this before and after upstream syncs:

```sh
just local-product-health
```

Pay particular attention to changes under `code-rs/tui/src/chatwidget.rs`,
`code-rs/core/src/patch_harness.rs`, model/version files, and release scripts.
Those are the highest-conflict long-term carry points.
