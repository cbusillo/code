# Fork Policy (Every Code)

## Branch roles

- `main`: mirror of upstream `just-every/code` `main`. No fork-only commits.
  No release workflow.
- `webui-main`: fork release branch and GitHub default. Contains fork-only
  patches. Release workflow triggers here.
- `app-server-v2`: feature branch for v2 app-server changes. Keep PR-ready for
  upstream.
- `fix/gpt-5.3-codex`: patch branch for the gpt-5.3 auth preference fix. Keep
  PR-ready for upstream.

## Updating `webui-main`

- Merge (no rebase) from upstream `main` or `origin/main` as needed.
- Merge or cherry-pick fork patches from feature branches.
- Run `./build-fast.sh` before pushing; use `./pre-release.sh` for release
  preflight.

## Release workflow

- The Release workflow must trigger only on pushes/tags to `webui-main`.
- It must not trigger on `main`.

## Versioning

- Track upstream versions.
- Fork patch releases append an extra decimal: `vX.Y.Z.N` (N starts at 1).
- Example: upstream `v0.6.59` -> fork `v0.6.59.1`.

## Monitoring

- Use `scripts/wait-for-gh-run.sh` to follow Release runs.
- For the fork, set `GH_REPO=cbusillo/code`.

## Auth/model note

- `gpt-5.3-codex` requires ChatGPT auth in this fork. If you see
  “model does not exist or you do not have access,” confirm
  `using_chatgpt_auth` and stored ChatGPT credentials.
