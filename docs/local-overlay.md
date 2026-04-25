# Local Overlay

`local/cbusillo-overlay` is the canonical branch for the local `code` binary on
this machine. It carries fork-specific behavior on top of `upstream/main`
(`just-every/code`). Treat `codex-rs/` as a read-only mirror and put editable
Rust changes under `code-rs/`.

## Owned Differences

- **Remote Inbox:** remote session control, request-user-input forwarding, and
  remote Auto Drive triggers. Keep transport/protocol code isolated from TUI
  event handling where possible.
- **Auto Drive integration:** coordinator UI, Esc semantics, status surfaces,
  and local crash diagnostics. Keep routing in `chatwidget.rs` and `app.rs`
  consistent with `AGENTS.md`.
- **Patch harness:** local validation for changed files, project tool discovery,
  and workspace-aware validator execution.
- **Release workflow:** `overlay-v*` binary releases and local PATH rebuilds are
  fork-owned infrastructure.
- **Model defaults:** local defaults may intentionally differ from upstream, but
  request wire compatibility should use upstream model metadata when available.

## Upstream Sync

Use the overlay helper from a clean `local/*` branch:

```sh
just local-overlay-update
```

After conflicts are resolved, preserve upstream fixes unless they contradict an
owned overlay behavior above. Keep `scripts/local/overlay-picks.txt` empty unless
a patch intentionally lives outside the overlay branch and must be replayed.

## Release Cadence

Cut a fork release after every successful upstream sync or local hotfix that
should be installed elsewhere.

Tag format:

```text
overlay-v<upstream-version>.<fork-patch>
```

Examples:

- `overlay-v0.6.95.1` for the first fork release on upstream `0.6.95`
- `overlay-v0.6.95.2` for a second local hotfix on the same upstream base
- `overlay-v0.6.96.1` after the next upstream version bump

Before pushing a release tag:

```sh
git status --short --branch
./build-fast.sh
just local-code-rebuild
code --version
code exec -m gpt-5.5 --sandbox read-only --max-seconds 30 "Reply with exactly OK."
```

Then push the branch and tag, and monitor `Binary Release`.

## Fork Health

Run this before and after upstream syncs:

```sh
scripts/local/fork-health.sh
```

Pay particular attention to changes under `code-rs/tui/src/chatwidget.rs`,
`code-rs/core/src/patch_harness.rs`, model/version files, and release scripts.
Those are the highest-conflict long-term carry points.
