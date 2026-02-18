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

Before running locally, build the fork backend once from repo root:

```bash
./build-fast.sh
```

1. Open `native/CodeNativeMac/CodeNativeMac.xcodeproj`.
2. Pick `My Mac`.
3. Run the `CodeNativeMac` scheme.

The target now bundles a managed backend binary into
`Contents/Resources/backend/code` and launches it automatically on a random
loopback port. If you need an explicit binary, set
`CODE_NATIVE_BACKEND_BINARY` in the scheme environment.

## TestFlight workflow

Use `.github/workflows/native-macos-testflight.yml` to archive, export, and
upload a signed macOS build to TestFlight.
