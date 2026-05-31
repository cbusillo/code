---
name: babysit-pr
description: Watch a pull request through checks and review follow-ups without treating readiness as merge intent.
---

# Babysit PR

Use this skill when asked to watch, babysit, or keep an eye on a GitHub pull
request. It is a coordination entry point for the repo's PR workflow metadata,
not an automatic merge command.

## Workflow

1. Read `.github/github.json`, especially `prWorkflow`, `importantWorkflows`,
   `githubSignals`, and `cleanup`.
2. Identify the PR and branch with `gh pr view --json` and check whether it is
   draft, mergeable, blocked, behind, or waiting on review.
3. Watch required checks with `gh pr checks --watch` or, for a known workflow,
   the repo's configured wait command.
4. If checks fail, inspect the failing job and report the smallest actionable
   fix. Do not mark the PR ready.
5. If checks pass, look for fresh review comments, auto-review findings, and
   branch drift. Apply or report genuine findings before declaring readiness.
6. Report readiness separately from merge intent. This repo's metadata says a
   ready PR still needs explicit user approval before merging.
7. After an approved merge, verify the post-merge GitHub signals configured in
   `.github/github.json` when available, then clean only safe local artifacts.

## Commands

Useful commands:

```sh
gh pr view <pr> --json state,isDraft,mergeStateStatus,reviewDecision,url
gh pr checks <pr> --watch
gh pr view <pr> --comments
```

Use `./build-fast.sh` as the local validation gate for code changes in this
repository unless the user explicitly asks for a different check.
