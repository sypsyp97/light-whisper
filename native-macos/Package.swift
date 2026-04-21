// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "LightWhisperNativeApp",
    platforms: [
        .macOS(.v14),
    ],
    products: [
        .executable(
            name: "LightWhisperNativeApp",
            targets: ["LightWhisperNativeApp"]
        ),
    ],
    targets: [
        .executableTarget(
            name: "LightWhisperNativeApp",
            path: "Sources/LightWhisperNativeApp"
        ),
        .testTarget(
            name: "LightWhisperNativeAppTests",
            dependencies: ["LightWhisperNativeApp"],
            path: "Tests/LightWhisperNativeAppTests"
        ),
    ]
)
