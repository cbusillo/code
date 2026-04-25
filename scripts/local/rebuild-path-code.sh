#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
code_rs_root="$repo_root/code-rs"
release_bin="$code_rs_root/target/release/code"

resolve_code_version() {
	{
		git -C "$repo_root" tag --list 'rust-v[0-9]*' | sed 's/^rust-v//'
		git ls-remote --tags --refs https://github.com/openai/codex.git 'refs/tags/rust-v[0-9]*' 2>/dev/null |
			awk '{print $2}' |
			sed 's#^refs/tags/rust-v##'
	} |
		grep -E '^[0-9]+\.[0-9]+\.[0-9]+([-.+][A-Za-z0-9.]+)?$' |
		sort -V |
		tail -n 1
}

code_version="${CODE_VERSION:-$(resolve_code_version || true)}"

echo "Building release binary from $code_rs_root"
if [[ -n "$code_version" ]]; then
	echo "Embedding CODE_VERSION=$code_version"
	CODE_VERSION="$code_version" cargo build --manifest-path "$code_rs_root/Cargo.toml" -p code-cli --release
else
	echo "warning: could not resolve CODE_VERSION from rust-v tags; building without override" >&2
	cargo build --manifest-path "$code_rs_root/Cargo.toml" -p code-cli --release
fi

path_code="$(command -v code || true)"
if [[ -z "$path_code" ]]; then
	echo "error: could not find 'code' on PATH" >&2
	exit 1
fi

resolved_path="$(
	python3 - "$path_code" <<'PY'
import os, sys
print(os.path.realpath(sys.argv[1]))
PY
)"

echo
echo "PATH entry:   $path_code"
echo "Resolves to:  $resolved_path"
echo "Release bin:  $release_bin"

if [[ "$resolved_path" != "$release_bin" ]]; then
	echo "warning: PATH does not resolve to the freshly built binary" >&2
fi

stat -f 'Updated: %Sm %N' "$release_bin"
"$path_code" --version
