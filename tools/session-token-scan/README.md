# Session Token Scan

`tools/session-token-scan/scan.py` is a deterministic, read-only scanner for
Every Code rollout/session files. It highlights token-efficiency suspects before
prompt, memory, agent, or history behavior is changed.

The scanner does not call an LLM. Optional local or cheaper model classifiers
should be layered on later and should consume only the compact scanner output or
narrowed suspect spans, not whole raw rollout files.

## Run

```sh
python3 tools/session-token-scan/scan.py ~/.code/sessions/2026/05/11 --limit 20
```

Useful options:

- `--limit 0`: scan all discovered rollout files under the inputs.
- `--json`: emit machine-readable output for PR evidence or later classifiers.
- `--usage-root ~/.code/usage`: correlate timestamped usage entries when
  available. When the usage root contains multiple account files, pass
  `--account-id <id>` to avoid mixing unrelated account usage into a session.
- `--account-id <id>`: filter usage correlation to one account.
- `--large-threshold 16384`: set the byte threshold for large-record suspects.

## Reports

The text report includes:

- peak cumulative and largest single-turn token usage
- cache ratio from recorded token counts
- token total reset counts when cumulative counters restart inside a rollout
- rollout file size, image payload bytes, and base-instruction size
- duplicated project-doc/skill injection flags
- largest token-count events
- largest persisted records and string fields
- `data:image` and `input_image` suspects
- optional usage-entry totals that overlap the session timestamp range

## Scope

This tool reads local files only. It does not mutate session history, enforce
budgets, change model routing, or compact stored payloads.
