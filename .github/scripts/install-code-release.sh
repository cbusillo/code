#!/usr/bin/env bash
set -euo pipefail

dest_dir="${1:-.github/auto/code-bin}"
repo="${GITHUB_REPOSITORY:-cbusillo/code}"

case "$(uname -s)-$(uname -m)" in
Linux-x86_64) target="x86_64-unknown-linux-musl" ;;
Linux-aarch64 | Linux-arm64) target="aarch64-unknown-linux-musl" ;;
Darwin-x86_64) target="x86_64-apple-darwin" ;;
Darwin-arm64) target="aarch64-apple-darwin" ;;
*)
	echo "unsupported platform for Code release bootstrap: $(uname -s)-$(uname -m)" >&2
	exit 1
	;;
esac

asset="code-${target}.tar.gz"
url="https://github.com/${repo}/releases/latest/download/${asset}"

mkdir -p "$dest_dir"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

curl_args=(-fL --retry 3 --retry-delay 2)
if [ -n "${GITHUB_TOKEN:-}" ]; then
	curl_args+=(-H "Authorization: Bearer ${GITHUB_TOKEN}")
fi

curl "${curl_args[@]}" "$url" -o "$tmp_dir/$asset"
tar -xzf "$tmp_dir/$asset" -C "$tmp_dir"
install -m 0755 "$tmp_dir/code-${target}" "$dest_dir/code"
"$dest_dir/code" --version
