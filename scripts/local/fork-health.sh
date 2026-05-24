#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

current_branch="$(git symbolic-ref --quiet --short HEAD || echo HEAD)"
upstream_ref="${UPSTREAM_REF:-upstream/main}"
product_branch="${PRODUCT_BRANCH:-$(git symbolic-ref --quiet --short refs/remotes/origin/HEAD 2>/dev/null | sed 's#^origin/##')}"
product_branch="${product_branch:-local/cbusillo-overlay}"
latest_product_tag="$(git tag --list 'every-code-v*' 'overlay-v*' --sort=-v:refname | head -n 1 || true)"
latest_upstream_tag="$(git tag --list 'v*' --sort=-v:refname | head -n 1 || true)"
package_version="$(node -p "require('$repo_root/codex-cli/package.json').version" 2>/dev/null || true)"

echo "Every Code health"
echo "================="
echo "branch:              $current_branch"
echo "product branch:      $product_branch"
echo "upstream ref:        $upstream_ref"
echo "head:                $(git rev-parse --short HEAD)"
echo "origin product:      $(git rev-parse --short "origin/$product_branch" 2>/dev/null || echo unknown)"
echo "upstream head:       $(git rev-parse --short "$upstream_ref" 2>/dev/null || echo unknown)"
echo "package version:     ${package_version:-unknown}"
echo "latest upstream tag: ${latest_upstream_tag:-none}"
echo "latest product tag:  ${latest_product_tag:-none}"
echo

echo "status:"
git status --short --branch
echo

if git rev-parse --verify --quiet "$upstream_ref" >/dev/null; then
	echo "fork delta vs $upstream_ref:"
	git diff --shortstat "$upstream_ref"..HEAD -- code-rs scripts/local .github/workflows/binary-release.yml docs/local-overlay.md || true
	echo
	echo "hotspot files:"
	git diff --name-only "$upstream_ref"..HEAD -- \
		code-rs/tui/src/chatwidget.rs \
		code-rs/core/src/patch_harness.rs \
		code-rs/core/src/config.rs \
		code-rs/core/src/model_family.rs \
		code-rs/common/src/model_presets.rs \
		.github/workflows/binary-release.yml \
		scripts/local \
		docs/local-overlay.md || true
else
	echo "warning: upstream ref '$upstream_ref' is not available" >&2
fi
