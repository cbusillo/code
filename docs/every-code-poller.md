# Every Code Local Poller

`scripts/every-code-poller.sh` watches GitHub issues from this host and opens
visible local Code sessions for issues that are ready for automation.

The trigger contract has two parts:

- Repository topic `every-code`: this repo is part of the local work queue.
- Issue label `every-code`: this issue may be picked up by the local poller.

The label must be applied by a trusted user. By default, trusted means the label
actor has `write`, `maintain`, or `admin` permission on the repo. This keeps
arbitrary issue authors from triggering local automation just by adding a label.
If the poller cannot verify the label actor's permission, it marks the issue
blocked and comments with the lookup error instead of silently skipping it.
Users listed in `EVERY_CODE_TRUSTED_USERS` bypass the repo-permission check, so
only put fully trusted automation or maintainer accounts there.

The poller uses a `tmux` session named `every-code`. The `poller` window scans
GitHub on an interval, and every claimed issue gets its own window named like
`ec-owner-repo-123-...`. Job windows stay open after `code exec` finishes so the
run can be inspected.

## Commands

```sh
scripts/every-code-poller.sh start
scripts/every-code-poller.sh attach
scripts/every-code-poller.sh stop
scripts/every-code-poller.sh once
```

`start` creates the session if needed and attaches to it. `once` is useful for a
manual scan without starting the loop.

## Labels

The script creates and maintains these labels in watched repos:

- `every-code`: ready for local automation.
- `every-code/working`: a local tmux window has claimed the issue.
- `every-code/done`: `code exec` completed successfully.
- `every-code/blocked`: local automation could not complete the issue.

If this host has no checkout for a watched repo, the issue is marked blocked and
a comment explains the missing checkout. By default, the checkout root is the
parent directory of the repo where the script is run. The poller checks both
`<root>/<repo-name>` and `<root>/<owner>/<repo-name>`.

## Configuration

Common environment overrides:

```sh
EVERY_CODE_OWNER=cbusillo                  # defaults to gh api user.login
EVERY_CODE_CHECKOUT_ROOT=/path/to/checkouts # defaults to parent of current repo
EVERY_CODE_POLL_INTERVAL_SECONDS=60
EVERY_CODE_MODEL=gpt-5.5
EVERY_CODE_SANDBOX=workspace-write
EVERY_CODE_MAX_SECONDS=7200
EVERY_CODE_TRUSTED_PERMISSIONS=admin,maintain,write
EVERY_CODE_TRUSTED_USERS=cbusillo,some-bot
EVERY_CODE_LOCK_STALE_SECONDS=900
```

Use a maintainer-applied label as the approval step. Issue text is untrusted
input, and the poller should not be run against arbitrary unlabeled issues.
