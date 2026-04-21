import Foundation
import Testing
@testable import LightWhisperNativeApp

@Suite("Native Parity Regressions")
struct NativeParityRegressionTests {
    @Test
    func settingsViewFlushesPendingEditsWhenWindowDisappears() throws {
        let fixture = try NativeParityFixture()
        let settingsSource = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/App/Views/SettingsView.swift"
        )

        #expect(
            settingsSource.contains(".onDisappear"),
            "SettingsView should flush pending edits when the Settings window disappears so text-field changes persist without pressing Return."
        )
    }

    @Test
    func inputDeviceSelectionExposesSystemDefaultChoice() throws {
        let fixture = try NativeParityFixture()
        let settingsSource = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/App/Views/SettingsView.swift"
        )
        let appModelSource = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/App/AppModel.swift"
        )

        let appModelSupportsNilSelection =
            appModelSource.contains("func selectInputDevice(uid: String?)")
            && appModelSource.contains("Following system default input")
        let pickerExposesSystemDefaultChoice =
            settingsSource.localizedCaseInsensitiveContains("system default")
            || settingsSource.localizedCaseInsensitiveContains("follow default")
            || settingsSource.containsRegex(#"\.tag\(\s*nil\s+as\s+String\?\s*\)"#)
            || settingsSource.contains("selectInputDevice(uid: nil)")
        let rejectsEmptySelection = settingsSource.contains("guard !$0.isEmpty else { return }")

        #expect(
            appModelSupportsNilSelection,
            "AppModel should continue to support a nil input-device UID for following the system default device."
        )
        #expect(
            pickerExposesSystemDefaultChoice && !rejectsEmptySelection,
            "SettingsView should expose a system-default input option instead of requiring a concrete device UID."
        )
    }

    @Test
    func appModelExposesProfileHotWordAndCorrectionMaintenanceActions() throws {
        let fixture = try NativeParityFixture()
        let appModelSource = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/App/AppModel.swift"
        )

        let hasAddHotWordWorkflow =
            appModelSource.containsIdentifier("addHotWord")
            && appModelSource.contains("ProfileManagementService.addHotWord")
        let hasRemoveHotWordWorkflow =
            appModelSource.containsIdentifier("removeHotWord")
            && appModelSource.contains("ProfileManagementService.removeHotWord")
        let hasRemoveCorrectionWorkflow =
            appModelSource.containsIdentifier("removeCorrection")
            && appModelSource.contains("ProfileManagementService.removeCorrection")

        #expect(
            hasAddHotWordWorkflow,
            "AppModel should expose an add-hot-word workflow that delegates to ProfileManagementService and persists the updated user profile."
        )
        #expect(
            hasRemoveHotWordWorkflow,
            "AppModel should expose a remove-hot-word workflow that delegates to ProfileManagementService and persists the updated user profile."
        )
        #expect(
            hasRemoveCorrectionWorkflow,
            "AppModel should expose a correction-removal workflow that delegates to ProfileManagementService and persists the updated user profile."
        )
    }

    @Test
    func menuBarUpdateActionPrefersOpeningKnownReleasePage() throws {
        let fixture = try NativeParityFixture()
        let statusBarSource = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/Platform/StatusBarController.swift"
        )
        let appModelSource = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/App/AppModel.swift"
        )

        let menuReflectsKnownUpdate = statusBarSource.contains("Install Available Update")
        let actionOpensReleasePage =
            appModelSource.contains("case .checkForUpdates:")
            && appModelSource.contains("UpdaterService.openReleasePage")
            && appModelSource.containsIdentifier("updateInfo")

        #expect(
            menuReflectsKnownUpdate,
            "StatusBarController should continue to relabel the menu item once an update is already known."
        )
        #expect(
            actionOpensReleasePage,
            "When an update is already known, the menu-bar action should open the release page instead of re-running update discovery."
        )
    }

    @Test
    func mainWindowReopenPathCanRequestFreshScene() throws {
        let fixture = try NativeParityFixture()
        let appSource = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/App/LightWhisperNativeApp.swift"
        )
        let appModelSource = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/App/AppModel.swift"
        )

        let definesAddressableMainScene =
            appSource.containsRegex(#"WindowGroup\s*\(\s*"[^"]+"\s*,\s*id\s*:"#)
            || appSource.containsRegex(#"WindowGroup\s*\(\s*id\s*:"#)
            || appSource.containsRegex(#"Window\s*\(\s*"[^"]+"\s*,\s*id\s*:"#)
        let canRequestFreshScene =
            appSource.contains("openWindow(")
            || appModelSource.contains("openWindow(")
            || appModelSource.contains("newWindowForTab:")
            || appModelSource.contains("openMainWindowScene")

        #expect(
            definesAddressableMainScene,
            "The native app should define an addressable main scene so reopening can create a fresh main window when none exists."
        )
        #expect(
            canRequestFreshScene,
            "The main-window reopen path should include an explicit fresh-scene request instead of relying only on existing NSApp windows."
        )
    }

    @Test
    func screenContextTogglesRequireConcreteNativePayloads() throws {
        let fixture = try NativeParityFixture()
        let coordinatorSource = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/App/DictationCoordinator.swift"
        )
        let assistantSource = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/Networking/AssistantService.swift"
        )
        let polishSource = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/Networking/AIPolishService.swift"
        )

        let assistantToggleIsWired =
            coordinatorSource.contains("assistantScreenContextEnabled")
            && !coordinatorSource.contains("includeScreenContext: false")
        let requestPayloadCarriesConcreteScreenContext =
            assistantSource.containsIdentifier("screenContext")
            || assistantSource.containsIdentifier("screenContextPath")
            || assistantSource.containsIdentifier("capturedScreenContext")
            || polishSource.containsIdentifier("screenContext")
            || polishSource.containsIdentifier("screenContextPath")
            || polishSource.containsIdentifier("capturedScreenContext")
        let hasNativeScreenCapturePath = fixture.sourceIndex.anyDocumentContains(
            any: [
                "CGDisplayCreateImage",
                "CGWindowListCreateImage",
                "ScreenCaptureKit",
                "SCScreenshotManager",
                "SCShareableContent",
            ]
        )

        #expect(
            assistantToggleIsWired,
            "Assistant requests should honor the assistant screen-context toggle instead of hard-coding includeScreenContext to false."
        )
        #expect(
            requestPayloadCarriesConcreteScreenContext || hasNativeScreenCapturePath,
            "Screen-context toggles need either a real native capture path or a concrete screen-context payload instead of a placeholder boolean-only request."
        )
    }

    @Test
    func nativeBundleVersionMatchesRepoMetadata() throws {
        let fixture = try NativeParityFixture()
        let packageJSON = try fixture.repoContents(of: "package.json")
        let cargoToml = try fixture.repoContents(of: "src-tauri/Cargo.toml")
        let infoPlist = try fixture.packageContents(of: "Bundle/Info.plist")

        let repoVersion = try fixture.packageVersion(from: packageJSON)
        let cargoVersion = try fixture.cargoPackageVersion(from: cargoToml)
        let plistVersion = try fixture.plistStringValue(
            forKey: "CFBundleShortVersionString",
            from: infoPlist
        )
        let plistBuild = try fixture.plistStringValue(
            forKey: "CFBundleVersion",
            from: infoPlist
        )

        #expect(cargoVersion == repoVersion)
        #expect(plistVersion == repoVersion)
        #expect(plistBuild == repoVersion)
    }

    @Test
    func nativePackagingDerivesVersionAndFailsLoudlyWhenAssetsOrSigningAreMissing() throws {
        let fixture = try NativeParityFixture()
        let buildScript = try fixture.packageContents(of: "scripts/build-native-app.sh")

        let derivesVersionFromRepoMetadata =
            buildScript.contains("package.json")
            || buildScript.contains("Cargo.toml")
            || buildScript.contains("tauri.conf")
            || buildScript.contains("CFBundleShortVersionString")
            || buildScript.contains("CFBundleVersion")
        let swallowsCodesignFailure =
            buildScript.contains("codesign")
            && buildScript.contains("|| true")
        let failsLoudlyForRequiredAssets =
            buildScript.localizedCaseInsensitiveContains("missing icon")
            || buildScript.localizedCaseInsensitiveContains("required icon")
            || buildScript.localizedCaseInsensitiveContains("missing signing")
            || buildScript.localizedCaseInsensitiveContains("required signing")
            || buildScript.contains("exit 1")

        #expect(
            derivesVersionFromRepoMetadata,
            "Native packaging should derive the app version from repo/package metadata instead of relying on a copied static Info.plist version."
        )
        #expect(
            !swallowsCodesignFailure,
            "Native packaging should fail loudly when signing is required but codesign fails."
        )
        #expect(
            failsLoudlyForRequiredAssets,
            "Native packaging should fail loudly when required assets or signing inputs are missing instead of silently continuing."
        )
    }
}

private struct NativeParityFixture {
    let packageRoot: URL
    let repoRoot: URL
    let sourceIndex: NativeSourceIndex

    init() throws {
        packageRoot = Self.packageRoot()
        repoRoot = packageRoot.deletingLastPathComponent()
        sourceIndex = try NativeSourceIndex(packageRoot: packageRoot)
    }

    func packageContents(of relativePath: String) throws -> String {
        try String(
            contentsOf: packageRoot.appendingPathComponent(relativePath, isDirectory: false),
            encoding: .utf8
        )
    }

    func repoContents(of relativePath: String) throws -> String {
        try String(
            contentsOf: repoRoot.appendingPathComponent(relativePath, isDirectory: false),
            encoding: .utf8
        )
    }

    func packageVersion(from json: String) throws -> String {
        let data = Data(json.utf8)
        let object = try JSONSerialization.jsonObject(with: data, options: [])
        guard let dictionary = object as? [String: Any],
              let version = dictionary["version"] as? String,
              !version.isEmpty else {
            throw NSError(
                domain: "NativeParityFixture",
                code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Missing package.json version"]
            )
        }
        return version
    }

    func cargoPackageVersion(from toml: String) throws -> String {
        var inPackageSection = false
        for rawLine in toml.components(separatedBy: .newlines) {
            let line = rawLine.trimmingCharacters(in: .whitespacesAndNewlines)
            if line.hasPrefix("[") && line.hasSuffix("]") {
                inPackageSection = (line == "[package]")
                continue
            }
            guard inPackageSection else { continue }
            if let version = line.firstMatch(for: #"^version\s*=\s*"([^"]+)""#) {
                return version
            }
        }
        throw NSError(
            domain: "NativeParityFixture",
            code: 2,
            userInfo: [NSLocalizedDescriptionKey: "Missing Cargo package version"]
        )
    }

    func plistStringValue(forKey key: String, from plist: String) throws -> String {
        let data = Data(plist.utf8)
        let object = try PropertyListSerialization.propertyList(from: data, format: nil)
        guard let dictionary = object as? [String: Any],
              let value = dictionary[key] as? String,
              !value.isEmpty else {
            throw NSError(
                domain: "NativeParityFixture",
                code: 3,
                userInfo: [NSLocalizedDescriptionKey: "Missing plist key \(key)"]
            )
        }
        return value
    }

    private static func packageRoot() -> URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
    }
}

private struct NativeSourceIndex {
    private struct SourceDocument {
        let contents: String
    }

    private let documents: [SourceDocument]

    init(packageRoot: URL) throws {
        let sourceRoot = packageRoot.appendingPathComponent("Sources/LightWhisperNativeApp", isDirectory: true)
        let enumerator = FileManager.default.enumerator(
            at: sourceRoot,
            includingPropertiesForKeys: nil
        )

        var loaded: [SourceDocument] = []
        while let fileURL = enumerator?.nextObject() as? URL {
            guard fileURL.pathExtension == "swift" else { continue }
            let contents = try String(contentsOf: fileURL, encoding: .utf8)
            loaded.append(SourceDocument(contents: contents))
        }

        documents = loaded
    }

    func anyDocumentContains(any snippets: [String]) -> Bool {
        documents.contains { document in
            snippets.contains { snippet in
                document.contents.localizedCaseInsensitiveContains(snippet)
            }
        }
    }
}

private extension String {
    func containsRegex(_ pattern: String) -> Bool {
        range(of: pattern, options: .regularExpression) != nil
    }

    func containsIdentifier(_ identifier: String) -> Bool {
        containsRegex(#"(?<![A-Za-z0-9_])\#(NSRegularExpression.escapedPattern(for: identifier))(?![A-Za-z0-9_])"#)
    }

    func firstMatch(for pattern: String) -> String? {
        guard let regex = try? NSRegularExpression(pattern: pattern) else {
            return nil
        }
        let searchRange = NSRange(startIndex..<endIndex, in: self)
        guard let match = regex.firstMatch(in: self, range: searchRange),
              match.numberOfRanges > 1,
              let range = Range(match.range(at: 1), in: self) else {
            return nil
        }
        return String(self[range])
    }
}
