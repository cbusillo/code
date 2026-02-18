#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CURRENT_DIR="$ROOT_DIR/.code/ux-bench/native-ui/current"
BASELINE_DIR="$ROOT_DIR/docs/reference/native-ui/baseline"

REQUIRED_SCENARIOS=(
	"transcript-idle"
	"transcript-long"
	"activity-heavy"
	"approval-pending"
	"disconnected-state"
	"settings-shell"
)

missing=0

check_artifact() {
	local label="$1"
	local path="$2"
	if [[ ! -f "$path" ]]; then
		echo "[verify-native-benchmark-baseline] missing $label: $path" >&2
		missing=1
	fi
}

for scenario in "${REQUIRED_SCENARIOS[@]}"; do
	check_artifact "current" "$CURRENT_DIR/$scenario.png"
	check_artifact "baseline" "$BASELINE_DIR/$scenario.png"
done

if [[ "$missing" -ne 0 ]]; then
	exit 1
fi

echo "[verify-native-benchmark-baseline] verified required baseline artifacts:"
for scenario in "${REQUIRED_SCENARIOS[@]}"; do
	current="$CURRENT_DIR/$scenario.png"
	baseline="$BASELINE_DIR/$scenario.png"
	current_sha="$(shasum -a 256 "$current" | awk '{print $1}')"
	baseline_sha="$(shasum -a 256 "$baseline" | awk '{print $1}')"
	echo "  $scenario current=$current_sha baseline=$baseline_sha"
done
