# Code Exec Harness

`tools/code-exec-harness/harness.py` runs isolated `code exec --json` scenarios
and saves compact evidence under `.tmp/code-exec-harness/`.

Use it to compare Every Code behavior across prompt, memory, skill, model, and
configuration variants without writing to real GitHub or reusing the real
`CODE_HOME`.

The harness defaults `code exec` to `danger-full-access` because external tool
shims such as fake `gh` need to write logs and state outside the fixture
workspace. Run it only with trusted scenarios.

By default, each scenario gets an empty `CODE_HOME`. Pass `--inherit-auth` only
when you want a live model-backed run; it copies Code auth and GitHub CLI config
into the isolated run home without copying the rest of your config.

`HOME`, `ZDOTDIR`, `XDG_CONFIG_HOME`, and `XDG_CACHE_HOME` are also redirected
inside the run directory so shell startup files and home-directory tooling do
not silently use the real user profile. When auth is inherited, `GH_CONFIG_DIR`
points at the copied GitHub CLI config inside that redirected home.

## Run

```sh
python3 tools/code-exec-harness/harness.py \
  tools/code-exec-harness/scenarios/github-plan-smoke.json \
  --skill-root /Users/cbusillo/Developer/codex-skills \
  --inherit-auth
```

Each run writes:

- `artifacts/stdout.jsonl`: raw `code exec --json` events
- `artifacts/stderr.log`: stderr from the run
- `artifacts/summary.json`: final answer, token usage, tool commands, fake `gh`
  calls, fake GitHub state, and expectation failures
- `artifacts/gh-calls.jsonl`: fake `gh` invocations when the scenario enables it
- `artifacts/gh-state.json`: fake issue state after the run

## Scenario Shape

Scenarios are JSON files. Common fields:

- `prompt`: prompt passed to `code exec`
- `files`: workspace files to create before the run
- `skill_roots`: skill roots copied or symlinked into isolated `CODE_HOME/skills`
- `gh`: fake GitHub fixture; when present, the harness prepends a fake `gh` to
  `PATH`
- `config_toml`: isolated `CODE_HOME/config.toml` contents
- `config_overrides`: `-c key=value` arguments passed to `code exec`
- `inherit_auth`: copy Code auth and GitHub CLI config for this scenario
- `expect`: simple assertions over the final answer, commands, fake `gh` calls,
  and exit code

The harness is intentionally black-box: the unit under test is the real `code
exec` binary and its emitted JSONL stream.
