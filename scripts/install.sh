#!/usr/bin/env bash
set -euo pipefail

REPO_DEFAULT="cbusillo/code"
REPO="${CODE_RELEASE_REPO:-$REPO_DEFAULT}"
TAG="${CODE_RELEASE_TAG:-}"
VERSION="${CODE_RELEASE_VERSION:-}"
INSTALL_DIR="${CODE_INSTALL_DIR:-}"
SKIP_CONFIG=false

usage() {
	cat <<'USAGE'
Usage: install.sh [--repo owner/name] [--tag vX.Y.Z.N] [--version X.Y.Z.N]
                  [--install-dir /path] [--skip-config]

Environment overrides:
  CODE_RELEASE_REPO      GitHub repo (owner/name)
  CODE_RELEASE_TAG       Release tag (e.g., v0.5.1.1)
  CODE_RELEASE_VERSION   Release version (e.g., 0.5.1.1)
  CODE_INSTALL_DIR       Install directory
USAGE
}

while [[ $# -gt 0 ]]; do
	case "$1" in
	--repo)
		REPO="$2"
		shift 2
		;;
	--tag)
		TAG="$2"
		shift 2
		;;
	--version)
		VERSION="$2"
		shift 2
		;;
	--install-dir)
		INSTALL_DIR="$2"
		shift 2
		;;
	--skip-config)
		SKIP_CONFIG=true
		shift
		;;
	-h | --help)
		usage
		exit 0
		;;
	*)
		echo "Unknown argument: $1" >&2
		usage
		exit 1
		;;
	esac
done

if [[ -n "$VERSION" && -z "$TAG" ]]; then
	TAG="v${VERSION}"
fi

fetch_latest_tag() {
	curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" |
		sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' |
		head -n 1
}

verify_checksums() {
	local checksum_file="$1"
	if command -v sha256sum >/dev/null 2>&1; then
		(cd "$TMP_DIR" && sha256sum -c "$checksum_file" --ignore-missing)
		return 0
	fi
	if command -v shasum >/dev/null 2>&1; then
		(cd "$TMP_DIR" && shasum -a 256 -c "$checksum_file")
		return 0
	fi
	echo "sha256sum/shasum not found; skipping checksum verification." >&2
	return 0
}

if [[ -z "$TAG" ]]; then
	TAG="$(fetch_latest_tag || true)"
fi

if [[ -z "$TAG" ]]; then
	echo "Unable to resolve release tag for ${REPO}." >&2
	exit 1
fi

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
Darwin)
	if [[ "$ARCH" == "arm64" ]]; then
		ASSET="code-aarch64-apple-darwin.tar.gz"
	else
		ASSET="code-x86_64-apple-darwin.tar.gz"
	fi
	;;
Linux)
	if [[ "$ARCH" == "aarch64" || "$ARCH" == "arm64" ]]; then
		ASSET="code-aarch64-unknown-linux-musl.tar.gz"
	else
		ASSET="code-x86_64-unknown-linux-musl.tar.gz"
	fi
	;;
*)
	echo "Unsupported OS: $OS" >&2
	exit 1
	;;
esac

BASE_URL="https://github.com/${REPO}/releases/download/${TAG}"
TMP_DIR="$(mktemp -d)"
cleanup() { rm -rf "$TMP_DIR"; }
trap cleanup EXIT

curl -fsSL "${BASE_URL}/${ASSET}" -o "${TMP_DIR}/${ASSET}"
if curl -fsSL "${BASE_URL}/SHA256SUMS.txt" -o "${TMP_DIR}/SHA256SUMS.txt"; then
	verify_checksums "SHA256SUMS.txt"
fi

tar -C "$TMP_DIR" -xzf "${TMP_DIR}/${ASSET}"
rm -f "${TMP_DIR:?}/${ASSET}"
BIN_SRC="$(find "$TMP_DIR" -maxdepth 1 -type f -name 'code-*' ! -name '*.tar.gz' -perm -111 | head -n 1)"
if [[ -z "$BIN_SRC" ]]; then
	echo "Unable to find extracted binary." >&2
	exit 1
fi

if [[ -z "$INSTALL_DIR" ]]; then
	if command -v code >/dev/null 2>&1; then
		INSTALL_DIR="$(dirname "$(command -v code)")"
	else
		INSTALL_DIR="$HOME/.local/bin"
	fi
fi

mkdir -p "$INSTALL_DIR"
if [[ -w "$INSTALL_DIR" ]]; then
	install -m 755 "$BIN_SRC" "${INSTALL_DIR}/code"
else
	echo "Installing to ${INSTALL_DIR} requires elevated permissions." >&2
	sudo install -m 755 "$BIN_SRC" "${INSTALL_DIR}/code"
fi

echo "Installed code to ${INSTALL_DIR}/code"

if [[ "$SKIP_CONFIG" != true ]]; then
	CONFIG_DIR="$HOME/.code"
	CONFIG_FILE="$CONFIG_DIR/config.toml"
	mkdir -p "$CONFIG_DIR"

	if [[ ! -f "$CONFIG_FILE" ]]; then
		touch "$CONFIG_FILE"
	fi

	if ! grep -q "^\[updates\]" "$CONFIG_FILE"; then
		cat >>"$CONFIG_FILE" <<CONFIG

[updates]
release_repo = "${REPO}"
upgrade_command = ["bash", "-lc", "curl -fsSL https://github.com/${REPO}/releases/latest/download/install.sh | bash"]
CONFIG
	else
		if ! grep -q "^release_repo" "$CONFIG_FILE" || ! grep -q "^upgrade_command" "$CONFIG_FILE"; then
			echo "Existing [updates] block found in ${CONFIG_FILE}." >&2
			echo "Please set release_repo and upgrade_command manually:" >&2
			echo "  release_repo = \"${REPO}\"" >&2
			echo "  upgrade_command = [\"bash\", \"-lc\", \"curl -fsSL https://github.com/${REPO}/releases/latest/download/install.sh | bash\"]" >&2
		fi
	fi
fi

echo "Done."
