# EveryCode WebUI

Browser-native SvelteKit prototype for a continuity-first EveryCode companion.

This workspace is the fresh start for the new web product. It is intentionally
focused on the transcript first, because the transcript is where EveryCode’s UX
gets won or lost: reasoning, tool calls, diffs, approvals, questions, and
cross-device handoff all have to coexist without turning the page into noise.

## Current goal

The first milestone is not a fully wired app. It is a high-quality golden
transcript prototype that lets us judge the interaction model before we expand
the surrounding shell.

Related planning artifacts:

- `~/.codex/plans/every-code-webui-transcript-behavior-plan.md`
- `~/.codex/plans/every-code-transcript-ux-spec.md`

## Developing

Install dependencies in this package only:

```sh
pnpm install --ignore-workspace --dir every-code-webui
```

Run the local prototype:

```sh
pnpm --dir every-code-webui dev --host 127.0.0.1 --port 4173
```

Deterministic transcript fixture route:

```text
/prototype
```

## Validation

Package-local checks:

```sh
pnpm --dir every-code-webui check
pnpm --dir every-code-webui build
pnpm --dir every-code-webui test -- --run
```

## Near-term roadmap

1. Replace fixture-only data with a richer golden transcript dataset.
2. Add a typed WebSocket/domain layer using EveryCode’s generated protocol
   types.
3. Move from static continuity states to real read-only / continue /
   take-over semantics.
4. Build the session home and live transcript shell around the proven
   transcript cells.
