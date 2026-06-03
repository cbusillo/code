---
name: remote-tests
description: How to run tests using remote executor.
commands:
  - name: build-remote-env
    source: repo
    example_argv: ["./scripts/test-remote-env.sh"]
    purpose: Build and initialize the Docker container used by remote executor tests.
  - name: list-devboxes
    source: external
    example_argv: ["applied_devbox", "ls"]
    purpose: List available devboxes before selecting one with code in the name.
  - name: connect-devbox
    source: external
    example_argv: ["ssh", "<devbox_name>"]
    purpose: Connect to the selected Linux devbox.
workflow_defaults:
  - name: remote_env
    value: CODEX_TEST_REMOTE_ENV
    description: Set when integration tests should use a remote executor.
  - name: remote_checkout
    value: ~/code/code
    description: Reuse the devbox checkout and keep SHA and modified files in sync.
---

Some Every Code integration tests support running against a remote executor.
This means that when `CODEX_TEST_REMOTE_ENV` is set they will attempt to start
an executor process in a Docker container `CODEX_TEST_REMOTE_ENV` points to and
use it in tests.

Docker container is built and initialized via ./scripts/test-remote-env.sh

Currently running remote tests is only supported on Linux, so you need to use a
devbox to run them.

You can list devboxes via `applied_devbox ls`, pick the one with `code` in the
name.
Connect to devbox via `ssh <devbox_name>`.
Reuse the same checkout in `~/code/code`. Reset files if needed. Multiple
checkouts take longer to build and take up more space.
Check whether the SHA and modified files are in sync between remote and local.
