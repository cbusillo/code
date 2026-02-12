#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

HOST="127.0.0.1"
PORT="4317"
WATCH_MODE="1"

while [[ $# -gt 0 ]]; do
	case "$1" in
	--host)
		HOST="$2"
		shift 2
		;;
	--port)
		PORT="$2"
		shift 2
		;;
	--no-watch)
		WATCH_MODE="0"
		shift
		;;
	*)
		echo "Unknown arg: $1" >&2
		echo "Usage: scripts/dev-native.sh [--host 127.0.0.1] [--port 4317] [--no-watch]" >&2
		exit 1
		;;
	esac
done

BACKEND_PID=""
APP_PID=""

start_backend() {
	echo "[dev-native] starting backend: code web --host $HOST --port $PORT"
	cargo run --manifest-path code-rs/Cargo.toml -p code-cli --bin code --profile dev-fast -- web --host "$HOST" --port "$PORT" &
	BACKEND_PID=$!
}

start_app() {
	echo "[dev-native] starting app: swift run CodeNativeApp"
	swift run --package-path native/CodeNative CodeNativeApp &
	APP_PID=$!
}

stop_processes() {
	for pid in "$BACKEND_PID" "$APP_PID"; do
		if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
			kill "$pid" 2>/dev/null || true
			wait "$pid" 2>/dev/null || true
		fi
	done

	BACKEND_PID=""
	APP_PID=""
}

cleanup() {
	echo "[dev-native] stopping"
	stop_processes
}

watch_snapshot() {
	find code-rs native/CodeNative \
		-type f \
		\( -name '*.rs' -o -name '*.toml' -o -name '*.swift' -o -name '*.md' -o -name '*.json' -o -name '*.mjs' -o -name '*.js' -o -name '*.css' -o -name '*.html' \) \
		! -path '*/target/*' \
		! -path '*/.build/*' \
		! -path '*/node_modules/*' \
		-print0 |
		xargs -0 stat -f '%m %N' |
		shasum -a 256 |
		awk '{print $1}'
}

trap cleanup INT TERM EXIT

start_backend
sleep 1
start_app

if [[ "$WATCH_MODE" == "0" ]]; then
	wait
	exit 0
fi

echo "[dev-native] watch mode enabled"
PREV_SNAPSHOT="$(watch_snapshot)"

while true; do
	sleep 1
	NEXT_SNAPSHOT="$(watch_snapshot)"
	if [[ "$NEXT_SNAPSHOT" != "$PREV_SNAPSHOT" ]]; then
		PREV_SNAPSHOT="$NEXT_SNAPSHOT"
		echo "[dev-native] changes detected, restarting backend + app"
		stop_processes
		start_backend
		sleep 1
		start_app
	fi
done
