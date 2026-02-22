# Every Code Companion iOS Demo

This folder contains a lightweight iOS/iPadOS demo app target that reuses the
shared SwiftUI client from `native/CodeNative/Sources/CodeNativeApp`.

## Generate the project

```bash
cd native/CodeNativeiOS
xcodegen generate
```

This creates `CodeNativeiOSDemo.xcodeproj`.

## Run in Xcode

1. Open `native/CodeNativeiOS/CodeNativeiOSDemo.xcodeproj`.
2. Pick an iPhone or iPad simulator.
3. Run `CodeNativeiOSDemo`.

On iOS, Every Code Companion runs in companion mode and does not launch a
local bundled backend. Connect it to a Mac/CLI endpoint using pairing or a
manual `ws://` / `wss://` endpoint.

## Branding and Attribution

Use the following legal attribution in release metadata and in-app surfaces:

`Every Code Companion is an independent project and is not affiliated with or
endorsed by Every Code.`

## iOS Core Controls

- Top bar actions now include:
  - quick actions menu (`New thread`, `Refresh`, and `Reconnect` when disconnected)
  - thread picker button
  - settings button
- Pull down in the thread picker to refresh sessions.

## Smoke Test

Run the iPhone simulator smoke test for top-bar controls and thread picker:

```bash
SIM_NAME="$(xcrun simctl list devices available \
  | awk -F'[()]' '/iPhone/{print $1; exit}' \
  | xargs)"
TEST_CASE="CodeNativeiOSDemoUITests/CodeNativeiOSDemoUITests/testTopBarQuickActionsAndThreadPicker"

xcodebuild \
  -project native/CodeNativeiOS/CodeNativeiOSDemo.xcodeproj \
  -scheme CodeNativeiOSDemo \
  -destination "platform=iOS Simulator,name=${SIM_NAME}" \
  test \
  -only-testing:"${TEST_CASE}"
```

Run the iPad split-layout smoke test:

```bash
IPAD_SIM="$(xcrun simctl list devices available \
  | awk -F'[()]' '/iPad/{print $1; exit}' \
  | xargs)"
IPAD_TEST_CASE="CodeNativeiOSDemoUITests/CodeNativeiOSDemoUITests/testIPadSplitLayoutShowsPersistentSidebar"

xcodebuild \
  -project native/CodeNativeiOS/CodeNativeiOSDemo.xcodeproj \
  -scheme CodeNativeiOSDemo \
  -destination "platform=iOS Simulator,name=${IPAD_SIM}" \
  test \
  -only-testing:"${IPAD_TEST_CASE}"
```

## Keep Platforms In Sync

- Put session parsing/state logic in shared files under
  `native/CodeNative/Sources/CodeNativeApp`.
- Keep platform APIs at the edges (`#if os(macOS)` / iOS-specific wrappers).
- Avoid importing `AppKit` in shared files unless guarded.

## Release Metadata Source

App Store/TestFlight listing copy lives in
`docs/native-app-store-metadata.md`.

## CI TestFlight Build

Use `.github/workflows/native-ios-testflight.yml` (manual dispatch) to produce
a signed IPA artifact and optionally upload it to TestFlight.

Pushing a tag that matches `ios-v*` triggers the same workflow automatically
for release-driven uploads.

Use `scripts/release-ios-tag.sh 1.4.0` to create and push a matching tag.
