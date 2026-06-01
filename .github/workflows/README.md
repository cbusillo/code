# Workflow Strategy

Rust-specific `rust-ci*.yml` workflows are intentionally removed for Every Code.

## Verification Paths

- `./build-fast.sh` is the required local Rust verification gate.
- `preview-build.yml` builds preview binaries for pull requests.
- `release.yml` is displayed in GitHub Actions as `Release Intent`. It runs
  after relevant `main` pushes, determines whether the committed `VERSION` has
  an existing `v<version>` tag, and either exits successfully as a no-op or
  publishes the GitHub Release.

## Release Operator Notes

- A successful `Release Intent` run does not always mean a release was cut. It
  means the workflow either published the new `VERSION` or confirmed that the
  current version tag already exists.
- To cut a release, merge a release metadata PR that bumps `VERSION` and updates
  `CHANGELOG.md` plus `docs/release-notes/RELEASE_NOTES.md`.
- To verify publishing, check for the tag or release directly. Example:

  ```sh
  gh release view v0.6.112 --repo cbusillo/code
  ```

## Upstream Merge Guardrail

- `.github/workflows/**` is fork-owned during upstream merges.
- `.github/workflows/rust-ci.yml` and `.github/workflows/rust-ci-full.yml` are
  also listed in `.github/merge-policy.json` under `perma_removed_paths` so
  future upstream syncs keep them deleted.
