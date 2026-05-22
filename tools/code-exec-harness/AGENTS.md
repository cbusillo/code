# Code Exec Harness Agent Guidance

Use this harness as the default proving ground for realistic `code exec`
behavior when working on token efficiency, prompt/context composition, skills,
memory, compaction, resume, model routing, tool choice, and GitHub automation.

The goal is effectiveness-adjusted token efficiency: reduce wasted tokens only
when task success, reliability, instruction following, and useful tool behavior
are preserved or improved. Saving tokens by making the agent less capable is a
regression.

Validation order:

1. Prefer deterministic fake `/v1/responses` scenarios when request bodies,
   JSONL events, fake service state, or tool calls can prove the behavior.
2. Use live model runs when the question depends on model behavior that a fake
   response cannot represent.
3. Use local model runs when available and adequate for the scenario,
   especially for repeated exploratory checks.
4. Keep lower-level Rust tests for local mechanics, but add a harness scenario
   when the risk crosses config loading, skills, memory, resume, prompt
   assembly, tool routing, or full `code exec` behavior.

When adding or running scenarios, preserve evidence future agents can compare:

- request shape and duplicated context markers
- token usage when available
- tool commands and fake service calls
- final answer quality or task completion signal
- resume and compaction behavior across turns
- unexpected retries, confusion, or wasted work

For context-bloat regressions, prefer exact-count assertions over broad
contains/not_contains checks. A project doc, skill manifest, memory block,
summary, or large artifact placeholder should appear the intended number of
times, usually once.

Spending real OpenAI tokens in this harness is acceptable when it answers an
effectiveness question. Try fake responses first, but do not optimize away the
evidence needed to avoid larger token waste in real sessions.
