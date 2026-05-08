#!/usr/bin/env bash

set -euo pipefail

usage() {
	cat <<'EOF'
Usage: every-code-worker [--json] [start|status|stop|run|run-once|logs]

Controls the local Every Code worker that claims Launchplane work requests and
opens visible tmux/Code sessions on this Mac.

Environment overrides:
  LAUNCHPLANE_EVERY_CODE_WORKTREE   Launchplane checkout to run from
  LAUNCHPLANE_EVERY_CODE_SERVICE    Launchplane service URL
  LAUNCHPLANE_EVERY_CODE_WORKSPACE  Directory containing repo checkouts
  LAUNCHPLANE_EVERY_CODE_STATE_DIR  Local worker state directory
  LAUNCHPLANE_EVERY_CODE_TOKEN_ITEM macOS Keychain item name for worker token
EOF
}

json_output=0
if [ "${1:-}" = "--json" ]; then
	json_output=1
	shift
fi

command_name="${1:-status}"
if [ "$command_name" = "help" ] || [ "$command_name" = "--help" ]; then
	usage
	exit 0
fi
if [ "$#" -gt 0 ]; then
	shift
fi

remaining_args=()
for arg in "$@"; do
	if [ "$arg" = "--json" ]; then
		json_output=1
	else
		remaining_args+=("$arg")
	fi
done
set -- "${remaining_args[@]}"

service_url="${LAUNCHPLANE_EVERY_CODE_SERVICE:-https://launchplane.shinycomputers.com}"
workspace_root="${LAUNCHPLANE_EVERY_CODE_WORKSPACE:-/Users/cbusillo/Developer}"
launchplane_worktree="${LAUNCHPLANE_EVERY_CODE_WORKTREE:-$workspace_root/launchplane}"
state_dir="${LAUNCHPLANE_EVERY_CODE_STATE_DIR:-$HOME/.local/state/launchplane/every-code}"
token_item="${LAUNCHPLANE_EVERY_CODE_TOKEN_ITEM:-launchplane-every-code-worker-token}"
worker_host="${LAUNCHPLANE_EVERY_CODE_HOST:-Chris-Studio}"
launchd_label="${LAUNCHPLANE_EVERY_CODE_LAUNCHD_LABEL:-com.cbusillo.every-code-worker}"
launchd_domain="gui/$(id -u)"

require_worker_token() {
	local worker_token
	worker_token="$(security find-generic-password -a "$USER" -s "$token_item" -w 2>/dev/null || true)"
	if [ -z "$worker_token" ]; then
		echo "Every Code worker token not found in Keychain item: $token_item" >&2
		exit 1
	fi
	export LAUNCHPLANE_EVERY_CODE_WORKER_TOKEN="$worker_token"
}

run_launchplane() {
	if [ ! -d "$launchplane_worktree" ]; then
		echo "Launchplane worktree not found: $launchplane_worktree" >&2
		exit 1
	fi
	uv --directory "$launchplane_worktree" run launchplane every-code "$@"
}

launchd_status_payload() {
	local service_output
	if ! service_output="$(launchctl print "$launchd_domain/$launchd_label" 2>/dev/null)"; then
		return 1
	fi
	SERVICE_OUTPUT="$service_output" python3 - "$launchd_label" <<'PY'
import json
import os
import re
import sys

label = sys.argv[1]
output = os.environ["SERVICE_OUTPUT"]
state_match = re.search(r"\n\s*state = ([^\n]+)", output)
pid_match = re.search(r"\n\s*pid = (\d+)", output)
state = state_match.group(1).strip() if state_match else "unknown"
pid = int(pid_match.group(1)) if pid_match else None
running = state == "running" and pid is not None
print(json.dumps({
    "detail": (
        f"Every Code worker launchd service {label} is running."
        if running
        else f"Every Code worker launchd service {label} is {state}."
    ),
    "launchd_label": label,
    "launchd_state": state,
    "pid": pid,
    "running": running,
}, indent=2, sort_keys=True))
PY
}

emit_result() {
	local payload="$1"
	if [ "$json_output" -eq 1 ]; then
		printf '%s\n' "$payload"
		return
	fi
	PAYLOAD="$payload" python3 - "$command_name" <<'PY'
import json
import os
import sys

command = sys.argv[1]
payload = json.loads(os.environ["PAYLOAD"])


def text(name: str, default: str = "") -> str:
    value = payload.get(name, default)
    return str(value) if value is not None else default


def line(label: str, value: str) -> None:
    if value:
        print(f"{label}: {value}")


if command == "status":
    print(text("detail", "Every Code worker status is unavailable."))
    if payload.get("pid") is not None:
        line("pid", text("pid"))
    line("log", text("log_file"))
elif command == "start":
    status = text("status", "unknown")
    if status == "already_running":
        print("Every Code worker is already running.")
    elif status == "started":
        print("Every Code worker started.")
    else:
        print(f"Every Code worker start result: {status}")
    if payload.get("pid") is not None:
        line("pid", text("pid"))
    line("log", text("log_file"))
elif command == "stop":
    print(text("detail", f"Every Code worker stop result: {text('status', 'unknown')}"))
elif command == "run-once":
    print(text("detail", f"Every Code run-once result: {text('status', 'unknown')}"))
    line("request", text("request_id"))
    line("repo", text("repository"))
    if payload.get("issue_number"):
        line("issue", text("issue_number"))
    line("session", text("session_name"))
    line("checkout", text("checkout_root"))
else:
    print(json.dumps(payload, indent=2, sort_keys=True))
PY
}

case "$command_name" in
start)
	require_worker_token
	payload="$(run_launchplane start \
		--service-url "$service_url" \
		--workspace-root "$workspace_root" \
		--state-dir "$state_dir" \
		"$@")"
	emit_result "$payload"
	;;
status)
	payload="$(launchd_status_payload || run_launchplane status --state-dir "$state_dir" "$@")"
	emit_result "$payload"
	;;
stop)
	payload="$(run_launchplane stop --state-dir "$state_dir" "$@")"
	emit_result "$payload"
	;;
run-once)
	require_worker_token
	payload="$(run_launchplane run-once \
		--service-url "$service_url" \
		--workspace-root "$workspace_root" \
		--state-dir "$state_dir" \
		"$@")"
	emit_result "$payload"
	;;
run)
	require_worker_token
	exec uv --directory "$launchplane_worktree" run launchplane every-code run \
		--service-url "$service_url" \
		--workspace-root "$workspace_root" \
		--state-dir "$state_dir" \
		--host "$worker_host" \
		"$@"
	;;
logs)
	log_file="$state_dir/every-code-worker/worker.log"
	if [ ! -f "$log_file" ]; then
		echo "Every Code worker log not found: $log_file" >&2
		exit 1
	fi
	tail -f "$log_file"
	;;
*)
	echo "Unknown Every Code worker command: $command_name" >&2
	usage >&2
	exit 2
	;;
esac
