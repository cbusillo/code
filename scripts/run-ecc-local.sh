#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/run-ecc-local.sh [options]

Build and launch the local Every Code Companion macOS app from a deterministic
DerivedData path so you can verify you're on the latest build.

Options:
  --no-build   Skip build steps and launch existing app bundle.
  --open       Launch with `open` (default behavior).
  --foreground Launch app binary in foreground (replaces shell process).
  -h, --help   Show this help message.
EOF
}

build=true
launch_with_open=true

while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-build)
      build=false
      ;;
    --open)
      launch_with_open=true
      ;;
    --foreground)
      launch_with_open=false
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
  shift
done

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
derived_data_path="${ECC_DERIVED_DATA_PATH:-$repo_root/.code/local-mac-dd}"
configuration="${ECC_CONFIGURATION:-Debug}"
project_path="$repo_root/native/CodeNativeMac/CodeNativeMac.xcodeproj"
app_path="$derived_data_path/Build/Products/$configuration/EveryCodeCompanion.app"
binary_path="$app_path/Contents/MacOS/EveryCodeCompanion"

if [[ "$build" == true ]]; then
  echo "[run-ecc-local] building backend with ./build-fast.sh"
  (
    cd "$repo_root"
    ./build-fast.sh
  )

  echo "[run-ecc-local] building macOS app ($configuration)"
  xcodebuild \
    -project "$project_path" \
    -scheme CodeNativeMac \
    -configuration "$configuration" \
    -derivedDataPath "$derived_data_path" \
    build
fi

if [[ ! -x "$binary_path" ]]; then
  echo "error: app binary not found: $binary_path" >&2
  echo "hint: run without --no-build or verify ECC_DERIVED_DATA_PATH/ECC_CONFIGURATION" >&2
  exit 1
fi

echo "[run-ecc-local] stopping stale ECC/backend processes"
pkill -f 'EveryCodeCompanion.app/Contents/MacOS/EveryCodeCompanion' || true
pkill -f 'CodeBackend.app/Contents/MacOS/code web' || true

echo "[run-ecc-local] launching: $app_path"
if [[ "$launch_with_open" == true ]]; then
  open "$app_path"
  echo "[run-ecc-local] launched via open"
else
  exec "$binary_path"
fi
