#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PARITY_DIR="$ROOT_DIR/docs/reference/native-ui/parity"

TRIPLETS=(
	"workflow-active"
	"m2-activity-heavy"
	"m3-multi-agent-progress"
)

missing=0

for triplet in "${TRIPLETS[@]}"; do
	for app in tui codex-mac native; do
		artifact="$PARITY_DIR/${triplet}-${app}.png"
		if [[ ! -f "$artifact" ]]; then
			echo "[verify-parity-triplets] missing: $artifact" >&2
			missing=1
		fi
	done
done

if [[ "$missing" -ne 0 ]]; then
	exit 1
fi

echo "[verify-parity-triplets] verified triplet artifacts:"
for triplet in "${TRIPLETS[@]}"; do
	for app in tui codex-mac native; do
		artifact="$PARITY_DIR/${triplet}-${app}.png"
		checksum="$(shasum -a 256 "$artifact" | awk '{print $1}')"
		echo "  ${triplet}-${app}.png sha256=${checksum}"
	done
done
