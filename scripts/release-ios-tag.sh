#!/usr/bin/env bash

set -euo pipefail

usage() {
	cat <<'EOF'
Usage: scripts/release-ios-tag.sh [options] <version-or-tag>

Create a release tag for the iOS TestFlight pipeline.

Examples:
  scripts/release-ios-tag.sh 1.4.0
  scripts/release-ios-tag.sh ios-v1.4.0
  scripts/release-ios-tag.sh --ref HEAD~1 1.4.0
  scripts/release-ios-tag.sh --no-push 1.4.0

Options:
  --remote <name>   Git remote to push to (default: origin)
  --ref <rev>       Commit/tag to annotate (default: HEAD)
  --no-push         Create local tag only
  -h, --help        Show this help

Notes:
  - Tags are normalized to the format: ios-v<semver>
  - Pushing ios-v* tags triggers .github/workflows/native-ios-testflight.yml
EOF
}

remote="origin"
target_ref="HEAD"
push_tag=true
version_input=""

while [[ $# -gt 0 ]]; do
	case "$1" in
	--remote)
		[[ $# -ge 2 ]] || {
			echo "missing value for --remote" >&2
			exit 1
		}
		remote="$2"
		shift 2
		;;
	--ref)
		[[ $# -ge 2 ]] || {
			echo "missing value for --ref" >&2
			exit 1
		}
		target_ref="$2"
		shift 2
		;;
	--no-push)
		push_tag=false
		shift
		;;
	-h | --help)
		usage
		exit 0
		;;
	-*)
		echo "unknown option: $1" >&2
		usage
		exit 1
		;;
	*)
		if [[ -n "$version_input" ]]; then
			echo "expected a single version/tag argument" >&2
			usage
			exit 1
		fi
		version_input="$1"
		shift
		;;
	esac
done

if [[ -z "$version_input" ]]; then
	echo "missing version/tag argument" >&2
	usage
	exit 1
fi

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
	echo "must be run from inside a git repository" >&2
	exit 1
fi

if ! git diff --quiet || ! git diff --cached --quiet; then
	echo "working tree has uncommitted changes; commit or stash before tagging" >&2
	exit 1
fi

if ! git rev-parse --verify --quiet "$target_ref^{commit}" >/dev/null; then
	echo "unable to resolve target ref: $target_ref" >&2
	exit 1
fi

if [[ "$version_input" =~ ^ios-v ]]; then
	tag="$version_input"
else
	version="${version_input#v}"
	tag="ios-v$version"
fi

if [[ ! "$tag" =~ ^ios-v[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z-]+)*$ ]]; then
	echo "invalid tag format: $tag" >&2
	echo "expected ios-v<semver>, e.g. ios-v1.4.0" >&2
	exit 1
fi

if git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null; then
	echo "tag already exists locally: $tag" >&2
	exit 1
fi

if git ls-remote --exit-code --tags "$remote" "refs/tags/$tag" >/dev/null 2>&1; then
	echo "tag already exists on $remote: $tag" >&2
	exit 1
fi

tag_message="iOS release $tag"
git tag -a "$tag" "$target_ref" -m "$tag_message"

if [[ "$push_tag" == true ]]; then
	git push "$remote" "refs/tags/$tag"
	echo "pushed $tag to $remote"
else
	echo "created local tag $tag (not pushed)"
fi
