#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/native-preflight-macos.sh [options]

Build a signed macOS package via GitHub Actions without TestFlight upload,
download the artifact, expand the pkg, launch the app payload, and run a
basic managed-backend crash smoke check.

Options:
  -r, --repo OWNER/REPO       GitHub repo (default: current gh repo)
  -b, --branch BRANCH         Branch/ref to build (default: current branch)
  -w, --workflow FILE         Workflow file (default: native-macos-testflight.yml)
  -n, --workflow-name NAME    Workflow display name (default: Native macOS TestFlight)
  -d, --artifact-dir PATH     Artifact output dir (default: /tmp/ecc-preflight)
  -s, --smoke-seconds N       Seconds to observe runtime logs (default: 25)
  -i, --interval N            GH polling interval seconds (default: 15)
  -h, --help                  Show this help text
EOF
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: missing required command '$1'" >&2
    exit 1
  fi
}

current_branch() {
  git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "fork-main"
}

REPO="$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || true)"
BRANCH="$(current_branch)"
WORKFLOW_FILE="native-macos-testflight.yml"
WORKFLOW_NAME="Native macOS TestFlight"
ARTIFACT_DIR="/tmp/ecc-preflight"
SMOKE_SECONDS="25"
INTERVAL="15"

while [[ $# -gt 0 ]]; do
  case "$1" in
    -r|--repo)
      REPO="${2:-}"
      shift 2
      ;;
    -b|--branch)
      BRANCH="${2:-}"
      shift 2
      ;;
    -w|--workflow)
      WORKFLOW_FILE="${2:-}"
      shift 2
      ;;
    -n|--workflow-name)
      WORKFLOW_NAME="${2:-}"
      shift 2
      ;;
    -d|--artifact-dir)
      ARTIFACT_DIR="${2:-}"
      shift 2
      ;;
    -s|--smoke-seconds)
      SMOKE_SECONDS="${2:-}"
      shift 2
      ;;
    -i|--interval)
      INTERVAL="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown option '$1'" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -z "$REPO" ]]; then
  echo "error: failed to resolve repo; pass --repo OWNER/REPO" >&2
  exit 1
fi

require_cmd gh
require_cmd jq
require_cmd pkgutil
require_cmd open
require_cmd log

echo "Dispatching ${WORKFLOW_FILE} on ${REPO} (${BRANCH}) with upload_to_testflight=false..."
gh workflow run "$WORKFLOW_FILE" \
  -R "$REPO" \
  --ref "$BRANCH" \
  -f git_ref="$BRANCH" \
  -f upload_to_testflight=false

run_id=""
for _ in {1..20}; do
  run_id="$(gh run list \
    -R "$REPO" \
    --workflow "$WORKFLOW_NAME" \
    --branch "$BRANCH" \
    --limit 1 \
    --json databaseId \
    -q '.[0].databaseId // ""')"
  if [[ -n "$run_id" ]]; then
    break
  fi
  sleep 3
done

if [[ -z "$run_id" ]]; then
  echo "error: failed to resolve workflow run id" >&2
  exit 1
fi

echo "Waiting for run ${run_id}..."
GH_REPO="$REPO" scripts/wait-for-gh-run.sh \
  --run "$run_id" \
  --interval "$INTERVAL"

run_url="https://github.com/${REPO}/actions/runs/${run_id}"
echo "Run succeeded: ${run_url}"

rm -rf "$ARTIFACT_DIR"
mkdir -p "$ARTIFACT_DIR"

echo "Downloading artifact..."
gh run download "$run_id" \
  -R "$REPO" \
  -n EveryCodeCompanion-macos-pkg \
  -D "$ARTIFACT_DIR"

pkg_path="$(find "$ARTIFACT_DIR" -maxdepth 2 -name '*.pkg' -print -quit)"
if [[ -z "$pkg_path" ]]; then
  echo "error: pkg artifact not found under ${ARTIFACT_DIR}" >&2
  exit 1
fi

expanded_dir="${ARTIFACT_DIR}/pkg-expanded"
rm -rf "$expanded_dir"
pkgutil --expand-full "$pkg_path" "$expanded_dir" >/dev/null

app_path="$(find "$expanded_dir" -type d -name 'EveryCodeCompanion.app' -print -quit)"
if [[ -z "$app_path" ]]; then
  echo "error: EveryCodeCompanion.app not found in expanded pkg" >&2
  exit 1
fi

start_epoch="$(date +%s)"
echo "Launching payload app for ${SMOKE_SECONDS}s smoke check..."
open -na "$app_path"
sleep "$SMOKE_SECONDS"

log_path="${ARTIFACT_DIR}/runtime.log"
log show --style compact --last "${SMOKE_SECONDS}s" \
  --predicate 'subsystem == "com.every.code.native" OR process == "code" OR process == "secinitd"' \
  > "$log_path" 2>/dev/null || true

crash_hits="$(
  find "$HOME/Library/Logs/DiagnosticReports" \
    "$HOME/Library/Logs/DiagnosticReports/Retired" \
    -name 'code-*.ips' \
    -type f \
    -newermt "@${start_epoch}" 2>/dev/null | wc -l | tr -d ' '
)"

has_term=0
has_bundle_id_fault=0
if rg -q 'Managed backend terminated unexpectedly' "$log_path"; then
  has_term=1
fi
if rg -q 'Unable to get bundle identifier because Info.plist from code signature information has no value for kCFBundleIdentifierKey' "$log_path"; then
  has_bundle_id_fault=1
fi

echo "--- Preflight summary ---"
echo "Run: ${run_url}"
echo "Artifact dir: ${ARTIFACT_DIR}"
echo "Payload app: ${app_path}"
echo "Runtime log: ${log_path}"
echo "New code crash reports: ${crash_hits}"

if [[ "$has_term" -eq 1 || "$has_bundle_id_fault" -eq 1 || "$crash_hits" -gt 0 ]]; then
  echo "result: FAIL"
  echo "Detected runtime startup regressions in managed backend." >&2
  exit 1
fi

echo "result: PASS"
