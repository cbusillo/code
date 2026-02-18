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

run_help_checks() {
  local bin_path="$1"
  local label="$2"

  if [[ ! -x "$bin_path" ]]; then
    if command -v "$bin_path" >/dev/null 2>&1; then
      bin_path="$(command -v "$bin_path")"
    else
      echo "$label binary not executable or not found: $bin_path" >&2
      exit 1
    fi
  fi

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

  echo "[cli-smoke] $label checks passed" >&2
  printf '%s\n' "$version"
}

code_version="$(run_help_checks "$CODE_BIN" "code")"

if [[ -n "$ECC_BIN" ]]; then
  ecc_version="$(run_help_checks "$ECC_BIN" "ecc")"
  if [[ "$code_version" != "$ecc_version" ]]; then
    echo "Version mismatch between code and ecc:" >&2
    echo "  code: $code_version" >&2
    echo "  ecc:  $ecc_version" >&2
    exit 1
  fi
  echo "[cli-smoke] code/ecc version parity confirmed" >&2
fi
