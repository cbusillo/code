## @just-every/code v0.6.108

This release tightens the local release metadata workflow for smaller, cleaner release PRs.

### Changes

- Require locally prepared release metadata before publishing, with CI validation that the generated release notes match the target version.
- Make local release note generation scale down for tiny releases, including concise one-bullet GitHub notes when that is the clearest output.

### Install

```bash
gh release download v0.6.108 --repo cbusillo/code
```

Compare: https://github.com/cbusillo/code/compare/v0.6.107...v0.6.108
