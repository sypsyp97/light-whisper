import Foundation
import Testing
@testable import LightWhisperNativeApp

@Suite("Review Audit Fixes")
struct ReviewAuditFixTests {
    @Test
    func legacyMigrationMergesLegacySupportIntoExistingCurrentDirectory() throws {
        let temporaryDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: temporaryDirectory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: temporaryDirectory) }

        let currentDirectory = temporaryDirectory.appendingPathComponent("com.light-whisper.desktop", isDirectory: true)
        let legacyDirectory = temporaryDirectory.appendingPathComponent("com.light-whisper.app", isDirectory: true)
        try FileManager.default.createDirectory(at: currentDirectory, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: legacyDirectory, withIntermediateDirectories: true)

        let currentEngine = currentDirectory.appendingPathComponent("engine.json", isDirectory: false)
        let legacyProfile = legacyDirectory.appendingPathComponent("user_profile.json", isDirectory: false)
        try #"{"engine":"alibaba-asr"}"#.write(to: currentEngine, atomically: true, encoding: .utf8)
        try #"{"dictation_hotkey":"f2"}"#.write(to: legacyProfile, atomically: true, encoding: .utf8)

        let didFullyMigrate = try AppPaths.migrateLegacySupportDirectoryIfNeeded(
            appDirectory: currentDirectory,
            legacyDirectory: legacyDirectory,
            fileManager: .default
        )

        #expect(didFullyMigrate)
        #expect(FileManager.default.fileExists(atPath: currentEngine.path))
        #expect(FileManager.default.fileExists(atPath: currentDirectory.appendingPathComponent("user_profile.json").path))
        #expect(!FileManager.default.fileExists(atPath: legacyProfile.path))
        #expect(!FileManager.default.fileExists(atPath: legacyDirectory.path))
    }

    @Test
    func buildScriptRequiresStableCodesignIdentityAndSignsRuntimeArtifacts() throws {
        let fixture = try ReviewAuditFixture()
        let buildScript = try fixture.packageContents(of: "scripts/build-native-app.sh")

        #expect(
            !buildScript.contains(#"${CODESIGN_IDENTITY:--}"#),
            "Release packaging should not default to ad-hoc signing."
        )
        #expect(
            buildScript.localizedCaseInsensitiveContains("CODESIGN_IDENTITY is required")
                || buildScript.localizedCaseInsensitiveContains("missing required codesign identity"),
            "Packaging should fail loudly when no stable signing identity is provided."
        )
        #expect(buildScript.contains("--options runtime"))
        #expect(buildScript.contains("--timestamp"))
        #expect(buildScript.contains("Developer ID Application"))
        #expect(buildScript.contains("Applications"))
        #expect(
            buildScript.contains(#"codesign --force --sign "$CODESIGN_IDENTITY" "$DMG_PATH""#)
                || buildScript.contains(#"codesign --force --timestamp --sign "$CODESIGN_IDENTITY" "$DMG_PATH""#),
            "The built DMG should be signed as part of release packaging."
        )
    }

    @Test
    func accessibilityPromptUsesTrustedCheckOptionsPrompt() throws {
        let fixture = try ReviewAuditFixture()
        let source = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/Platform/PermissionsService.swift"
        )

        #expect(source.contains("AXIsProcessTrustedWithOptions"))
        #expect(source.contains("kAXTrustedCheckOptionPrompt"))
    }

    @Test
    func showMainWindowTracksConcreteMainWindowReference() throws {
        let fixture = try ReviewAuditFixture()
        let appModelSource = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/App/AppModel.swift"
        )
        let contentSource = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/App/Views/ContentView.swift"
        )

        #expect(
            appModelSource.contains("mainWindowReference")
                || appModelSource.contains("bindMainWindow(")
                || contentSource.contains("WindowAccessor("),
            "Main-window reopen should use an explicit main-window reference instead of scanning every NSApp window."
        )
        #expect(
            !appModelSource.contains("NSApp.windows.first(where: { !($0 is NSPanel) })"),
            "Show Main Window should not treat the first non-panel window as the main window."
        )
    }

    @Test
    func settingsPersistApiKeysAsTheyChangeBeforeProviderSwitches() throws {
        let fixture = try ReviewAuditFixture()
        let settingsSource = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/App/Views/SettingsView.swift"
        )
        let appModelSource = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/App/AppModel.swift"
        )

        #expect(settingsSource.contains(".onChange(of: model.onlineASRAPIKey)"))
        #expect(settingsSource.contains(".onChange(of: model.aiPolishAPIKey)"))
        #expect(settingsSource.contains(".onChange(of: model.assistantAPIKey)"))
        #expect(settingsSource.contains(".onChange(of: model.webSearchAPIKey)"))
        #expect(
            !appModelSource.contains("persistEngineSettings() {\n        do {\n            let store = try JSONFileStore<EngineSettings>(url: AppPaths.engineSettingsURL())\n            try store.save(engineSettings)\n            loadOnlineASRAPIKey()"),
            "Persisting settings should not immediately reload and clobber unsaved credential edits."
        )
    }

    @Test
    func subtitleOverlayTargetsFrontmostScreenInsteadOfOnlyNSScreenMain() throws {
        let fixture = try ReviewAuditFixture()
        let source = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/Platform/SubtitlePanelController.swift"
        )

        #expect(
            source.contains("frontmostScreen")
                || source.contains("NSWorkspace.shared.frontmostApplication")
                || source.contains("NSEvent.mouseLocation"),
            "Subtitle overlay layout should resolve the active/frontmost screen before positioning the panel."
        )
    }

    @Test
    func cleanupScriptCanAlsoResetCurrentBundlePermissionsWhenRequested() throws {
        let fixture = try ReviewAuditFixture()
        let script = try fixture.packageContents(of: "scripts/clear-legacy-permissions.sh")

        #expect(script.contains("com.light-whisper.desktop"))
        #expect(
            script.contains("--include-current")
                || script.contains("RESET_CURRENT_TCC")
                || script.contains("CURRENT_IDENTIFIER"),
            "The repo should provide an explicit way to clear current-bundle TCC state during migration testing."
        )
    }

    @Test
    func localInstallScriptBuildsAUsableAppForThisMachine() throws {
        let fixture = try ReviewAuditFixture()
        let script = try fixture.packageContents(of: "scripts/install-local-app.sh")

        #expect(
            script.contains("Apple Development")
                || script.contains("find-identity")
                || script.contains("security find-identity"),
            "Local install should support a machine-local signing identity instead of requiring Developer ID distribution signing."
        )
        #expect(
            script.contains("$HOME/Applications")
                || script.contains("${HOME}/Applications")
                || script.contains("TARGET_DIR=\"$HOME/Applications\""),
            "Local install should place the app into the user's Applications folder."
        )
        #expect(
            script.contains("open \"$TARGET_APP\"")
                || script.contains("open \"$INSTALL_PATH\"")
                || script.contains("open \"$APP_TARGET\""),
            "Local install should launch the installed app after deployment."
        )
    }

    @Test
    func nativeSettingsEditsNamedCustomProvidersInPlace() throws {
        let fixture = try ReviewAuditFixture()
        let settingsSource = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/App/Views/SettingsView.swift"
        )

        #expect(
            settingsSource.contains("ForEach(model.userProfile.llmProvider.customProviders)"),
            "Native settings should include named custom providers in provider pickers."
        )
        #expect(
            settingsSource.contains("customProviders[index].baseURL = value"),
            "Editing Base URL for a named custom provider should update customProviders[index].baseURL, which is the field used by endpoint resolution."
        )
        #expect(
            settingsSource.contains("customProviders[index].model = value"),
            "Editing Model for a named custom provider should update customProviders[index].model instead of the legacy global custom model field."
        )
    }

    @Test
    func nativeAssistantProviderSelectionTogglesSeparateModelState() throws {
        let fixture = try ReviewAuditFixture()
        let settingsSource = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/App/Views/SettingsView.swift"
        )

        #expect(settingsSource.contains("followActiveAssistantProviderTag"))
        #expect(
            settingsSource.contains("assistantUseSeparateModel = false"),
            "Choosing Use Active Provider should explicitly disable assistant separate-model resolution."
        )
        #expect(
            settingsSource.contains("assistantUseSeparateModel = true"),
            "Choosing a concrete assistant provider should enable assistant separate-model resolution."
        )
        #expect(
            settingsSource.contains("model.persistAssistantAPIKey()"),
            "Changing assistant provider should save the currently typed assistant key under the old provider before switching."
        )
    }

    @Test
    func nativeProviderSwitchesReloadCredentialFieldsAfterChangingProvider() throws {
        let fixture = try ReviewAuditFixture()
        let settingsSource = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/App/Views/SettingsView.swift"
        )
        let appModelSource = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/App/AppModel.swift"
        )

        #expect(appModelSource.contains("func loadOnlineASRAPIKey()"))
        #expect(appModelSource.contains("func loadAIPolishAPIKey()"))
        #expect(appModelSource.contains("func loadAssistantAPIKey()"))
        #expect(
            settingsSource.contains("model.loadOnlineASRAPIKey()"),
            "Switching ASR engine or region should load the key for the new keychain account before the settings window can flush again."
        )
        #expect(
            settingsSource.contains("model.loadAIPolishAPIKey()"),
            "Switching the active LLM provider should load the key for the new provider to avoid writing the old provider's key on window close."
        )
        #expect(
            settingsSource.contains("model.loadAssistantAPIKey()"),
            "Switching assistant provider should load the key for the new assistant provider to avoid stale-key writes on window close."
        )
    }

    @Test
    func nativeAssistantKeyDoesNotOverwriteSharedProviderKeyWhenProvidersShareKeychainUser() throws {
        let fixture = try ReviewAuditFixture()
        let appModelSource = try fixture.packageContents(
            of: "Sources/LightWhisperNativeApp/App/AppModel.swift"
        )
        let persistAssistantSegment = try fixture.segment(
            in: appModelSource,
            from: "func persistAssistantAPIKey()",
            to: "func loadAssistantAPIKey()"
        )

        #expect(
            persistAssistantSegment.contains("guard !assistantUsesActiveProviderKeychainUser() else")
                && persistAssistantSegment.contains("return"),
            "When assistant and polish resolve to the same keychain account, persisting the assistant key should not write a second value to that shared account."
        )
        #expect(
            appModelSource.contains("private func assistantUsesActiveProviderKeychainUser()")
                && appModelSource.contains("activeUser == assistantUser"),
            "Shared-key detection should compare the resolved keychain accounts, not just the separate-model toggle."
        )
        #expect(
            appModelSource.contains("assistantAPIKey = aiPolishAPIKey"),
            "When assistant shares the active provider key, the assistant field should mirror the active AI polish key instead of keeping stale text."
        )
    }
}

private struct ReviewAuditFixture {
    let packageRoot: URL

    init() {
        packageRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
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
        guard let endRange = source[startRange.upperBound...].range(of: end) else {
            throw NSError(
                domain: "ReviewAuditFixture",
                code: 2,
                userInfo: [NSLocalizedDescriptionKey: "Missing end marker: \(end)"]
            )
        }
        return String(source[startRange.lowerBound..<endRange.lowerBound])
    }
}
