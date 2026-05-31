#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'USAGE'
Usage: scripts/release/determine-release-intent.sh [--github-output PATH] [--github-step-summary PATH]

Determines whether the committed package version should publish a GitHub
Release. A release is intended when codex-cli/package.json names a version whose
v<version> tag does not exist yet.
USAGE
}

github_output="${GITHUB_OUTPUT:-}"
github_step_summary="${GITHUB_STEP_SUMMARY:-}"

while [[ $# -gt 0 ]]; do
	case "$1" in
	--github-output)
		github_output="${2:-}"
		shift 2
		;;
	--github-output=*)
		github_output="${1#--github-output=}"
		shift
		;;
	--github-step-summary)
		github_step_summary="${2:-}"
		shift 2
		;;
	--github-step-summary=*)
		github_step_summary="${1#--github-step-summary=}"
		shift
		;;
	-h | --help)
		usage
		exit 0
		;;
	*)
		echo "unknown argument: $1" >&2
		usage >&2
		exit 2
		;;
	esac
done

repo_root="$(git rev-parse --show-toplevel)"
current_version="$(node -p "require('${repo_root}/codex-cli/package.json').version")"
new_version="$current_version"

if git rev-parse "v${new_version}" >/dev/null 2>&1; then
	release_intent=false
else
	release_intent=true
fi

if [[ -n "$github_output" ]]; then
	{
		echo "version=${new_version}"
		echo "release_intent=${release_intent}"
	} >>"$github_output"
fi

if [[ -n "$github_step_summary" ]]; then
	{
		echo "### Release intent"
		echo
		if [[ "$release_intent" == true ]]; then
			echo "Preparing to publish v${new_version}."
		else
			echo "No release intended: v${new_version} already exists."
		fi
	} >>"$github_step_summary"
fi

printf 'version=%s\n' "$new_version"
printf 'release_intent=%s\n' "$release_intent"
