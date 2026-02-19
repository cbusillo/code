# AGENTS Override (Repo-Specific)

This file defines repo-specific operating rules for agents working in this
repository. Read `AGENTS.md` first, then apply this file as an override.

## Source Of Truth

- The active native app roadmap lives in `PLAN.md`.
- Keep implementation aligned with `PLAN.md` phases and acceptance criteria.
- If scope changes, update `PLAN.md` in the same workstream.

## Branching And Git Flow

- Default integration target is `origin/fork-main`.
- Start feature work from `origin/fork-main` on a dedicated feature branch.
- Do not commit directly to `main` or `fork-main` except release/maintainer flows.
- Prefer merge-based sync (no rebase-first workflow unless explicitly requested).

## Fork And Upstream Expectations

- Treat `origin` as the primary collaboration remote for day-to-day work.
- Use `upstream` for periodic sync/reference only.
- Keep fork-specific behavior documented here or in `PLAN.md`.
- Do not put fork policy in temporary notes.

## Validation Policy

- Required completion check remains `./build-fast.sh` from repo root.
- Treat all warnings as failures.
- For UI work, validate behavior using browser tooling before finishing.
- Use IntelliJ inspections (via Idea inspections MCP) on touched areas when practical,
  and fix real issues before finalizing.

## UI/UX Source Of Truth

- Follow `UI_UX_REFERENCE.md` for native Apple UI/UX decisions.
- Keep naming, sizing, spacing, and hierarchy aligned with that reference.
- For native UI changes, apply the `Pre-Merge UX Gate` from
  `UI_UX_REFERENCE.md` before finalizing.

## Native/TUI Parity Bar

- Native apps and TUI are equal clients of one session model.
- Preserve strict ordering metadata and replay semantics.
- Changes that affect one surface should be evaluated for parity impact on the other.

## Release Policy

- Release through GitHub-based distribution for this line of work.
- Do not publish this fork's native effort to npm by default.
- If release automation is added, keep it scoped to GitHub releases.
- Document release automation changes in this file or `PLAN.md`.

## Documentation Hygiene

- Keep this file durable and policy-oriented.
- Put tactical execution details in `PLAN.md`.
- Do not leave temporary scratch instructions in repo docs.
