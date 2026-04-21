# Native macOS Swift App

This package is the native macOS replacement for the Tauri shell in this branch.

## Goals

- Keep the existing user data layout under `~/Library/Application Support/com.light-whisper.desktop/`
- Keep online-ASR behavior compatible with the current mac branch
- Use native AppKit/SwiftUI windowing, especially for fullscreen subtitle overlay
- Package a signed `.app` bundle without depending on Tauri

## Structure

- `Sources/LightWhisperNativeApp/`: native app sources
- `Tests/LightWhisperNativeAppTests/`: Swift Testing compatibility coverage
- `Bundle/Info.plist`: bundle metadata and macOS permission usage strings
- `scripts/build-native-app.sh`: builds the Swift package and wraps it into a `.app`

## Commands

```bash
swift build --package-path native-macos
swift test --package-path native-macos
native-macos/scripts/build-native-app.sh
```
