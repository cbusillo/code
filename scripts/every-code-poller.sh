#!/usr/bin/env bash
set -euo pipefail

derive_owner() {
	if [[ -n "${EVERY_CODE_OWNER:-}" ]]; then
		printf '%s' "$EVERY_CODE_OWNER"
		return 0
	fi
	if command -v gh >/dev/null 2>&1; then
		local user
		user="$(gh api user --jq '.login' 2>/dev/null || true)"
		if [[ -n "$user" ]]; then
			printf '%s' "$user"
			return 0
		fi
	fi
	printf '%s' ""
}

derive_checkout_root() {
	if [[ -n "${EVERY_CODE_CHECKOUT_ROOT:-}" ]]; then
		printf '%s' "$EVERY_CODE_CHECKOUT_ROOT"
		return 0
	fi
	local repo_root parent
	repo_root="$(git rev-parse --show-toplevel 2>/dev/null || true)"
	if [[ -n "$repo_root" ]]; then
		parent="$(cd "$(dirname "$repo_root")" && pwd -P)"
		printf '%s' "$parent"
		return 0
	fi
	pwd -P
}

SESSION_NAME="${EVERY_CODE_SESSION_NAME:-every-code}"
TOPIC="${EVERY_CODE_TOPIC:-every-code}"
READY_LABEL="${EVERY_CODE_READY_LABEL:-every-code}"
WORKING_LABEL="${EVERY_CODE_WORKING_LABEL:-every-code/working}"
DONE_LABEL="${EVERY_CODE_DONE_LABEL:-every-code/done}"
BLOCKED_LABEL="${EVERY_CODE_BLOCKED_LABEL:-every-code/blocked}"
SCAN_OWNER="$(derive_owner)"
CHECKOUT_ROOT="$(derive_checkout_root)"
POLL_INTERVAL_SECONDS="${EVERY_CODE_POLL_INTERVAL_SECONDS:-60}"
MAX_ISSUES_PER_REPO="${EVERY_CODE_MAX_ISSUES_PER_REPO:-5}"
CODE_BIN="${EVERY_CODE_CODE_BIN:-code}"
SANDBOX_MODE="${EVERY_CODE_SANDBOX:-workspace-write}"
MAX_SECONDS="${EVERY_CODE_MAX_SECONDS:-7200}"
WINDOW_PREFIX="ec"
LOCK_DIR="${EVERY_CODE_LOCK_DIR:-${TMPDIR:-/tmp}/every-code-poller-${SCAN_OWNER:-unknown}.lock}"
LOCK_STALE_SECONDS="${EVERY_CODE_LOCK_STALE_SECONDS:-900}"
TRUSTED_USERS_CSV="${EVERY_CODE_TRUSTED_USERS:-}"
TRUSTED_PERMISSIONS_CSV="${EVERY_CODE_TRUSTED_PERMISSIONS:-admin,maintain,write}"

usage() {
	cat <<EOF
Usage: $(basename "$0") <command>

Commands:
  start       Start a visible tmux session and run the poller in it.
  run         Run the poller loop in the current terminal.
  once        Scan once and launch jobs for ready issues.
  job REPO N  Run one issue job; normally launched by the poller.
  attach      Attach to the tmux session.
  stop        Stop the tmux session.

Environment:
  EVERY_CODE_OWNER=$SCAN_OWNER
  EVERY_CODE_TOPIC=$TOPIC
  EVERY_CODE_READY_LABEL=$READY_LABEL
  EVERY_CODE_CHECKOUT_ROOT=$CHECKOUT_ROOT
  EVERY_CODE_POLL_INTERVAL_SECONDS=$POLL_INTERVAL_SECONDS
EOF
}

require_tool() {
	if ! command -v "$1" >/dev/null 2>&1; then
		echo "Missing required tool: $1" >&2
		exit 1
	fi
}

require_owner() {
	if [[ -z "$SCAN_OWNER" ]]; then
		echo "Could not determine GitHub owner. Set EVERY_CODE_OWNER." >&2
		exit 1
	fi
}

lock_is_stale() {
	[[ -d "$LOCK_DIR" ]] || return 1
	local now modified age stamp_file
	stamp_file="$LOCK_DIR/started_at"
	now="$(date +%s)"
	if [[ -f "$stamp_file" ]]; then
		modified="$(cat "$stamp_file" 2>/dev/null || printf '')"
	fi
	if [[ -z "${modified:-}" ]]; then
		if ! modified="$(stat -f %m "$LOCK_DIR" 2>/dev/null)"; then
			modified="$(stat -c %Y "$LOCK_DIR" 2>/dev/null || printf '')"
		fi
	fi
	if [[ -z "${modified:-}" ]]; then
		return 1
	fi
	age=$((now - modified))
	[[ "$age" -gt "$LOCK_STALE_SECONDS" ]]
}

release_scan_lock() {
	rm -f "$LOCK_DIR/started_at" 2>/dev/null || true
	rmdir "$LOCK_DIR" 2>/dev/null || true
}

refresh_scan_lock() {
	[[ -d "$LOCK_DIR" ]] || return 0
	date +%s >"$LOCK_DIR/started_at" 2>/dev/null || true
}

with_scan_lock() {
	if ! mkdir "$LOCK_DIR" 2>/dev/null; then
		if lock_is_stale; then
			echo "Removing stale Every Code poller lock: $LOCK_DIR"
			release_scan_lock
			if ! mkdir "$LOCK_DIR" 2>/dev/null; then
				echo "Another Every Code poller scan is already running; skipping this pass."
				return 0
			fi
		else
			echo "Another Every Code poller scan is already running; skipping this pass."
			return 0
		fi
	fi
	local status
	date +%s >"$LOCK_DIR/started_at" 2>/dev/null || true
	trap release_scan_lock EXIT INT TERM
	scan_once_unlocked
	status=$?
	release_scan_lock
	trap - EXIT INT TERM
	return "$status"
}

script_path() {
	cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P
}

repo_name() {
	local repo="$1"
	printf '%s' "${repo##*/}"
}

checkout_dir_for_repo() {
	local repo="$1"
	local nested flat
	nested="$CHECKOUT_ROOT/$repo"
	flat="$CHECKOUT_ROOT/$(repo_name "$repo")"
	if git -C "$nested" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
		printf '%s' "$nested"
	else
		printf '%s' "$flat"
	fi
}

legacy_window_name_for_issue() {
	local repo="$1"
	local issue="$2"
	local name
	name="$(repo_name "$repo" | tr '[:upper:]' '[:lower:]' | tr -c '[:alnum:]_-' '-')"
	printf '%s-%s-%s' "$WINDOW_PREFIX" "$name" "$issue" | cut -c1-40
}

window_name_for_issue() {
	local repo="$1"
	local issue="$2"
	local name hash
	name="$(printf '%s' "$repo" | tr '[:upper:]/' '[:lower:]-' | tr -c '[:alnum:]_-' '-')"
	hash="$(printf '%s#%s' "$repo" "$issue" | cksum | awk '{print $1}')"
	printf '%s-%s-%s-%s' "$WINDOW_PREFIX" "$(printf '%s' "$name" | cut -c1-24)" "$issue" "$hash" | cut -c1-48
}

ensure_label() {
	local repo="$1"
	local label="$2"
	local color="$3"
	local description="$4"
	gh label create "$label" \
		--repo "$repo" \
		--color "$color" \
		--description "$description" \
		--force >/dev/null
}

ensure_state_labels() {
	local repo="$1"
	ensure_label "$repo" "$READY_LABEL" "5319E7" \
		"Local Every Code automation may pick up this issue and open a PR"
	ensure_label "$repo" "$WORKING_LABEL" "FBCA04" \
		"Local Every Code automation is working on this issue"
	ensure_label "$repo" "$DONE_LABEL" "0E8A16" \
		"Local Every Code automation opened or updated a PR for this issue"
	ensure_label "$repo" "$BLOCKED_LABEL" "B60205" \
		"Local Every Code automation tried this issue and needs help"
}

list_repos() {
	gh repo list "$SCAN_OWNER" \
		--limit 1000 \
		--json nameWithOwner,isArchived,repositoryTopics \
		--jq ".[] | select(.isArchived == false) | select([(.repositoryTopics // [])[].name] | index(\"$TOPIC\")) | .nameWithOwner"
}

list_ready_issues() {
	local repo="$1"
	gh issue list \
		--repo "$repo" \
		--state open \
		--label "$READY_LABEL" \
		--limit "$MAX_ISSUES_PER_REPO" \
		--json number,title,labels,url \
		--jq ".[] | select([.labels[].name] | index(\"$WORKING_LABEL\") | not) | select([.labels[].name] | index(\"$DONE_LABEL\") | not) | select([.labels[].name] | index(\"$BLOCKED_LABEL\") | not) | [.number, .title, .url] | @tsv"
}

issue_labels_csv() {
	local repo="$1"
	local issue="$2"
	gh issue view "$issue" --repo "$repo" --json labels --jq '[.labels[].name] | join(",")'
}

labels_include() {
	local csv="$1"
	local label="$2"
	[[ ",$csv," == *",$label,"* ]]
}

csv_includes() {
	local csv="$1"
	local value="$2"
	[[ ",${csv// /}," == *",$value,"* ]]
}

ready_label_actor() {
	local repo="$1"
	local issue="$2"
	gh api "repos/$repo/issues/$issue/events" \
		--paginate \
		--jq ".[] | select(.event == \"labeled\" and .label.name == \"$READY_LABEL\") | .actor.login" |
		tail -n 1
}

user_permission() {
	local repo="$1"
	local user="$2"
	gh api "repos/$repo/collaborators/$user/permission" --jq '.permission // "none"'
}

trusted_label_actor() {
	local repo="$1"
	local issue="$2"
	local actor permission

	actor="$(ready_label_actor "$repo" "$issue")"
	if [[ -z "$actor" ]]; then
		echo "Skipping $repo#$issue: could not determine who applied label '$READY_LABEL'."
		return 1
	fi

	if csv_includes "$TRUSTED_USERS_CSV" "$actor"; then
		return 0
	fi

	if ! permission="$(user_permission "$repo" "$actor" 2>&1)"; then
		echo "Could not verify repo permission for @$actor on $repo: $permission"
		return 2
	fi
	if csv_includes "$TRUSTED_PERMISSIONS_CSV" "$permission"; then
		return 0
	fi

	echo "Skipping $repo#$issue: @$actor applied '$READY_LABEL' but has repo permission '$permission'."
	return 1
}

issue_has_open_pr() {
	local repo="$1"
	local issue="$2"
	local count
	if ! count="$(gh search prs "repo:$repo is:pr is:open $issue" --json number --jq 'length')"; then
		echo "Could not search for existing PRs referencing $repo#$issue; skipping this issue for now." >&2
		return 2
	fi
	[[ "$count" != "0" ]]
}

tmux_window_exists() {
	local window_name="$1"
	tmux list-windows -t "$SESSION_NAME" -F '#W' 2>/dev/null | grep -Fxq "$window_name"
}

tmux_window_path_matches() {
	local window_name="$1"
	local checkout_dir="$2"
	local target_path pane_path
	target_path="$(cd "$checkout_dir" && pwd -P)"
	while IFS= read -r pane_path; do
		[[ -n "$pane_path" ]] || continue
		if [[ "$(cd "$pane_path" 2>/dev/null && pwd -P)" == "$target_path" ]]; then
			return 0
		fi
	done < <(tmux list-panes -t "$SESSION_NAME:$window_name" -F '#{pane_current_path}' 2>/dev/null || true)
	return 1
}

mark_blocked() {
	local repo="$1"
	local issue="$2"
	local message="${3:-}"
	gh issue edit "$issue" \
		--repo "$repo" \
		--add-label "$BLOCKED_LABEL" \
		--remove-label "$WORKING_LABEL" >/dev/null || true
	if [[ -n "$message" ]]; then
		gh issue comment "$issue" --repo "$repo" --body "$message" >/dev/null || true
	fi
}

claim_issue() {
	local repo="$1"
	local issue="$2"
	local labels trust_message trust_status

	ensure_state_labels "$repo"
	labels="$(issue_labels_csv "$repo" "$issue")"
	set +e
	trust_message="$(trusted_label_actor "$repo" "$issue")"
	trust_status=$?
	set -e
	case "$trust_status" in
	0) ;;
	1)
		[[ -n "$trust_message" ]] && echo "$trust_message"
		return 1
		;;
	*)
		mark_blocked "$repo" "$issue" \
			"Local Every Code automation could not verify who is allowed to trigger this issue. $trust_message"
		return 1
		;;
	esac
	if labels_include "$labels" "$WORKING_LABEL" ||
		labels_include "$labels" "$DONE_LABEL" ||
		labels_include "$labels" "$BLOCKED_LABEL"; then
		echo "Skipping $repo#$issue: already claimed or completed."
		return 1
	fi

	gh issue edit "$issue" \
		--repo "$repo" \
		--add-label "$WORKING_LABEL" \
		--remove-label "$BLOCKED_LABEL" >/dev/null

	labels="$(issue_labels_csv "$repo" "$issue")"
	if ! labels_include "$labels" "$WORKING_LABEL"; then
		echo "Skipping $repo#$issue: could not confirm working label."
		return 1
	fi
}

launch_issue_window() {
	local repo="$1"
	local issue="$2"
	local checkout_dir window_name legacy_window_name poller_path

	checkout_dir="$(checkout_dir_for_repo "$repo")"
	window_name="$(window_name_for_issue "$repo" "$issue")"
	poller_path="$(script_path)/$(basename "${BASH_SOURCE[0]}")"

	if tmux_window_exists "$window_name"; then
		echo "Already running in tmux window for $repo#$issue"
		return 0
	fi
	legacy_window_name="$(legacy_window_name_for_issue "$repo" "$issue")"
	if tmux_window_exists "$legacy_window_name" && tmux_window_path_matches "$legacy_window_name" "$checkout_dir"; then
		echo "Already running in legacy tmux window for $repo#$issue"
		return 0
	fi

	if ! git -C "$checkout_dir" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
		echo "No local checkout for $repo at $checkout_dir; marking blocked."
		mark_blocked "$repo" "$issue" \
			"Local Every Code automation could not start because this host has no checkout at \`$checkout_dir\`."
		return 0
	fi

	if ! claim_issue "$repo" "$issue"; then
		return 0
	fi

	tmux new-window \
		-t "$SESSION_NAME" \
		-n "$window_name" \
		"$(printf '%q ' "$poller_path" job "$repo" "$issue")"
	echo "Launched $repo#$issue in tmux window $window_name"
}

build_prompt() {
	local repo="$1"
	local issue="$2"
	local title="$3"
	local url="$4"
	cat <<EOF
You are running on this host from the Every Code GitHub issue poller.

Repository: $repo
Issue: #$issue
Title: $title
URL: $url

Goal:
- Inspect the GitHub issue and local repository.
- Create a focused branch if needed.
- Make a minimal, production-ready change that addresses the issue.
- Follow this repository's AGENTS.md instructions.
- Validate appropriately for the repository.
- Commit your work, push a task branch, and open or update a pull request.
- Leave a concise final summary with the PR URL or the blocker.

Do not merge the PR.
EOF
}

run_issue_job() {
	require_tool gh
	require_tool "$CODE_BIN"

	local repo="$1"
	local issue="$2"
	local checkout_dir title url prompt status

	checkout_dir="$(checkout_dir_for_repo "$repo")"
	title="$(gh issue view "$issue" --repo "$repo" --json title --jq '.title')"
	url="$(gh issue view "$issue" --repo "$repo" --json url --jq '.url')"

	cd "$checkout_dir"
	clear || true
	echo "Every Code local job"
	echo "Repo:  $repo"
	echo "Issue: #$issue"
	echo "Title: $title"
	echo "URL:   $url"
	echo
	echo "Starting at $(date)"
	echo

	prompt="$(build_prompt "$repo" "$issue" "$title" "$url")"
	set +e
	if [[ -n "${EVERY_CODE_MODEL:-}" ]]; then
		"$CODE_BIN" exec \
			--full-auto \
			--model "$EVERY_CODE_MODEL" \
			--sandbox "$SANDBOX_MODE" \
			--max-seconds "$MAX_SECONDS" \
			--cd "$checkout_dir" \
			"$prompt"
	else
		"$CODE_BIN" exec \
			--full-auto \
			--sandbox "$SANDBOX_MODE" \
			--max-seconds "$MAX_SECONDS" \
			--cd "$checkout_dir" \
			"$prompt"
	fi
	status=$?
	set -e

	echo
	echo "Code exited with status $status at $(date)."
	if [[ "$status" -eq 0 ]]; then
		gh issue edit "$issue" \
			--repo "$repo" \
			--add-label "$DONE_LABEL" \
			--remove-label "$WORKING_LABEL" >/dev/null || true
	else
		mark_blocked "$repo" "$issue"
	fi
	echo
	echo "Window left open for inspection. Press Ctrl-D or exit when done."
	exec "${SHELL:-/bin/zsh}" -l
}

scan_once_unlocked() {
	require_tool gh
	require_tool tmux
	require_owner

	local repo issue _title _url pr_status
	while IFS= read -r repo; do
		refresh_scan_lock
		[[ -n "$repo" ]] || continue
		ensure_state_labels "$repo"
		while IFS=$'\t' read -r issue _title _url; do
			refresh_scan_lock
			[[ -n "${issue:-}" ]] || continue
			set +e
			issue_has_open_pr "$repo" "$issue"
			pr_status=$?
			set -e
			if [[ "$pr_status" -eq 0 ]]; then
				echo "Skipping $repo#$issue: open PR already references issue."
				continue
			elif [[ "$pr_status" -gt 1 ]]; then
				continue
			fi
			launch_issue_window "$repo" "$issue"
		done < <(list_ready_issues "$repo")
	done < <(list_repos)
}

scan_once() {
	with_scan_lock
}

run_loop() {
	require_tool gh
	require_tool tmux
	require_owner
	echo "Every Code poller running. owner=$SCAN_OWNER topic=$TOPIC label=$READY_LABEL root=$CHECKOUT_ROOT interval=${POLL_INTERVAL_SECONDS}s"
	while true; do
		echo
		echo "Scan started at $(date)"
		if ! scan_once; then
			echo "Scan failed at $(date); will retry."
		fi
		echo "Scan finished at $(date); sleeping ${POLL_INTERVAL_SECONDS}s."
		sleep "$POLL_INTERVAL_SECONDS"
	done
}

start_session() {
	require_tool tmux
	if tmux has-session -t "$SESSION_NAME" 2>/dev/null; then
		echo "tmux session '$SESSION_NAME' already exists. Attaching."
	else
		local poller_path
		poller_path="$(script_path)/$(basename "${BASH_SOURCE[0]}")"
		tmux new-session -d -s "$SESSION_NAME" -n poller "$(printf '%q ' "$poller_path" run)"
		echo "Started tmux session '$SESSION_NAME'."
	fi
	tmux attach -t "$SESSION_NAME"
}

stop_session() {
	require_tool tmux
	if tmux has-session -t "$SESSION_NAME" 2>/dev/null; then
		tmux kill-session -t "$SESSION_NAME"
	fi
	release_scan_lock
}

case "${1:-}" in
start) start_session ;;
run) run_loop ;;
once) scan_once ;;
job)
	if [[ $# -ne 3 ]]; then
		usage >&2
		exit 2
	fi
	run_issue_job "$2" "$3"
	;;
attach)
	require_tool tmux
	tmux attach -t "$SESSION_NAME"
	;;
stop) stop_session ;;
-h | --help | help) usage ;;
*)
	usage >&2
	exit 2
	;;
esac
