# CodeNative iOS Demo

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

By default, the app connects to `ws://127.0.0.1:4317/ws`.

## Keep Platforms In Sync

- Put session parsing/state logic in shared files under
  `native/CodeNative/Sources/CodeNativeApp`.
- Keep platform APIs at the edges (`#if os(macOS)` / iOS-specific wrappers).
- Avoid importing `AppKit` in shared files unless guarded.
