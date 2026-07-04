import Foundation
import Testing
@testable import LightWhisperNativeApp

@Suite("Review Audit Regressions")
struct ReviewAuditRegressionTests {
    @Test
    func legacyMigrationDoesNotDeleteLegacyStateBeforeMigrationSucceeds() throws {
        let fixture = try ReviewAuditFixture()
        let source = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/Core/AppPaths.swift"
        )
        let dataDirectorySegment = try fixture.segment(
            in: source,
            from: "public static func dataDirectory",
            to: "public static let dataDirectoryURL"
        )

        let moveFailureIsHandledExplicitly = !dataDirectorySegment.contains("try? fileManager.moveItem")
        let existingDestinationHasDedicatedLegacyHandling =
            dataDirectorySegment.containsRegex(#"if\s+fileManager\.fileExists\(atPath:\s*appDirectory\.path\)[\s\S]{0,400}legacyDirectory"#)
            || dataDirectorySegment.containsIdentifier("mergeLegacy")
            || dataDirectorySegment.containsIdentifier("copyLegacy")
        let legacyCleanupIsNotUnconditional =
            !dataDirectorySegment.containsRegex(#"createDirectory\(at:\s*appDirectory[\s\S]{0,120}cleanupLegacyState\(fileManager:\s*fileManager\)"#)

        #expect(
            moveFailureIsHandledExplicitly,
            "Legacy-state migration should not swallow move failures with `try?`; failed migrations must keep the legacy data intact for retry."
        )
        #expect(
            existingDestinationHasDedicatedLegacyHandling,
            "When the new app directory already exists, AppPaths should still have an explicit legacy-state preservation or merge path instead of skipping migration entirely."
        )
        #expect(
            legacyCleanupIsNotUnconditional,
            "Legacy cleanup should be gated on successful migration instead of always running after directory creation."
        )
    }

    @Test
    func releasePackagingRequiresExplicitSigningInsteadOfAdHocDefault() throws {
        let fixture = try ReviewAuditFixture()
        let script = try fixture.packageContents(of: "scripts/build-native-app.sh")

        let doesNotDefaultToAdHoc =
            !script.contains(#"CODESIGN_IDENTITY="${CODESIGN_IDENTITY:--}""#)
            && !script.contains(#"CODESIGN_IDENTITY="${CODESIGN_IDENTITY:-"-"}""#)
        let surfacesSigningRequirement =
            script.contains("${CODESIGN_IDENTITY:?")
            || script.containsRegex(#"if\s+\[\[\s+-z\s+\"\$CODESIGN_IDENTITY\"\s+\]\]"#)
            || script.localizedCaseInsensitiveContains("missing signing")
            || script.localizedCaseInsensitiveContains("codesign identity")

        #expect(
            doesNotDefaultToAdHoc,
            "Release packaging should not silently fall back to ad-hoc signing."
        )
        #expect(
            surfacesSigningRequirement,
            "The native packaging script should fail with a concrete signing requirement instead of producing an ad-hoc-signed release build."
        )
    }

    @Test
    func accessibilityFlowUsesConcreteSystemTrustPromptPath() throws {
        let fixture = try ReviewAuditFixture()
        let source = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/Platform/PermissionsService.swift"
        )

        let usesAXPromptAPI = source.contains("AXIsProcessTrustedWithOptions")
        let setsPromptOption =
            source.contains("kAXTrustedCheckOptionPrompt")
            || source.containsRegex(#"\[\s*.*prompt.*:\s*true"#)

        #expect(
            usesAXPromptAPI,
            "Accessibility permission checks should use AXIsProcessTrustedWithOptions so the app can trigger the native trust prompt on clean installs."
        )
        #expect(
            setsPromptOption,
            "Accessibility permission checks should set the prompt option explicitly instead of only deep-linking System Settings."
        )
    }

    @Test
    func showMainWindowTargetsDedicatedMainWindowInsteadOfAnyNonPanelWindow() throws {
        let fixture = try ReviewAuditFixture()
        let source = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/App/AppModel.swift"
        )
        let segment = try fixture.segment(
            in: source,
            from: "private func showMainWindow()",
            to: "private func showSettingsWindow()"
        )

        let doesNotGrabArbitraryWindow =
            !segment.contains("NSApp.windows.first(where: { !($0 is NSPanel) })")
        let targetsDedicatedMainWindow =
            segment.contains("mainWindowSceneID")
            || segment.contains(".identifier")
            || segment.contains(".windowNumber")
            || segment.contains("openMainWindowScene()")
                && segment.containsRegex(#"if\s+let\s+window\s*=.*main"#)

        #expect(
            doesNotGrabArbitraryWindow,
            "Show Main Window should not reopen whichever non-panel window happens to be first, because that can focus Settings instead of the main window."
        )
        #expect(
            targetsDedicatedMainWindow,
            "Show Main Window should distinguish the dedicated main window from settings and auxiliary scenes."
        )
    }

    @Test
    func switchingEngineOrProvidersFlushesUnsavedAPIKeyEditsFirst() throws {
        let fixture = try ReviewAuditFixture()
        let source = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/App/Views/SettingsView.swift"
        )
        let engineSegment = try fixture.segment(
            in: source,
            from: #"Picker("Engine""#,
            to: "if model.engineSettings.engine == .alibabaAsr"
        )
        let providerSegment = try fixture.segment(
            in: source,
            from: #"Picker("Provider""#,
            to: "if activeProviderSupportsBaseURL"
        )
        let assistantProviderSegment = try fixture.segment(
            in: source,
            from: #"Picker("Assistant Provider", selection:"#,
            to: #"Toggle("Screen Context""#
        )

        let engineChangeFlushesPendingASRKey =
            engineSegment.contains("flushPendingChanges()")
            || (
                engineSegment.contains("model.persistOnlineASRAPIKey()")
                    && engineSegment.contains("model.persistEngineSettings()")
                    && engineSegment.contains("model.loadOnlineASRAPIKey()")
            )
        let providerChangeFlushesPendingLLMKeys =
            providerSegment.contains("flushPendingChanges()")
            || (
                providerSegment.contains("model.persistAIPolishAPIKey()")
                    && providerSegment.contains("model.persistAssistantAPIKey()")
                    && providerSegment.contains("model.persistUserProfile()")
                    && providerSegment.contains("model.loadAIPolishAPIKey()")
                    && providerSegment.contains("model.loadAssistantAPIKey()")
            )
        let assistantProviderChangeFlushesAssistantKey =
            assistantProviderSegment.contains("flushPendingChanges()")
            || (
                assistantProviderSegment.contains("model.persistAssistantAPIKey()")
                    && assistantProviderSegment.contains("model.persistUserProfile()")
                    && assistantProviderSegment.contains("model.loadAssistantAPIKey()")
            )

        #expect(
            engineChangeFlushesPendingASRKey,
            "Switching the speech engine should flush any in-progress Online ASR API key edit before reloading the engine-specific key from Keychain."
        )
        #expect(
            providerChangeFlushesPendingLLMKeys,
            "Switching the active AI provider should flush pending AI Polish and Assistant API key edits before the provider change reloads keys from Keychain."
        )
        #expect(
            assistantProviderChangeFlushesAssistantKey,
            "Switching the assistant provider should flush any in-progress Assistant API key edit before the provider change reloads keys from Keychain."
        )
    }

    @Test
    func subtitleOverlayLayoutUsesActiveScreenSignalsInsteadOfMainScreenOnly() throws {
        let fixture = try ReviewAuditFixture()
        let source = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/Platform/SubtitlePanelController.swift"
        )
        let layoutSegment = try fixture.segment(
            in: source,
            from: "private func layout(_ panel: NSPanel)",
            to: "private func copyCurrentText()"
        )

        let notMainScreenOnly = !layoutSegment.contains("NSScreen.main ?? NSScreen.screens.first")
        let usesActiveScreenSignal =
            layoutSegment.contains("panel.screen")
            || layoutSegment.contains("NSApp.keyWindow?.screen")
            || layoutSegment.contains("NSEvent.mouseLocation")
            || layoutSegment.contains("CGWindowListCopyWindowInfo")
            || layoutSegment.localizedCaseInsensitiveContains("frontmost")

        #expect(
            notMainScreenOnly,
            "Subtitle overlay layout should not rely on NSScreen.main alone, because fullscreen apps can be on another display."
        )
        #expect(
            usesActiveScreenSignal,
            "Subtitle overlay layout should pick the frontmost or active display using a concrete screen signal."
        )
    }

    @Test
    func subtitleOverlayReinforcesFullscreenPanelVisibility() throws {
        let fixture = try ReviewAuditFixture()
        let source = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/Platform/SubtitlePanelController.swift"
        )
        let windowingSegment = try fixture.segment(
            in: source,
            from: "private func reinforcePanelWindowing(_ panel: NSPanel)",
            to: "private func layout(_ panel: NSPanel)"
        )

        #expect(windowingSegment.contains("panel.level = .screenSaver"))
        #expect(windowingSegment.contains("panel.hidesOnDeactivate = false"))
        #expect(windowingSegment.contains("panel.canHide = false"))
        #expect(windowingSegment.contains(".canJoinAllSpaces"))
        #expect(windowingSegment.contains(".fullScreenAuxiliary"))
        #expect(windowingSegment.contains(".transient"))
        #expect(source.contains("reinforcePanelWindowing(panel)\n        layout(panel)"))
    }

    @Test
    func cleanupScriptCoversCurrentBundleIdentifierToo() throws {
        let fixture = try ReviewAuditFixture()
        let script = try fixture.packageContents(of: "scripts/clear-legacy-permissions.sh")

        let namesCurrentBundleIdentifier = script.contains("com.light-whisper.desktop")
        let resetsCurrentBundleTCC =
            script.containsRegex(#"tccutil reset All .*com\.light-whisper\.desktop"#)
            || script.contains(#"tccutil reset All "$CURRENT_IDENTIFIER""#)
        let removesCurrentBundleLocalState =
            script.containsRegex(#"Application Support/.*com\.light-whisper\.desktop"#)
            || script.containsRegex(#"Caches/.*com\.light-whisper\.desktop"#)
            || script.containsRegex(#"Preferences/.*com\.light-whisper\.desktop"#)

        #expect(
            namesCurrentBundleIdentifier,
            "The cleanup helper should explicitly cover the current native bundle identifier as well as the legacy bundle id."
        )
        #expect(
            resetsCurrentBundleTCC,
            "The cleanup helper should be able to reset TCC state for the current app bundle identifier."
        )
        #expect(
            removesCurrentBundleLocalState,
            "The cleanup helper should be able to remove current-app local state when reinstall and migration testing needs a clean slate."
        )
    }
}

private struct ReviewAuditFixture {
    let packageRoot: URL

    init() throws {
        packageRoot = Self.packageRoot()
    }

    func packageContents(of relativePath: String) throws -> String {
        try String(
            contentsOf: packageRoot.appendingPathComponent(relativePath, isDirectory: false),
            encoding: .utf8
        )
    }

    func segment(in source: String, from start: String, to end: String) throws -> String {
        guard let startRange = source.range(of: start) else {
            throw NSError(
                domain: "ReviewAuditFixture",
                code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Missing start marker: \(start)"]
            )
        }
        guard let endRange = source.range(of: end, range: startRange.upperBound..<source.endIndex) else {
            throw NSError(
                domain: "ReviewAuditFixture",
                code: 2,
                userInfo: [NSLocalizedDescriptionKey: "Missing end marker: \(end)"]
            )
        }
        return String(source[startRange.lowerBound..<endRange.lowerBound])
    }

    private static func packageRoot() -> URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
    }
}

private extension String {
    func containsRegex(_ pattern: String) -> Bool {
        range(of: pattern, options: .regularExpression) != nil
    }

    func containsIdentifier(_ identifier: String) -> Bool {
        containsRegex(#"(?<![A-Za-z0-9_])\#(NSRegularExpression.escapedPattern(for: identifier))(?![A-Za-z0-9_])"#)
    }
}
