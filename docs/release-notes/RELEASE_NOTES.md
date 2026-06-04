## Every Code v0.6.116

This release reduces prompt-cache churn so repeated Every Code turns are less likely to burn through rate limits, and adds a visible cache signal for dogfooding.

### Changes

- Stable prompt content now stays ahead of volatile context such as CTX_UI timelines, environment snapshots, and Auto Review ledger details, helping providers reuse cached input across turns.
- The TUI footer now shows prompt-cache hit rate when the provider reports cached input token telemetry, including a token-weighted rolling average for recent turns.
- App-server protocol schemas now expose whether cached input token telemetry was reported, so missing cache data is not confused with a real 0% cache hit.

### Install

```bash
gh release download v0.6.116 --repo cbusillo/code
```

Compare: https://github.com/cbusillo/code/compare/v0.6.115...v0.6.116
