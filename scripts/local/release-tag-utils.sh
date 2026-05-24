#!/usr/bin/env bash

release_tag_version() {
  local release_tag="$1"
  case "$release_tag" in
    every-code-v*)
      printf '%s\n' "${release_tag#every-code-v}"
      ;;
    overlay-v*)
      printf '%s\n' "${release_tag#overlay-v}"
      ;;
    v*)
      printf '%s\n' "${release_tag#v}"
      ;;
    *)
      printf '%s\n' "$release_tag"
      ;;
  esac
}

strip_release_tag_version() {
  release_tag_version "$1"
}

release_tag_priority() {
  local release_tag="$1"
  case "$release_tag" in
    every-code-v*)
      printf '2\n'
      ;;
    overlay-v* | v*)
      printf '1\n'
      ;;
    *)
      printf '0\n'
      ;;
  esac
}

release_tag_sort_key() {
  local release_tag="$1"
  printf '%s\t%s\t%s\n' \
    "$(release_tag_version "$release_tag")" \
    "$(release_tag_priority "$release_tag")" \
    "$release_tag"
}

latest_release_tag() {
  while IFS= read -r release_tag; do
    [[ -n "$release_tag" ]] || continue
    release_tag_sort_key "$release_tag"
  done \
    | sort -t $'\t' -k1,1V -k2,2n -k3,3r \
    | tail -n 1 \
    | sed -n 's/^[^\t]*\t[^\t]*\t//p'
}

