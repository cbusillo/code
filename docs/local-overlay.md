# Upstream Import And Local Runtime Policy

Every Code owns its product direction, defaults, releases, and UX. `main` is the
canonical product branch and GitHub default branch for the local `code` command on
this machine.

## Naming Ledger

- **Every Code** is the product name. Use it in prose, docs, UI copy, issue text,
  release text, and first mentions.
- **Every Code CLI** is the product CLI surface. **The `code` command** is the
  executable users type. Avoid “code CLI” in prose.
- **The Every Code agent** is the assistant identity. **Code** is only a short
  display name after Every Code context is established.
- **Every Code harness** is the runtime/session wrapper that we restart and
  dogfood. **Every Code runtime** is appropriate for process, session, and tool
  execution internals.
- **Upstream import sources** are `just-every/code` and `openai/codex`.
  `just-every/code` is a fork upstream/import source. `openai/codex` / Codex CLI
  is the original/direct upstream and provenance source.
- `codex-rs/` is a read-only local mirror of `openai/codex:main`. `code-rs/` is
  the editable Every Code Rust implementation.
- `CODE_HOME` / `~/.code` are primary config and state locations. `CODEX_HOME` /
  `~/.codex` are compatibility fallbacks. Keep `CODEX_*` names where external,
  backend, or upstream compatibility requires them; add `CODE_*` aliases or
  rename only through scoped migrations.

The pre-cutover `origin/main` history was archived at
`archive/pre-every-code-main-2026-05-24` before `main` was repointed to the Every
Code product branch. Treat `local/cbusillo-overlay` as a retired branch name;
do not revive it for new work.

Treat `codex-rs/` as a read-only mirror of `openai/codex`; put editable Rust
changes under `code-rs/`.

Remote map:

- `upstream`: `just-every/code`, a fork upstream/import source.
- `origin` / `fork`: `cbusillo/code`, where Every Code product branches and tags
  are pushed.
- `openai`: remote for `openai/codex`, the original/direct upstream and
  provenance source. Use it for direct upstream intake when intentional; do not
  push Every Code changes there.

## Every Code-Owned Surfaces

- **Remote Inbox:** remote session control, request-user-input forwarding, and
  remote Auto Drive triggers. Keep transport/protocol code isolated from TUI
  event handling where possible.
- **Auto Drive integration:** coordinator UI, Esc semantics, status surfaces,
  and local crash diagnostics. Keep routing in `chatwidget.rs` and `app.rs`
  consistent with `AGENTS.md`.
- **Patch harness:** local validation for changed files, project tool discovery,
  and workspace-aware validator execution.
- **Release workflow:** GitHub Releases and local PATH rebuilds are Every Code
  infrastructure. GitHub Releases are the canonical internal update source;
  npm and Homebrew publishing are deferred unless package-manager distribution
  becomes intentional again.
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
that should be installed by dogfood users. The active Release workflow runs from
`main`, opens a release metadata PR when the package version or notes need to be
updated, and publishes GitHub Release assets after that metadata lands.

Release tags use the plain `v<version>` format, for example `v0.6.101`.

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
the fast build creates dev-fast artifacts for validation, while the rebuild
recipe owns the PATH-resolved release binary and embeds the package version.

If an old manual Homebrew link exists, remove it so PATH resolution stays
repo-owned and predictable:

```sh
just local-remove-homebrew-code-link
```

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

Then push the product branch to `origin`, open or merge the release metadata PR,
and monitor the `Release` workflow:

```sh
git push origin HEAD
scripts/wait-for-gh-run.sh --workflow Release --branch main
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
