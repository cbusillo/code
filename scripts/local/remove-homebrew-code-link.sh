#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
homebrew_link="/opt/homebrew/bin/code"
repo_release_bin="$repo_root/code-rs/target/release/code"

if [[ ! -L "$homebrew_link" ]]; then
	echo "No Homebrew code symlink to remove: $homebrew_link"
	exit 0
fi

link_target="$(readlink "$homebrew_link")"
if [[ "$link_target" != "$repo_release_bin" ]]; then
	echo "Refusing to remove $homebrew_link; it points to $link_target" >&2
	exit 1
fi

rm -f "$homebrew_link"
echo "Removed stale Homebrew code symlink: $homebrew_link"
