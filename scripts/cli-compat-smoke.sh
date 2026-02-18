#!/usr/bin/env bash
set -euo pipefail

CODE_BIN="${CODE_BIN:-code}"
ECC_BIN="${ECC_BIN:-}"

usage() {
  cat <<'EOF'
Usage: scripts/cli-compat-smoke.sh [options]

Run a lightweight CLI compatibility sweep for core TUI workflows.

Options:
  --code PATH    Path to the code-compatible binary (default: code in PATH)
  --ecc PATH     Optional path to an ecc alias binary/shim
  -h, --help     Show this help text

Environment overrides:
  CODE_BIN, ECC_BIN
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --code)
      CODE_BIN="$2"
      shift 2
      ;;
    --ecc)
      ECC_BIN="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage
      exit 1
      ;;
  esac
done

resolve_binary() {
  local candidate="$1"
  if [[ -x "$candidate" ]]; then
    printf '%s\n' "$candidate"
    return 0
  fi

  if command -v "$candidate" >/dev/null 2>&1; then
    command -v "$candidate"
    return 0
  fi

  return 1
}

extract_doctor_field() {
  local output="$1"
  local field="$2"
  awk -v key="$field" -F': ' '$1 == key { print $2; exit }' <<< "$output"
}

run_help_checks() {
  local bin_path="$1"
  local label="$2"

  echo "[cli-smoke] checking $label: $bin_path" >&2

  local root_help
  root_help="$($bin_path --help)"
  [[ "$root_help" == *"Usage:"* ]]

  local version
  version="$($bin_path --version)"
  [[ -n "$version" ]]

  local exec_help
  exec_help="$($bin_path exec --help)"
  [[ "$exec_help" == *"resume"* ]]

  local resume_help
  resume_help="$($bin_path resume --help)"
  [[ "$resume_help" == *"--last"* ]]

  local web_help
  web_help="$($bin_path web --help)"
  [[ "$web_help" == *"--host"* ]]

  local mcp_help
  mcp_help="$($bin_path mcp --help)"
  [[ "$mcp_help" == *"stdio"* ]]

  echo "[cli-smoke] $label help checks passed" >&2
  printf '%s\n' "$version"
}

run_doctor_checks() {
  local bin_path="$1"
  local label="$2"
  local expected_home="$3"

  expected_home="$(cd "$expected_home" && pwd -P)"

  local doctor_output
  doctor_output="$(CODE_HOME="$expected_home" CODEX_HOME='' "$bin_path" doctor)"

  local resolved_home
  resolved_home="$(extract_doctor_field "$doctor_output" "resolved code_home")"
  local resolved_sessions
  resolved_sessions="$(extract_doctor_field "$doctor_output" "resolved sessions_dir")"

  local expected_sessions
  expected_sessions="${expected_home}/sessions"

  if [[ "$resolved_home" != "$expected_home" ]]; then
    echo "[cli-smoke] $label resolved code_home mismatch" >&2
    echo "  expected: $expected_home" >&2
    echo "  actual:   $resolved_home" >&2
    exit 1
  fi

  if [[ "$resolved_sessions" != "$expected_sessions" ]]; then
    echo "[cli-smoke] $label resolved sessions_dir mismatch" >&2
    echo "  expected: $expected_sessions" >&2
    echo "  actual:   $resolved_sessions" >&2
    exit 1
  fi

  echo "[cli-smoke] $label doctor checks passed" >&2

  printf '%s\n' "$resolved_home|$resolved_sessions"
}

CODE_BIN_RESOLVED="$(resolve_binary "$CODE_BIN")"
code_version="$(run_help_checks "$CODE_BIN_RESOLVED" "code")"

probe_home="$(mktemp -d)"
trap 'rm -rf "$probe_home"' EXIT

code_doctor_tuple="$(run_doctor_checks "$CODE_BIN_RESOLVED" "code" "$probe_home")"

if [[ -n "$ECC_BIN" ]]; then
  ECC_BIN_RESOLVED="$(resolve_binary "$ECC_BIN")"
  ecc_version="$(run_help_checks "$ECC_BIN_RESOLVED" "ecc")"

  if [[ "$code_version" != "$ecc_version" ]]; then
    echo "Version mismatch between code and ecc:" >&2
    echo "  code: $code_version" >&2
    echo "  ecc:  $ecc_version" >&2
    exit 1
  fi

  ecc_doctor_tuple="$(run_doctor_checks "$ECC_BIN_RESOLVED" "ecc" "$probe_home")"
  if [[ "$code_doctor_tuple" != "$ecc_doctor_tuple" ]]; then
    echo "Doctor mismatch between code and ecc:" >&2
    echo "  code: $code_doctor_tuple" >&2
    echo "  ecc:  $ecc_doctor_tuple" >&2
    exit 1
  fi

  echo "[cli-smoke] code/ecc parity confirmed" >&2
fi
