#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
APP_NAME="CodeNativeApp"
SCENARIO_DIR="$ROOT_DIR/native/CodeNative/automation/benchmarks"
CURRENT_DIR="$ROOT_DIR/.code/ux-bench/native-ui/current"
BASELINE_DIR="$ROOT_DIR/docs/reference/native-ui/baseline"

SCENARIOS=(
	"transcript-idle"
	"transcript-long"
	"activity-heavy"
	"approval-pending"
	"disconnected-state"
	"settings-shell"
)

mkdir -p "$CURRENT_DIR"
mkdir -p "$BASELINE_DIR"

started_app_pid=""
if ! pgrep -x "$APP_NAME" >/dev/null 2>&1; then
	echo "[benchmark-native-ui] launching $APP_NAME"
	(
		cd "$ROOT_DIR"
		swift run --package-path native/CodeNative "$APP_NAME"
	) >/tmp/benchmark-native-ui-app.log 2>&1 &
	started_app_pid="$!"
	sleep 1.4
fi

cleanup() {
	if [[ -n "$started_app_pid" ]]; then
		kill "$started_app_pid" >/dev/null 2>&1 || true
	fi
}
trap cleanup EXIT

for scenario in "${SCENARIOS[@]}"; do
	scenario_path="$SCENARIO_DIR/$scenario.json"
	echo "[benchmark-native-ui] running $scenario"
	(
		cd "$ROOT_DIR"
		swift run --package-path native/CodeNative CodeNativeAutomation --scenario "$scenario_path"
	)
done

if [[ "${UPDATE_BASELINE:-0}" == "1" ]]; then
	echo "[benchmark-native-ui] updating baseline artifacts"
	cp "$CURRENT_DIR"/*.png "$BASELINE_DIR"/
fi

echo "[benchmark-native-ui] current artifacts:"
ls -1 "$CURRENT_DIR"/*.png
