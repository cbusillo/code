#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
code_rs_root="$repo_root/code-rs"
release_bin="$code_rs_root/target/release/code"

echo "Building release binary from $code_rs_root"
cargo build --manifest-path "$code_rs_root/Cargo.toml" -p code-cli --release

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
