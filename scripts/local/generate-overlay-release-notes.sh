#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'USAGE'
Usage: scripts/local/generate-overlay-release-notes.sh \
  --tag TAG \
  --code-version VERSION \
  --commit SHA \
  --ref-name REF \
  --dist-dir DIR \
  --output FILE

Generates GitHub release notes for fork-owned overlay binary releases.
USAGE
}

tag=""
code_version=""
commit=""
ref_name=""
dist_dir=""
output=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tag)
      tag="${2:-}"
      shift 2
      ;;
    --code-version)
      code_version="${2:-}"
      shift 2
      ;;
    --commit)
      commit="${2:-}"
      shift 2
      ;;
    --ref-name)
      ref_name="${2:-}"
      shift 2
      ;;
    --dist-dir)
      dist_dir="${2:-}"
      shift 2
      ;;
    --output)
      output="${2:-}"
      shift 2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 2
      ;;
  esac
done

if [[ -z "$tag" || -z "$code_version" || -z "$commit" || -z "$ref_name" || -z "$dist_dir" || -z "$output" ]]; then
  usage
  exit 2
fi

previous_tag="$(git tag --list 'overlay-v*' --merged "$commit" --sort=-v:refname \
  | grep -Fxv "$tag" \
  | head -n 1 || true)"

if [[ -n "$previous_tag" ]]; then
  range_base="$previous_tag"
else
  upstream_tag="v${tag#overlay-v}"
  upstream_tag="${upstream_tag%.*}"
  if git rev-parse --verify --quiet "${upstream_tag}^{commit}" >/dev/null \
    && git merge-base --is-ancestor "$upstream_tag" "$commit"; then
    range_base="$upstream_tag"
  else
    parent_commit="$(git rev-parse --verify --quiet "${commit}^" || true)"
    range_base="$parent_commit"
  fi
fi

if [[ -n "$range_base" ]]; then
  range="${range_base}..${commit}"
else
  range="$commit"
fi

commit_summary="$(mktemp)"
fixes="$(mktemp)"
features="$(mktemp)"
maintenance_updates="$(mktemp)"
ci_updates="$(mktemp)"
other_updates="$(mktemp)"
seen_subjects="$(mktemp)"
listed_entry_count=0

git log --no-merges --format='%s' "$range" > "$commit_summary" \
  || git log -1 --format='%s' "$commit" > "$commit_summary"

while IFS= read -r subject; do
  [[ -n "$subject" ]] || continue
  if grep -Fxq -- "$subject" "$seen_subjects"; then
    continue
  fi
  printf '%s\n' "$subject" >> "$seen_subjects"
  lower_subject="$(printf '%s' "$subject" | tr '[:upper:]' '[:lower:]')"
  if [[ "$subject" =~ ^(fix|revert)(\(|:) ]]; then
    printf '%s\n' "$subject" >> "$fixes"
    listed_entry_count=$((listed_entry_count + 1))
  elif [[ "$subject" =~ ^feat(\(|:) ]]; then
    printf '%s\n' "$subject" >> "$features"
    listed_entry_count=$((listed_entry_count + 1))
  elif [[ "$subject" == "Add GitHub repo workflow metadata" ]]; then
    printf '%s\n' "$subject" >> "$maintenance_updates"
    listed_entry_count=$((listed_entry_count + 1))
  elif [[ "$subject" =~ ^(ci|build)(\(|:) ]] \
    || [[ "$lower_subject" == *workflow* ]] \
    || [[ "$lower_subject" == *runner* ]] \
    || [[ "$lower_subject" == *actionlint* ]]; then
    printf '%s\n' "$subject" >> "$ci_updates"
    listed_entry_count=$((listed_entry_count + 1))
  elif [[ "$subject" =~ ^(docs|chore|test)(\(|:) ]] \
    || [[ "$lower_subject" == *local* ]] \
    || [[ "$lower_subject" == *overlay* ]] \
    || [[ "$lower_subject" == *cleanup* ]]; then
    printf '%s\n' "$subject" >> "$maintenance_updates"
    listed_entry_count=$((listed_entry_count + 1))
  else
    printf '%s\n' "$subject" >> "$other_updates"
    listed_entry_count=$((listed_entry_count + 1))
  fi
done < "$commit_summary"

write_section() {
  local title="$1"
  local file="$2"
  if [[ -s "$file" ]]; then
    echo "### $title"
    echo ""
    while IFS= read -r subject; do
      [[ -n "$subject" ]] || continue
      echo "- $subject"
    done < "$file"
    echo ""
  fi
}

{
  echo "## What's Changed"
  echo ""
  echo "This overlay release carries local fork updates on top of upstream Code $code_version."
  if [[ -n "$previous_tag" ]]; then
    echo "It includes $listed_entry_count listed changes since $previous_tag."
  fi
  echo ""

  write_section "Fixes" "$fixes"
  write_section "Features" "$features"
  write_section "Maintenance Updates" "$maintenance_updates"
  write_section "CI and Release Infrastructure" "$ci_updates"
  write_section "Other Updates" "$other_updates"

  echo "## Verification"
  echo ""
  echo "- Binary Release workflow completed successfully for Linux x64, macOS x86_64, and macOS aarch64."
  echo "- Release smoke tests ran for the Linux x64 and macOS aarch64 binaries before packaging."
  echo "- macOS x86_64 was built and packaged, but not smoke-tested in workflow."
  echo "- SHA-256 checksums are included in SHA256SUMS.txt."
  echo ""
  echo "## Build"
  echo ""
  echo "Built from \`$ref_name\` at \`$commit\`."
  if [[ -n "$previous_tag" ]]; then
    echo "Previous overlay release: \`$previous_tag\`."
  fi
  echo ""
  echo "### Assets"
  echo ""
  while IFS= read -r asset; do
    echo "- \`$asset\`"
  done < <(find "$dist_dir" -maxdepth 1 -type f -exec basename {} \; | sort)
} > "$output"
