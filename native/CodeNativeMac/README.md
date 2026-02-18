# Every Code Companion macOS Demo

This folder contains a native macOS app target that reuses shared SwiftUI
sources from `native/CodeNative/Sources/CodeNativeApp`.

## Generate the project

```bash
cd native/CodeNativeMac
xcodegen generate
```

This creates `CodeNativeMac.xcodeproj`.

## Run in Xcode

1. Open `native/CodeNativeMac/CodeNativeMac.xcodeproj`.
2. Pick `My Mac`.
3. Run the `CodeNativeMac` scheme.

By default, Every Code Companion connects to `ws://127.0.0.1:4317/ws`.

## TestFlight workflow

Use `.github/workflows/native-macos-testflight.yml` to archive, export, and
upload a signed macOS build to TestFlight.
