#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
repo_name="$(basename "$repo_root")"

apply=0
keep_current_fast_cache=0
keep_release_cache=0
cleanup_failed=0
declare -a protected_realpaths=()

usage() {
	cat <<USAGE
Usage: scripts/local/cleanup-space.sh [--apply] [--keep-current-fast-cache] [--keep-release-cache]

Aggressively removes local build artifacts that make this checkout grow large
while keeping the PATH-resolved local release binary intact.

Default mode is a dry run. Pass --apply to delete the listed paths.

Options:
  --apply                    Delete paths instead of only listing them.
  --keep-current-fast-cache  Preserve the active build-fast cache bucket.
  --keep-release-cache       Preserve code-rs/target/release dependency cache.
  -h, --help                 Show this help.
USAGE
}

while [[ $# -gt 0 ]]; do
	case "$1" in
	--apply)
		apply=1
		;;
	--keep-current-fast-cache)
		keep_current_fast_cache=1
		;;
	--keep-release-cache)
		keep_release_cache=1
		;;
	-h | --help)
		usage
		exit 0
		;;
	*)
		echo "error: unknown option: $1" >&2
		usage >&2
		exit 2
		;;
	esac
	shift
done

real_path() {
	python3 - "$1" <<'PY'
import os
import sys

print(os.path.realpath(sys.argv[1]))
PY
}

protect_path() {
	local path="$1"

	if [[ -e "$path" || -L "$path" ]]; then
		protected_realpaths+=("$(real_path "$path")")
	fi
}

is_under() {
	local child="$1"
	local parent="$2"

	[[ "$child" == "$parent" || "$child" == "$parent"/* ]]
}

path_protects_active_code() {
	local path="$1"
	local path_real
	local protected

	path_real="$(real_path "$path")"
	for protected in "${protected_realpaths[@]}"; do
		if is_under "$protected" "$path_real"; then
			return 0
		fi
	done

	return 1
}

human_size() {
	local path="$1"

	if [[ -e "$path" || -L "$path" ]]; then
		du -sh "$path" 2>/dev/null | awk '{print $1}'
	else
		printf '0B'
	fi
}

remove_path() {
	local path="$1"
	local note="${2:-}"

	if [[ ! -e "$path" && ! -L "$path" ]]; then
		return 0
	fi

	local size
	size="$(human_size "$path")"
	if path_protects_active_code "$path"; then
		if [[ -n "$note" ]]; then
			printf '%8s  %s  (%s, preserved active code path)\n' "$size" "$path" "$note"
		else
			printf '%8s  %s  (preserved active code path)\n' "$size" "$path"
		fi
		return 0
	fi

	if [[ -n "$note" ]]; then
		printf '%8s  %s  (%s)\n' "$size" "$path" "$note"
	else
		printf '%8s  %s\n' "$size" "$path"
	fi

	if [[ "$apply" -eq 1 ]]; then
		if ! rm -rf -- "$path" 2>/dev/null; then
			sleep 1
			if ! rm -rf -- "$path" 2>/dev/null; then
				echo "warning: failed to remove $path" >&2
				cleanup_failed=1
			fi
		fi
	fi
}

remove_release_cache() {
	local release_dir="$repo_root/code-rs/target/release"

	[[ -d "$release_dir" ]] || return 0
	while IFS= read -r -d '' path; do
		if [[ "$(basename "$path")" = "code" ]]; then
			continue
		fi
		remove_path "$path" "release cache, preserving code binary"
	done < <(find "$release_dir" -mindepth 1 -maxdepth 1 -print0 | sort -z)
}

mode="dry run"
if [[ "$apply" -eq 1 ]]; then
	mode="apply"
fi

echo "Local space cleanup ($mode)"
echo "Repo: $repo_root"
echo

path_code="$(command -v code || true)"
release_bin="$repo_root/code-rs/target/release/code"
release_bin_real=""
if [[ -n "$path_code" ]]; then
	resolved_code="$(real_path "$path_code")"
	echo "PATH code:     $path_code"
	echo "Resolves to:   $resolved_code"
else
	resolved_code=""
	echo "PATH code:     not found"
fi
echo "Release bin:   $release_bin"
if [[ -e "$release_bin" || -L "$release_bin" ]]; then
	release_bin_real="$(real_path "$release_bin")"
	echo "Release real:  $release_bin_real"
fi
if [[ -n "$resolved_code" && "$resolved_code" != "$release_bin" && "$resolved_code" != "$release_bin_real" ]]; then
	echo "warning: PATH 'code' does not resolve to the local release binary" >&2
fi
echo

protect_path "$path_code"
protect_path "$release_bin"

echo "Cleanup candidates:"
target_cache_root="$repo_root/.code/working/_target-cache/$repo_name"
active_fast_bin=""
if [[ -e "$repo_root/code-rs/target/dev-fast/code" || -L "$repo_root/code-rs/target/dev-fast/code" ]]; then
	active_fast_bin="$(real_path "$repo_root/code-rs/target/dev-fast/code")"
fi

remove_path "$repo_root/codex-rs/target" "upstream mirror build artifacts"
remove_path "$repo_root/target/debug" "legacy/root debug artifacts"
remove_path "$repo_root/target/dev-fast" "legacy/root fast-build artifacts"
remove_path "$repo_root/target/tmp" "legacy/root temporary target files"
remove_path "$repo_root/code-rs/target/debug" "local debug artifacts"
if [[ "$keep_current_fast_cache" -eq 0 ]]; then
	remove_path "$repo_root/code-rs/target/dev-fast" "local fast-build symlinks/artifacts"
fi
remove_path "$repo_root/code-rs/target/tmp" "local temporary target files"

if [[ -d "$target_cache_root" ]]; then
	while IFS= read -r -d '' bucket; do
		bucket_real="$(real_path "$bucket")"
		if [[ "$keep_current_fast_cache" -eq 1 && -n "$active_fast_bin" ]] && is_under "$active_fast_bin" "$bucket_real"; then
			continue
		fi
		remove_path "$bucket" "build-fast cache bucket"
	done < <(find "$target_cache_root" -mindepth 1 -maxdepth 1 -type d -print0 | sort -z)
fi

if [[ "$keep_release_cache" -eq 0 ]]; then
	remove_release_cache
fi

echo
if [[ "$apply" -eq 1 ]]; then
	if [[ "$cleanup_failed" -eq 1 ]]; then
		echo "Cleanup finished with warnings. Some paths could not be removed." >&2
		exit 1
	fi
	echo "Cleanup complete. The release binary path was preserved."
else
	echo "Dry run only. Re-run with --apply to delete these paths."
fi
