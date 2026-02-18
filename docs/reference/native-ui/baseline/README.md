# Native UI Baseline

This folder stores accepted baseline screenshots for the native benchmark suite.

Populate/update via:

```bash
UPDATE_BASELINE=1 scripts/ux/benchmark-native-ui.sh
```

Required PAR-010 baseline states:

- `transcript-idle.png`
- `transcript-long.png`
- `activity-heavy.png`
- `approval-pending.png`
- `disconnected-state.png`
- `settings-shell.png`

Verify baseline coverage with:

```bash
scripts/ux/verify-native-benchmark-baseline.sh
```
