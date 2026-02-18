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
- `history-telemetry`
- `browser-workflow`
- `multi-agent-progress`
- `voice-guardrails`
- `request-user-input`
- `request-user-input-depth`
- `request-user-input-error`
- `command-launcher`
- `command-launcher-depth`
- `context-mention`
- `context-mention-depth`
- `git-recovery`
- `ide-integration`
- `settings-workflow-controls`
- `session-rail-scale`
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

Verify required PAR-010 baseline states are present and checksum-visible:

```bash
scripts/ux/verify-native-benchmark-baseline.sh
```

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

Milestone-tagged parity triplets:

- `docs/reference/native-ui/parity/workflow-active-{tui,codex-mac,native}.png`
- `docs/reference/native-ui/parity/m2-activity-heavy-{tui,codex-mac,native}.png`
- `docs/reference/native-ui/parity/m3-multi-agent-progress-{tui,codex-mac,native}.png`

Verify triplet completeness and reproducible checksums:

```bash
scripts/ux/verify-parity-triplets.sh
```

## PAR-019 Visual Rubric Gate

When enforcing PAR-019 (`visual quality rubric enforcement`), treat the
following deterministic scenarios as required review checkpoints:

- `transcript-long` (dense conversation hierarchy and readability)
- `activity-heavy` (task/activity card clarity under load)
- `approval-pending` (high-priority action prominence)
- `history-telemetry` (paging metrics and retention visibility)
- `request-user-input-depth` (interactive card depth and spacing consistency)
- `settings-workflow-controls` (settings information hierarchy and alignment)

Merge is blocked for unresolved high-severity visual regressions in any of
these checkpoints.

## PAR-022 Session Rail Scale Gate

For PAR-022 (`session rail/grouping ergonomics at scale`), include
`session-rail-scale` in each deterministic benchmark run. The screenshot must
show thread-rail grouping controls and legible grouped rows under a large,
fixture-backed session catalog.

## PAR-018 Browser Workflow Gate

For PAR-018 (`browser workflow parity`), include `browser-workflow` in each
deterministic benchmark run. The screenshot must show readable browser cards
covering in-progress, completed, and failure states with visible artifact
detail for at least one completed browser action.

## PAR-017 Multi-Agent Progress Gate

For PAR-017 (`multi-agent progress visualization`), include
`multi-agent-progress` in each deterministic benchmark run. The screenshot must
show coordinator progress cards with at least one in-progress, one completed,
and one error state visible in transcript flow.

## PAR-021 Voice Hardening Gate

For PAR-021 (`voice interaction parity hardening`), include
`voice-guardrails` in each deterministic benchmark run. The screenshot must
show voice input remaining blocked while an approval decision is pending,
without losing the current draft text.

## PAR-020 Telemetry Guardrail Gate

For PAR-020 (`performance guardrails + telemetry`), include
`history-telemetry` in each deterministic benchmark run. The screenshot must
show connection/runtime state plus non-zero history telemetry counters
(`req/ok/avg/slow`) and retention-trim counters when present.

## PAR-023 Cross-App Evidence Gate

For PAR-023 (`cross-app screenshot parity evidence`), keep milestone-tagged
triplets current in `docs/reference/native-ui/parity/` and verify with
`scripts/ux/verify-parity-triplets.sh` after refreshing captures.
