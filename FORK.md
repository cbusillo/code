# Fork policy

This repository tracks upstream closely, but we carry a long-running fork branch.

## Branches

- `main` mirrors upstream (`upstream/main`). Keep it fast-forward only.
- `webui-main` is the fork’s default branch; fork-only changes land here.
- Sync upstream into the fork via merge-only: merge `origin/main` into
  `webui-main` (do not rebase).

## Releases (fork)

- Workflow: `Release (Fork)` (`.github/workflows/release-fork.yml`).
- Trigger: push a tag matching `v*`.
- Tags must point at the desired `webui-main` commit (not at `main`).

### Version scheme

We stay aligned with upstream versions while allowing fork-only patch releases.

- Upstream version: `vX.Y.Z`
- Fork patch releases: `vX.Y.Z.N` (extra decimal), where `N` starts at `1`.

Examples:

- Upstream `v0.6.59` → fork patches `v0.6.59.1`, `v0.6.59.2`, …

## Monitoring

`scripts/wait-for-gh-run.sh` uses the current repo by default.

- When monitoring the fork from a local checkout, set `GH_REPO=cbusillo/code`.
