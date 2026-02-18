#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${1:-$ROOT_DIR/.code/native-artifacts}"
DERIVED_DATA_DIR="$OUT_DIR/derived-data"
MACOS_BIN_DIR="$OUT_DIR/macos"
IOS_BIN_DIR="$OUT_DIR/ios-sim"
PUBLIC_APP_NAME="EveryCodeCompanion"

mkdir -p "$MACOS_BIN_DIR" "$IOS_BIN_DIR"

echo "[native-release] building CodeNativeApp (release)"
swift build --configuration release --package-path "$ROOT_DIR/native/CodeNative"

MACOS_BINARY="$ROOT_DIR/native/CodeNative/.build/release/CodeNativeApp"
cp "$MACOS_BINARY" "$MACOS_BIN_DIR/$PUBLIC_APP_NAME"

echo "[native-release] building CodeNativeiOSDemo (release simulator)"
xcodebuild \
	-project "$ROOT_DIR/native/CodeNativeiOS/CodeNativeiOSDemo.xcodeproj" \
	-scheme CodeNativeiOSDemo \
	-configuration Release \
	-destination "generic/platform=iOS Simulator" \
	-derivedDataPath "$DERIVED_DATA_DIR" \
	build

IOS_APP="$DERIVED_DATA_DIR/Build/Products/Release-iphonesimulator/CodeNativeiOSDemo.app"
IOS_PUBLIC_APP="$IOS_BIN_DIR/$PUBLIC_APP_NAME.app"
rm -rf "$IOS_PUBLIC_APP"
rsync -a "$IOS_APP/" "$IOS_PUBLIC_APP/"

echo "[native-release] artifacts ready"
echo "  macOS: $MACOS_BIN_DIR/$PUBLIC_APP_NAME"
echo "  iOS simulator app: $IOS_PUBLIC_APP"
