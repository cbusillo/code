#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
REPO_ROOT="${SCRIPT_DIR}/.."

expected_version=""
while [ "$#" -gt 0 ]; do
	case "$1" in
	--version)
		shift
		expected_version="${1:-}"
		;;
	--version=*)
		expected_version="${1#--version=}"
		;;
	-h | --help)
		echo "Usage: scripts/check-release-notes-version.sh [--version X.Y.Z]"
		exit 0
		;;
	*)
		echo "unknown argument: $1" >&2
		exit 2
		;;
	esac
	shift || true
done

notes_file="${REPO_ROOT}/docs/release-notes/RELEASE_NOTES.md"
version_file="${REPO_ROOT}/VERSION"

if [ ! -f "$notes_file" ]; then
	echo "release notes file missing: $notes_file" >&2
	exit 1
fi

if [ ! -f "$version_file" ]; then
	echo "VERSION file missing: $version_file" >&2
	exit 1
fi

package_version=$(tr -d '[:space:]' <"$version_file")

if ! [[ "$package_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
	echo "Failed to read semver from $version_file" >&2
	exit 1
fi

if [ -n "$expected_version" ] && [ "$package_version" != "$expected_version" ]; then
	echo "release version mismatch" >&2
	echo "  expected: $expected_version" >&2
	echo "  actual:   $package_version" >&2
	echo "Run 'just local-release-notes' before publishing this release." >&2
	exit 1
fi

expected_header="## Every Code v${package_version}"
actual_header=$(grep -m1 '^## Every Code v' "$notes_file" || true)

if [ "$actual_header" != "$expected_header" ]; then
	echo "release notes header mismatch" >&2
	echo "  expected: $expected_header" >&2
	echo "  actual:   ${actual_header:-<none>}" >&2
	echo "Run 'just local-release-notes' before publishing this release." >&2
	exit 1
fi

if grep -qF "See CHANGELOG.md for details." "$notes_file"; then
	echo "release notes still contain the fallback stub" >&2
	echo "Run 'just local-release-notes' to generate reviewed local notes." >&2
	exit 1
fi

if ! grep -qF "## [${package_version}]" "${REPO_ROOT}/CHANGELOG.md"; then
	echo "CHANGELOG.md is missing release entry ## [${package_version}]" >&2
	echo "Run 'just local-release-notes' before publishing this release." >&2
	exit 1
fi

if ! grep -qF "### Changes" "$notes_file"; then
	echo "release notes are missing the required ### Changes section" >&2
	echo "Run 'just local-release-notes' before publishing this release." >&2
	exit 1
fi

exit 0
