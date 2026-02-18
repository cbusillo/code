#!/usr/bin/env bash

set -euo pipefail

if [ $# -ne 1 ]; then
	echo "usage: $0 <version>" >&2
	echo "example: $0 1.0.0" >&2
	exit 2
fi

version="$1"
tag="macos-v${version}"

if git rev-parse "$tag" >/dev/null 2>&1; then
	echo "tag already exists locally: $tag" >&2
	exit 1
fi

git tag "$tag"
git push origin "refs/tags/$tag"

echo "pushed $tag"
echo "watch workflow: scripts/wait-for-gh-run.sh --workflow 'Native macOS TestFlight' --branch $(git rev-parse --abbrev-ref HEAD)"
