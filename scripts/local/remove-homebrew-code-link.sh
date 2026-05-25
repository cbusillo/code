#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
repo_release_bin="$repo_root/code-rs/target/release/code"
homebrew_links=(
	"/opt/homebrew/bin/code"
	"/usr/local/bin/code"
)

canonical_path() {
	local path="$1"
	if realpath "$path" 2>/dev/null; then
		return 0
	fi

	local parent
	parent="$(dirname "$path")"
	if parent="$(cd "$parent" >/dev/null 2>&1 && pwd -P)"; then
		printf '%s/%s\n' "$parent" "$(basename "$path")"
		return 0
	fi

	printf '%s\n' "$path"
}

repo_release_bin_real="$(canonical_path "$repo_release_bin")"

resolve_link_target() {
	local link_path="$1"
	local link_target="$2"
	case "$link_target" in
	/*) ;;
	*)
		link_target="$(dirname "$link_path")/$link_target"
		;;
	esac
	canonical_path "$link_target"
}

removed=0
for homebrew_link in "${homebrew_links[@]}"; do
	if [[ ! -L "$homebrew_link" ]]; then
		continue
	fi

	link_target="$(readlink "$homebrew_link")"
	link_target_real="$(resolve_link_target "$homebrew_link" "$link_target")"
	if [[ "$link_target_real" != "$repo_release_bin_real" ]]; then
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
