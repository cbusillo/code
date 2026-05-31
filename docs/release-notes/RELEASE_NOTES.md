## @just-every/code v0.6.112

This release tightens skill command metadata and adds upstream review cursor tooling.

### Changes

- Support repo and external skill commands without breaking existing command metadata.
- Reject skill commands that point at non-script resources, so templates and references cannot be treated as runnable helpers.
- Add an upstream cursor report for tracking reviewed commits across Every Code's upstream sources.

### Install

```bash
gh release download v0.6.112 --repo cbusillo/code
```

Compare: https://github.com/cbusillo/code/compare/v0.6.111...v0.6.112
