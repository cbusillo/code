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

By default, Every Code Companion connects to `ws://127.0.0.1:4317/ws`.

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
