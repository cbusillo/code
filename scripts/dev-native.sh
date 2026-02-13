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
APP_NAME="CodeNativeApp"
LOCK_FILE="$ROOT_DIR/.code/dev-native.lock"

acquire_lock() {
	mkdir -p "$(dirname "$LOCK_FILE")"
	if [[ -f "$LOCK_FILE" ]]; then
		local existing_pid
		existing_pid="$(cat "$LOCK_FILE" 2>/dev/null || true)"
		if [[ -n "$existing_pid" ]] && kill -0 "$existing_pid" 2>/dev/null; then
			local existing_cmd
			existing_cmd="$(ps -p "$existing_pid" -o command= 2>/dev/null || true)"
			if [[ "$existing_cmd" == *"scripts/dev-native.sh"* ]]; then
				echo "[dev-native] replacing existing dev runner (pid $existing_pid)"
				kill "$existing_pid" 2>/dev/null || true
				for _ in {1..20}; do
					if ! kill -0 "$existing_pid" 2>/dev/null; then
						break
					fi
					sleep 0.1
				done
				if kill -0 "$existing_pid" 2>/dev/null; then
					echo "[dev-native] force stopping stale runner (pid $existing_pid)"
					kill -9 "$existing_pid" 2>/dev/null || true
				fi
			else
				echo "[dev-native] lock held by pid $existing_pid ($existing_cmd)" >&2
				echo "[dev-native] remove .code/dev-native.lock if this is stale." >&2
				exit 1
			fi
		fi
		rm -f "$LOCK_FILE"
	fi

	echo "$$" >"$LOCK_FILE"
}

kill_existing_backend() {
	local port_pids
	port_pids="$(lsof -ti "tcp:$PORT" -sTCP:LISTEN 2>/dev/null || true)"
	if [[ -n "$port_pids" ]]; then
		for pid in $port_pids; do
			local cmd
			cmd="$(ps -p "$pid" -o command= 2>/dev/null || true)"
			if [[ "$cmd" == *"code web"* ]]; then
				echo "[dev-native] stopping existing backend on :$PORT (pid $pid)"
				kill "$pid" 2>/dev/null || true
			fi
		done
		sleep 0.2
	fi
}

start_backend() {
	kill_existing_backend
	echo "[dev-native] starting backend: code web --host $HOST --port $PORT"
	cargo run --manifest-path code-rs/Cargo.toml -p code-cli --bin code --profile dev-fast -- web --host "$HOST" --port "$PORT" &
	BACKEND_PID=$!
}

kill_existing_native_app() {
	local pids
	pids="$(pgrep -af "$APP_NAME" 2>/dev/null | awk '!/CodeNativeAutomation/ {print $1}' || true)"
	if [[ -n "$pids" ]]; then
		echo "[dev-native] stopping existing $APP_NAME process(es): $pids"
		for pid in $pids; do
			kill "$pid" 2>/dev/null || true
		done
		sleep 0.2
	fi
}

activate_app() {
	osascript -e "tell application \"$APP_NAME\" to activate" >/dev/null 2>&1 || true
}

start_app() {
	kill_existing_native_app
	echo "[dev-native] starting app: swift run CodeNativeApp"
	swift run --package-path native/CodeNative CodeNativeApp &
	APP_PID=$!
	sleep 0.3
	activate_app
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
	if [[ -f "$LOCK_FILE" ]] && [[ "$(cat "$LOCK_FILE" 2>/dev/null || true)" == "$$" ]]; then
		rm -f "$LOCK_FILE"
	fi
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

acquire_lock

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
