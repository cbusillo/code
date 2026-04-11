#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

branch="$(git symbolic-ref --quiet --short HEAD || true)"
if [[ -z "$branch" ]]; then
	echo "error: not on a branch" >&2
	exit 1
fi

if [[ "$branch" != local/* ]]; then
	echo "error: expected a local/* overlay branch, got '$branch'" >&2
	exit 1
fi

if ! git diff --quiet || ! git diff --cached --quiet; then
	echo "error: worktree is dirty; commit or stash changes before updating" >&2
	exit 1
fi

echo "Fetching upstream"
git fetch upstream --tags

echo "Merging upstream/main into $branch"
git merge --no-edit upstream/main

echo
"$repo_root/scripts/local/rebuild-path-code.sh"
