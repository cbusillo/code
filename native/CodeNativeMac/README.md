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

## Optional CLI shims

Install `ecc` (and optionally `code`) wrappers that execute the bundled backend:

```bash
scripts/install-ecc-shims.sh
```

Also install `code` shim (opt-in):

```bash
scripts/install-ecc-shims.sh --install-code
```

Run compatibility smoke checks for core CLI flows:

```bash
scripts/cli-compat-smoke.sh --code ~/.local/bin/ecc
scripts/cli-compat-smoke.sh --code ~/.local/bin/code --ecc ~/.local/bin/ecc
```

## TestFlight workflow

Use `.github/workflows/native-macos-testflight.yml` to archive, export, and
upload a signed macOS build to TestFlight.
