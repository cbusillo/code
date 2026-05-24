#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat >&2 <<'USAGE'
Usage: scripts/release/generate-update-manifest.sh \
  --version VERSION \
  --commit SHA \
  --repository OWNER/REPO \
  --assets-dir DIR \
  --output FILE

Generates the GitHub Release update manifest used by internal dogfood builds.
USAGE
}

version=""
commit=""
repository=""
assets_dir=""
output=""
channel="stable"
published_at="${PUBLISHED_AT:-}"

while [[ $# -gt 0 ]]; do
	case "$1" in
	--version)
		version="${2:-}"
		shift 2
		;;
	--commit)
		commit="${2:-}"
		shift 2
		;;
	--repository)
		repository="${2:-}"
		shift 2
		;;
	--assets-dir)
		assets_dir="${2:-}"
		shift 2
		;;
	--output)
		output="${2:-}"
		shift 2
		;;
	--channel)
		channel="${2:-}"
		shift 2
		;;
	--published-at)
		published_at="${2:-}"
		shift 2
		;;
	-h | --help)
		usage
		exit 0
		;;
	*)
		echo "unknown argument: $1" >&2
		usage
		exit 2
		;;
	esac
done

if [[ -z "$version" || -z "$commit" || -z "$repository" || -z "$assets_dir" || -z "$output" ]]; then
	usage
	exit 2
fi

if [[ ! -d "$assets_dir" ]]; then
	echo "assets dir does not exist: $assets_dir" >&2
	exit 1
fi

if [[ -z "$published_at" ]]; then
	published_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
fi

tag="v${version#v}"
download_base="https://github.com/${repository}/releases/download/${tag}"

asset_for_target() {
	local target="$1"
	case "$target" in
	x86_64-pc-windows-msvc)
		printf '%s\n' "code-${target}.exe.zip"
		;;
	*)
		printf '%s\n' "code-${target}.tar.gz"
		;;
	esac
}

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

cat >"$tmp" <<JSON
{
  "schema_version": 1,
  "version": "$tag",
  "channel": "$channel",
  "commit": "$commit",
  "published_at": "$published_at",
  "platforms": {
JSON

targets=(
	aarch64-apple-darwin
	x86_64-apple-darwin
	aarch64-unknown-linux-musl
	x86_64-unknown-linux-musl
	x86_64-pc-windows-msvc
)

first=true
for target in "${targets[@]}"; do
	asset="$(asset_for_target "$target")"
	path="$assets_dir/$asset"
	if [[ ! -f "$path" ]]; then
		echo "missing required release asset for $target: $asset" >&2
		exit 1
	fi
	sha256="$(shasum -a 256 "$path" | awk '{print $1}')"
	size="$(wc -c <"$path" | tr -d '[:space:]')"
	url="${download_base}/${asset}"

	if [[ "$first" == true ]]; then
		first=false
	else
		printf ',\n' >>"$tmp"
	fi

	cat >>"$tmp" <<JSON
    "$target": {
      "asset": "$asset",
      "url": "$url",
      "sha256": "$sha256",
      "size": $size
    }
JSON
done

cat >>"$tmp" <<JSON

  }
}
JSON

mkdir -p "$(dirname "$output")"
jq -e . "$tmp" >"$output"
echo "Wrote update manifest: $output" >&2
