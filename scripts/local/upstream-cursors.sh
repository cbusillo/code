#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

metadata="$repo_root/.github/github.json"

if [[ ! -f "$metadata" ]]; then
	echo "error: metadata file not found: $metadata" >&2
	exit 1
fi

fetch_refs=true
if [[ "${1:-}" == "--no-fetch" ]]; then
	fetch_refs=false
elif [[ $# -gt 0 ]]; then
	echo "usage: $0 [--no-fetch]" >&2
	exit 1
fi

cursor_file_relative="$(jq -r '.upstreamReviewTracking.cursorFile // empty' "$metadata")"
if [[ -z "$cursor_file_relative" ]]; then
	echo "error: upstreamReviewTracking.cursorFile is missing from $metadata" >&2
	exit 1
fi

cursor_file="$repo_root/$cursor_file_relative"
if [[ ! -f "$cursor_file" ]]; then
	echo "error: cursor file not found: $cursor_file_relative" >&2
	exit 1
fi

product_branch="$(jq -r '.productBranch // .defaultBranch // "main"' "$cursor_file")"

if ! git rev-parse --verify --quiet "$product_branch^{commit}" >/dev/null; then
	echo "error: product branch '$product_branch' is not available locally" >&2
	exit 1
fi

mapfile -t cursor_keys < <(jq -r '.upstreams // {} | keys[]' "$cursor_file")
if [[ ${#cursor_keys[@]} -eq 0 ]]; then
	echo "error: no upstream entries found in $cursor_file_relative" >&2
	exit 1
fi

if [[ "$fetch_refs" == true ]]; then
	for key in "${cursor_keys[@]}"; do
		remote="$(jq -r --arg key "$key" '.upstreams[$key].remote' "$cursor_file")"
		branch="$(jq -r --arg key "$key" '.upstreams[$key].branch' "$cursor_file")"
		echo "Fetching $remote/$branch"
		git fetch "$remote" "$branch" >/dev/null
	done
	echo
fi

product_head="$(git rev-parse --short=12 "$product_branch")"
echo "Product branch: $product_branch ($product_head)"
echo "Cursor file:    $cursor_file_relative"
echo

for key in "${cursor_keys[@]}"; do
	remote="$(jq -r --arg key "$key" '.upstreams[$key].remote' "$cursor_file")"
	branch="$(jq -r --arg key "$key" '.upstreams[$key].branch' "$cursor_file")"
	repository="$(jq -r --arg key "$key" '.upstreams[$key].repository // "unknown"' "$cursor_file")"
	role="$(jq -r --arg key "$key" '.upstreams[$key].role // "upstream source"' "$cursor_file")"
	last_examined="$(jq -r --arg key "$key" '.upstreams[$key].lastExamined.commit // empty' "$cursor_file")"
	last_imported="$(jq -r --arg key "$key" '.upstreams[$key].lastImported.commit // empty' "$cursor_file")"
	remote_ref="$remote/$branch"

	echo "[$key] $repository"
	echo "  role:          $role"
	echo "  remote ref:    $remote_ref"
	echo "  lastExamined:  ${last_examined:-missing}"
	echo "  lastImported:  ${last_imported:-not recorded}"

	if [[ -z "$last_examined" ]]; then
		echo "  status:        missing lastExamined cursor"
		echo
		continue
	fi

	missing=false
	if ! git rev-parse --verify --quiet "$remote_ref^{commit}" >/dev/null; then
		echo "  status:        remote ref is not available locally"
		missing=true
	fi
	if ! git rev-parse --verify --quiet "$last_examined^{commit}" >/dev/null; then
		echo "  status:        lastExamined commit is not available locally"
		missing=true
	fi
	if [[ "$missing" == true ]]; then
		echo
		continue
	fi

	remote_head="$(git rev-parse "$remote_ref")"
	remote_head_short="$(git rev-parse --short=12 "$remote_ref")"
	if [[ "$remote_head" == "$last_examined" ]]; then
		review_delta=0
	else
		review_delta="$(git rev-list --count "$last_examined..$remote_ref")"
	fi

	read -r product_only remote_only < <(git rev-list --left-right --count "$product_branch...$remote_ref")
	merge_base="$(git merge-base "$product_branch" "$remote_ref")"
	merge_base_short="$(git rev-parse --short=12 "$merge_base")"

	echo "  remote head:   $remote_head_short"
	echo "  merge-base:    $merge_base_short"
	echo "  review delta:  $review_delta commits after lastExamined"
	echo "  branch delta:  $product_only product-only / $remote_only upstream-only"

	if [[ "$review_delta" != "0" ]]; then
		echo "  new commits:"
		git log --oneline --max-count=10 "$last_examined..$remote_ref" | sed 's/^/    /'
		remaining=$((review_delta - 10))
		if ((remaining > 0)); then
			echo "    ... plus $remaining more"
		fi
	fi
	echo
done
