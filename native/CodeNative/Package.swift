// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "CodeNative",
    platforms: [
        .macOS(.v14),
    ],
    products: [
        .executable(
            name: "CodeNativeApp",
            targets: ["CodeNativeApp"]
        ),
    ],
    targets: [
        .executableTarget(
            name: "CodeNativeApp",
            path: "Sources/CodeNativeApp"
        ),
    ]
)
