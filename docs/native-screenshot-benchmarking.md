# Native Screenshot Benchmarking

Milestone 1 uses repeatable screenshot captures to catch transcript/activity,
approval, and runtime-state regressions early.

The benchmark runner uses deterministic fixtures so each scenario captures a
stable, named UI state independent of live backend state.

## Scenario Set

Scenarios live in `native/CodeNative/automation/benchmarks/`:

- `transcript-idle`
- `transcript-long`
- `activity-heavy`
- `approval-pending`
- `disconnected-state`
- `settings-shell`

Deterministic fixture data lives in
`native/CodeNative/automation/benchmarks/fixtures/` with one fixture per
scenario.

## Run Benchmarks

```bash
scripts/ux/benchmark-native-ui.sh
```

Artifacts are written to:

- `.code/ux-bench/native-ui/current/`

Update baseline artifacts intentionally:

```bash
UPDATE_BASELINE=1 scripts/ux/benchmark-native-ui.sh
```

Baseline directory:

- `docs/reference/native-ui/baseline/`

## Cross-App Parity Captures

We can run and interact with Every Code TUI, Codex Mac app, and native app,
then capture screenshots for side-by-side parity checks.

Parity triplets are stored under:

- `docs/reference/native-ui/parity/`

Suggested naming for parity triplets:

- `<scenario>-tui.png`
- `<scenario>-codex-mac.png`
- `<scenario>-native.png`

Milestone 1 includes a representative triplet:

- `docs/reference/native-ui/parity/workflow-active-tui.png`
- `docs/reference/native-ui/parity/workflow-active-codex-mac.png`
- `docs/reference/native-ui/parity/workflow-active-native.png`
