# Upstream Import And Runtime Policy

Every Code owns its product direction, defaults, releases, and UX. `main` is the
canonical product branch and GitHub default branch for the local `code` command on
this machine.

This document replaces the old local overlay framing. Every Code now owns
`main`; upstream imports are synchronization work into the product branch, not a
long-running feature branch overlay.

## Naming Ledger

- **Every Code** is the product name. Use it in prose, docs, UI copy, issue text,
  release text, and first mentions.
- **Every Code Lab** is the `cbusillo/code` runtime/build tag, similar to a beta
  or lab channel label. Use it where users need to distinguish this build from
  upstream `just-every/code`; do not treat it as a product rebrand.
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

## CODE And CODEX Compatibility Policy

Use `CODE_*` names for new Every Code-owned environment variables, config paths,
automation metadata, and locally-authored integration contracts. Use
`EVERY_CODE_*` only when the variable needs to be unmistakably product-branded
outside this repo, such as cross-repo remote inbox metadata.

Keep `CODE_HOME` / `~/.code` as the primary config and state location. Keep
`CODEX_HOME` / `~/.codex` as a compatibility fallback. The lookup order is
`CODE_HOME`, then `CODEX_HOME`, then default `~/.code`, with legacy `~/.codex`
read only where the specific subsystem supports migration or compatibility.
Every Code-owned writes should go to `~/.code` / `CODE_HOME` unless a scoped
compatibility feature explicitly says otherwise.

Preserve `CODEX_*` names when they are part of an external contract, upstream
merge surface, backend/API behavior, or documented compatibility behavior. This
includes names such as `CODEX_HOME`, `CODEX_API_KEY`, cloud/backend variables,
Bazel/upstream CI variables, and protocol/security names until a dedicated
migration adds `CODE_*` aliases with tests.

Dev-only or test-only `CODEX_TUI_*` variables are rename candidates, not silent
cleanup. A change should add a `CODE_TUI_*` alias first, keep the old
`CODEX_TUI_*` spelling for a compatibility window, document the precedence, and
include focused tests for both names before any removal.

Do not mass-rename deep Rust identifiers, telemetry prefixes, generated schema
types, model names, or `codex-rs` mirror paths just to remove `codex`. Those
names often preserve upstream compatibility, historical provenance, or external
dashboard continuity.

The pre-cutover `origin/main` history was archived at
`archive/pre-every-code-main-2026-05-24` before `main` was repointed to the Every
Code product branch. Treat `local/cbusillo-overlay` as a retired branch name;
do not revive it for new work.

Treat `codex-rs/` as a read-only local mirror of `openai/codex:main`; put
editable Rust changes under `code-rs/`.

Remote map:

- `upstream`: `just-every/code`, a fork upstream/import source.
- `origin` / `fork`: `cbusillo/code`, where Every Code product branches and tags
  are pushed.
- `openai`: remote for `openai/codex`, the original/direct upstream and
  provenance source. Use it for direct upstream intake when intentional; do not
  push Every Code changes there.

Do not describe either import source as the permanent primary upstream. The
current import path may vary by task; the durable distinction is that
`just-every/code` is a fork upstream/import source and `openai/codex` is the
original/direct upstream and provenance source.

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

After conflicts are resolved, preserve imported fixes unless they contradict an
Every Code-owned behavior above. During conflict resolution, prefer the import
source unless the conflict touches one of those owned areas. Keep
`scripts/local/upstream-picks.txt` empty unless a patch intentionally lives
outside the product branch and must be replayed.

### Upstream Review Cursors

Every Code is a product branch with two upstream references. Track
where review left off in `.github/upstream-cursors.json`, and inspect that state
with:

```sh
just local-upstream-cursors
```

The cursor file records `lastExamined` and `lastImported` separately for each
upstream source:

- `lastExamined` is the newest upstream commit whose delta has been reviewed or
  intentionally skipped. This is the cursor future sessions should compare from
  to avoid rereading old upstream commits.
- `lastImported` is the newest upstream commit actually merged, cherry-picked, or
  manually ported into Every Code. It can be older than, newer than, or absent
  relative to `lastExamined`, especially for direct `openai/codex` provenance
  reviews.

Do not infer either value from `git merge-base`. Merge bases are useful branch
health diagnostics, but they do not say what a human or agent already examined.

When reviewing upstream without importing, advance only `lastExamined` for that
source after the review is complete. When importing or manually porting upstream
work, update `lastImported` as well and include the product commit that contains
the port. Keep cursor changes in the same PR as the review/import so the next
session can start from the recorded SHA.

Fetch branch refs without tags when checking cursors. Upstream and Every Code
share `v*` tag names, and tag fetches can collide even when branch refs are
healthy.

## Release Cadence

Every Code releases are intentional dogfood distribution events, not an
automatic follow-up to every upstream import, local fix, or merged PR. Use the
release policy in `.github/github.json` to decide whether to cut immediately or
defer until nearby release-worthy fixes settle into one batch.

The active release workflow file is `.github/workflows/release.yml`, and GitHub
displays it as `Release Intent`. That name is intentional: the workflow runs
after relevant `main` pushes, first decides whether the committed package
version represents a new release, and only then publishes GitHub Release assets.

Prepare release metadata locally with the Every Code harness only when cutting a
release intentionally: the local command bumps `codex-cli/package.json`, updates
`CHANGELOG.md`, and writes `docs/release-notes/RELEASE_NOTES.md`. For this repo,
the committed `codex-cli/package.json` version is release intent. If that
version already has a tag, the workflow exits successfully as a no-op; if the
tag does not exist, it validates the metadata and publishes exactly that
committed version instead of generating fallback notes in CI. The full preflight,
macOS/Linux release matrix, and Windows asset build run only on the publish pass
after the metadata PR has merged.

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

Generate and review release metadata before pushing the release PR:

```sh
just local-release-notes
scripts/check-release-notes-version.sh
```

Commit the resulting `codex-cli/package.json`, `CHANGELOG.md`, and
`docs/release-notes/RELEASE_NOTES.md` changes on a release metadata branch.

If an old manual Homebrew link exists, remove it so PATH resolution stays
repo-owned and predictable:

```sh
just local-remove-homebrew-code-link
```

Then push the release metadata branch to `origin`, merge the release metadata
PR, and monitor the `Release Intent` workflow:

```sh
git push origin HEAD
scripts/wait-for-gh-run.sh --workflow 'Release Intent' --branch main
```

A successful workflow run is not enough evidence that a release was published:
non-release runs also complete successfully as no-ops when the package version
already has a tag. When cutting a release, verify the tag or GitHub Release
directly after the workflow succeeds:

```sh
gh release view v<version> --repo cbusillo/code
```

Do not push Every Code releases to `upstream` or `openai`.

## Local Cache Cleanup

During active local work, keep rebuildable caches bounded while preserving the
currently wired fast-build cache bucket:

```sh
just local-cleanup-space --apply --keep-current-fast-cache
```

This is the normal cleanup path when `.code/working/_target-cache` grows large.
It removes stale per-branch `./build-fast.sh` buckets and local debug artifacts,
but keeps the active `code-rs/target/dev-fast/code` cache warm for the next
build.

When intentionally ending a local work session and accepting a colder next
`./build-fast.sh`, reclaim all rebuildable artifacts:

```sh
just local-cleanup-space --apply
```

The cold cleanup is intentionally aggressive: it removes `codex-rs/target`,
legacy root targets, `code-rs` debug/dev-fast artifacts, all `./build-fast.sh`
target cache buckets, and release dependency cache. It preserves
`code-rs/target/release/code`, the PATH-resolved local binary built by
`just local-code-rebuild`.

Run without `--apply` to preview deletions. Use `--keep-release-cache` only when
you intentionally want to preserve release dependency cache as well.

## Fork Health

Run this before and after upstream syncs:

```sh
just local-product-health
```

Pay particular attention to changes under `code-rs/tui/src/chatwidget.rs`,
`code-rs/core/src/patch_harness.rs`, model/version files, and release scripts.
Those are the highest-conflict long-term carry points.
