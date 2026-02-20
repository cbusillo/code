#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/native-preflight-macos.sh [options]

Build a signed macOS package via GitHub Actions without TestFlight upload,
download the artifact, expand the pkg, and run backend payload smoke checks.

Options:
  -r, --repo OWNER/REPO       GitHub repo (default: current git origin)
  -b, --branch BRANCH         Branch/ref to build (default: current branch)
  -w, --workflow FILE         Workflow file (default: native-macos-testflight.yml)
  -n, --workflow-name NAME    Workflow display name (default: Native macOS TestFlight)
  -d, --artifact-dir PATH     Artifact output dir (default: /tmp/ecc-preflight)
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

parse_repo_from_remote_url() {
  local remote_url="$1"

  case "$remote_url" in
    git@github.com:*)
      remote_url="${remote_url#git@github.com:}"
      remote_url="${remote_url%.git}"
      ;;
    https://github.com/*)
      remote_url="${remote_url#https://github.com/}"
      remote_url="${remote_url%.git}"
      ;;
    *)
      remote_url=""
      ;;
  esac

  if [[ "$remote_url" == */* ]]; then
    echo "$remote_url"
  fi
}

default_repo() {
  local remote_url=""
  local parsed=""

  for remote in origin fork upstream; do
    remote_url="$(git remote get-url "$remote" 2>/dev/null || true)"
    if [[ -z "$remote_url" ]]; then
      continue
    fi

    parsed="$(parse_repo_from_remote_url "$remote_url")"
    if [[ -n "$parsed" ]]; then
      echo "$parsed"
      return 0
    fi
  done

  gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || true
}

current_branch() {
  git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "fork-main"
}

REPO="$(default_repo)"
BRANCH="$(current_branch)"
WORKFLOW_FILE="native-macos-testflight.yml"
WORKFLOW_NAME="Native macOS TestFlight"
ARTIFACT_DIR="/tmp/ecc-preflight"
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

expected_sha="$(git rev-parse "$BRANCH" 2>/dev/null || echo "")"
baseline_run_id="$(gh run list \
  -R "$REPO" \
  --workflow "$WORKFLOW_NAME" \
  --branch "$BRANCH" \
  --limit 1 \
  --json databaseId \
  -q '.[0].databaseId // ""' 2>/dev/null || true)"

echo "Dispatching ${WORKFLOW_FILE} on ${REPO} (${BRANCH}) with upload_to_testflight=false..."
gh workflow run "$WORKFLOW_FILE" \
  -R "$REPO" \
  --ref "$BRANCH" \
  -f git_ref="$BRANCH" \
  -f upload_to_testflight=false

run_id=""
for _ in {1..40}; do
  runs_json="$(gh run list \
    -R "$REPO" \
    --workflow "$WORKFLOW_NAME" \
    --branch "$BRANCH" \
    --limit 10 \
    --json databaseId,headSha 2>/dev/null || echo '[]')"

  if [[ -n "$expected_sha" ]]; then
    run_id="$(jq -r --arg sha "$expected_sha" --arg baseline "$baseline_run_id" '
      .[]
      | select(.headSha == $sha)
      | select(($baseline | length) == 0 or ((.databaseId | tostring) != $baseline))
      | .databaseId
      ' <<<"$runs_json" | head -n1)"
  else
    run_id="$(jq -r --arg baseline "$baseline_run_id" '
      .[]
      | select(($baseline | length) == 0 or ((.databaseId | tostring) != $baseline))
      | .databaseId
      ' <<<"$runs_json" | head -n1)"
  fi

  if [[ -n "$run_id" && "$run_id" != "null" ]]; then
    break
  fi
  sleep 3
done

if [[ -z "$run_id" ]]; then
  echo "error: failed to resolve workflow run id" >&2
  exit 1
fi

echo "Waiting for run ${run_id}..."
gh run watch "$run_id" \
  -R "$REPO" \
  --interval "$INTERVAL" \
  --exit-status

run_url="https://github.com/${REPO}/actions/runs/${run_id}"
echo "Run succeeded: ${run_url}"

rm -rf "$ARTIFACT_DIR"
mkdir -p "$ARTIFACT_DIR"

echo "Downloading artifact..."
gh run download "$run_id" \
  -R "$REPO" \
  -n EveryCodeCompanion-macos-pkg \
  -D "$ARTIFACT_DIR"

pkg_path="$(find "$ARTIFACT_DIR" -path '*/export/EveryCodeCompanion.pkg' -print -quit)"
if [[ -z "$pkg_path" ]]; then
  pkg_path="$(find "$ARTIFACT_DIR" -maxdepth 3 -name '*.pkg' -print -quit)"
fi
if [[ -z "$pkg_path" ]]; then
  echo "error: pkg artifact not found under ${ARTIFACT_DIR}" >&2
  exit 1
fi

expanded_dir="${ARTIFACT_DIR}/pkg-expanded"
rm -rf "$expanded_dir"
pkgutil --expand-full "$pkg_path" "$expanded_dir" >/dev/null

app_path="$(find "$expanded_dir" -type d -name 'EveryCodeCompanion.app' -print -quit)"
if [[ -z "$app_path" ]]; then
  echo "error: payload app not found in expanded pkg" >&2
  exit 1
fi

backend_bundle="$app_path/Contents/Resources/backend/CodeBackend.app"
backend_code="$backend_bundle/Contents/MacOS/code"
backend_ecc="$app_path/Contents/Resources/backend/ecc"

if [[ ! -x "$backend_code" ]]; then
  echo "error: backend code binary missing at ${backend_code}" >&2
  exit 1
fi
if [[ ! -x "$backend_ecc" ]]; then
  echo "error: backend ecc shim missing at ${backend_ecc}" >&2
  exit 1
fi

echo "Running local CLI compatibility smoke checks..."
scripts/cli-compat-smoke.sh --code "$backend_code" --ecc "$backend_ecc"

backend_info="$backend_bundle/Contents/Info.plist"
if [[ ! -f "$backend_info" ]]; then
  echo "result: FAIL"
  echo "Bundled backend missing Info.plist" >&2
  exit 1
fi

backend_bundle_id="$(
  /usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' \
    "$backend_info" 2>/dev/null || true
)"
if [[ -z "$backend_bundle_id" ]]; then
  echo "result: FAIL"
  echo "Bundled backend missing CFBundleIdentifier" >&2
  exit 1
fi

backend_executable="$(
  /usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' \
    "$backend_info" 2>/dev/null || true
)"
if [[ "$backend_executable" != "code" ]]; then
  echo "result: FAIL"
  echo "Bundled backend CFBundleExecutable must be 'code'" >&2
  exit 1
fi

echo "--- Preflight summary ---"
echo "Run: ${run_url}"
echo "Artifact dir: ${ARTIFACT_DIR}"
echo "Payload app: ${app_path}"

entitlements_xml="$(codesign -d --entitlements :- "$backend_code" 2>/dev/null || true)"
if [[ "$entitlements_xml" != *"com.apple.security.inherit"* ]]; then
  echo "result: FAIL"
  echo "Missing backend entitlement: com.apple.security.inherit" >&2
  exit 1
fi
if [[ "$entitlements_xml" != *"com.apple.security.app-sandbox"* ]]; then
  echo "result: FAIL"
  echo "Missing backend entitlement: com.apple.security.app-sandbox" >&2
  exit 1
fi

echo "result: PASS"
