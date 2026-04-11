#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"
manifest="$repo_root/scripts/local/overlay-picks.txt"

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
echo "Applying overlay commit stack from $(basename "$manifest")"
while IFS= read -r raw_line || [[ -n "$raw_line" ]]; do
	line="${raw_line%%#*}"
	pick_commit="$(printf '%s' "$line" | xargs)"
	if [[ -z "$pick_commit" ]]; then
		continue
	fi

	if ! git cat-file -e "$pick_commit^{commit}" 2>/dev/null; then
		echo "error: overlay commit '$pick_commit' does not exist" >&2
		exit 1
	fi

	if git merge-base --is-ancestor "$pick_commit" HEAD; then
		echo "- $pick_commit already present"
		continue
	fi

	echo "- cherry-picking $pick_commit"
	git cherry-pick "$pick_commit"
done <"$manifest"

echo
"$repo_root/scripts/local/rebuild-path-code.sh"
