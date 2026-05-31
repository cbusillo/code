## @just-every/code v0.6.113

This release ships a small reliability fix for skill metadata and upstream cursor checks.

### Changes

- Normalize skill command resource paths before validation so helper scripts resolve consistently.
- Fetch upstream cursor refs without tags to avoid release tag collisions during review checks.

### Install

```bash
gh release download v0.6.113 --repo cbusillo/code
```

Compare: https://github.com/cbusillo/code/compare/v0.6.112...v0.6.113
