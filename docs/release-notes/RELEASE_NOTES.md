## @just-every/code v0.6.111

This release improves skill helper metadata and validation in Every Code.

### Changes

- Add structured command, resource, and default metadata for the repo-local PR watcher and issue digest skills.
- Reject skills that reference missing command helpers so invalid workflow metadata fails during load.

### Install

```bash
gh release download v0.6.111 --repo cbusillo/code
```

Compare: https://github.com/cbusillo/code/compare/v0.6.110...v0.6.111
