#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"
manifest="$repo_root/scripts/local/overlay-branches.txt"

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

if [[ ! -f "$manifest" ]]; then
	echo "error: overlay branch manifest not found: $manifest" >&2
	exit 1
fi

echo "Fetching upstream"
git fetch upstream --tags

echo "Merging upstream/main into $branch"
git merge --no-edit upstream/main

echo
echo "Applying overlay branch stack from $(basename "$manifest")"
while IFS= read -r raw_line || [[ -n "$raw_line" ]]; do
	line="${raw_line%%#*}"
	branch_name="$(printf '%s' "$line" | xargs)"
	if [[ -z "$branch_name" ]]; then
		continue
	fi

	if ! git show-ref --verify --quiet "refs/heads/$branch_name"; then
		echo "error: overlay branch '$branch_name' does not exist locally" >&2
		exit 1
	fi

	if git merge-base --is-ancestor "$branch_name" HEAD; then
		echo "- $branch_name already merged"
		continue
	fi

	echo "- merging $branch_name"
	git merge --no-edit "$branch_name"
done <"$manifest"

echo
"$repo_root/scripts/local/rebuild-path-code.sh"
