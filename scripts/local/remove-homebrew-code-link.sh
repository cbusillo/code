#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
repo_release_bin="$repo_root/code-rs/target/release/code"
homebrew_links=(
	"/opt/homebrew/bin/code"
	"/usr/local/bin/code"
)

removed=0
for homebrew_link in "${homebrew_links[@]}"; do
	if [[ ! -L "$homebrew_link" ]]; then
		continue
	fi

	link_target="$(readlink "$homebrew_link")"
	if [[ "$link_target" != "$repo_release_bin" ]]; then
		echo "Skipping unrelated Homebrew code symlink: $homebrew_link -> $link_target"
		continue
	fi

	rm -f "$homebrew_link"
	removed=1
	echo "Removed stale Homebrew code symlink: $homebrew_link"
done

if [[ "$removed" -eq 0 ]]; then
	echo "No repo-owned Homebrew code symlinks found."
fi
