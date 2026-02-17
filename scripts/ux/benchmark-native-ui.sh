#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
APP_NAME="CodeNativeApp"
SCENARIO_DIR="$ROOT_DIR/native/CodeNative/automation/benchmarks"
FIXTURE_DIR="$SCENARIO_DIR/fixtures"
CURRENT_DIR="$ROOT_DIR/.code/ux-bench/native-ui/current"
BASELINE_DIR="$ROOT_DIR/docs/reference/native-ui/baseline"

SCENARIOS=(
	"transcript-idle"
	"transcript-long"
	"activity-heavy"
	"approval-pending"
	"request-user-input"
	"command-launcher"
	"context-mention"
	"git-recovery"
	"ide-integration"
	"disconnected-state"
	"settings-shell"
)

mkdir -p "$CURRENT_DIR"
mkdir -p "$BASELINE_DIR"

started_app_pid=""

cleanup() {
	if [[ -n "$started_app_pid" ]]; then
		kill "$started_app_pid" >/dev/null 2>&1 || true
		wait "$started_app_pid" >/dev/null 2>&1 || true
		started_app_pid=""
	fi
}
trap cleanup EXIT

launch_app_with_fixture() {
	local scenario="$1"
	local fixture_path="$2"

	pkill -x "$APP_NAME" >/dev/null 2>&1 || true
	cleanup

	echo "[benchmark-native-ui] launching $APP_NAME with fixture $scenario"
	(
		cd "$ROOT_DIR"
		CODE_NATIVE_BENCHMARK_FIXTURE="$fixture_path" swift run --package-path native/CodeNative "$APP_NAME"
	) >/tmp/benchmark-native-ui-app-"$scenario".log 2>&1 &
	started_app_pid="$!"
	sleep 1.6
}

for scenario in "${SCENARIOS[@]}"; do
	scenario_path="$SCENARIO_DIR/$scenario.json"
	fixture_path="$FIXTURE_DIR/$scenario.json"
	if [[ ! -f "$fixture_path" ]]; then
		echo "[benchmark-native-ui] missing fixture: $fixture_path" >&2
		exit 1
	fi

	launch_app_with_fixture "$scenario" "$fixture_path"
	echo "[benchmark-native-ui] running $scenario"
	(
		cd "$ROOT_DIR"
		swift run --package-path native/CodeNative CodeNativeAutomation --scenario "$scenario_path"
	)
	cleanup
done

if [[ "${UPDATE_BASELINE:-0}" == "1" ]]; then
	echo "[benchmark-native-ui] updating baseline artifacts"
	cp "$CURRENT_DIR"/*.png "$BASELINE_DIR"/
fi

echo "[benchmark-native-ui] current artifacts:"
ls -1 "$CURRENT_DIR"/*.png
