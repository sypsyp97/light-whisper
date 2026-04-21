import Foundation
import Testing
@testable import LightWhisperNativeApp

@Suite("Advanced Parity")
struct AdvancedParityTests {
    @Test
    func openAIAuthModeRoundTripsThroughUserProfilePersistence() throws {
        let temporaryDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: temporaryDirectory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: temporaryDirectory) }

        var profile = UserProfile.defaultValue()
        profile.llmProvider.active = "openai"
        profile.llmProvider.openAIAuthMode = .oauth
        profile.llmProvider.openAIFastMode = true

        let url = temporaryDirectory.appendingPathComponent("user_profile.json", isDirectory: false)
        let store = JSONFileStore<UserProfile>(url: url)
        try store.save(profile)

        let reloaded = try store.load(defaultValue: UserProfile.defaultValue())

        #expect(reloaded.llmProvider.active == "openai")
        #expect(reloaded.llmProvider.openAIAuthMode == .oauth)
        #expect(reloaded.llmProvider.openAIFastMode)
    }

    @Test
    func oauthSessionPersistenceAndLogoutNeedExplicitNativeHooks() throws {
        let sourceIndex = try AdvancedSourceIndex()

        let hasSecureOAuthPersistence =
            sourceIndex.anyDocumentContains(all: ["oauth", "KeychainStore"])
            || sourceIndex.anyDocumentContains(all: ["oauth", "JSONFileStore"])
        let hasLogoutClearing =
            sourceIndex.anyDocumentContains(all: ["oauth", "deleteValue(for:"])
            || sourceIndex.anyDocumentContains(all: ["oauth", "logout"])

        #expect(
            hasSecureOAuthPersistence,
            "Native parity still needs persisted OpenAI OAuth session/status storage."
        )
        #expect(
            hasLogoutClearing,
            "Native parity still needs an explicit OAuth logout path that clears persisted session state."
        )
    }

    @Test
    func modelCatalogNeedsOpenAICompatibleFetchAndAnthropicFallback() throws {
        let sourceIndex = try AdvancedSourceIndex()

        let hasRemoteModelFetch =
            sourceIndex.anyDocumentContains(all: ["/models", "URLSession"])
            || sourceIndex.anyDocumentContains(all: ["fetchModels", "apiFormat"])
        let hasAnthropicFallback =
            sourceIndex.anyDocumentContains(all: ["anthropic", "fallback"])
            || sourceIndex.anyDocumentContains(all: ["anthropic", "claude"])

        #expect(
            hasRemoteModelFetch,
            "Native parity still needs model list fetching for OpenAI-compatible providers."
        )
        #expect(
            hasAnthropicFallback,
            "Native parity still needs a deterministic Anthropic model fallback when remote discovery is unavailable."
        )
    }

    @Test
    func userCorrectionPatternsRoundTripThroughJSONPersistence() throws {
        let temporaryDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: temporaryDirectory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: temporaryDirectory) }

        var profile = UserProfile.defaultValue()
        profile.correctionPatterns = [
            CorrectionPattern(
                original: "口子空间",
                corrected: "扣子空间",
                count: 4,
                lastSeen: 1_717_171_717,
                source: .user
            ),
        ]
        profile.totalTranscriptions = 9
        profile.lastUpdated = 1_717_171_718

        let url = temporaryDirectory.appendingPathComponent("user_profile.json", isDirectory: false)
        let store = JSONFileStore<UserProfile>(url: url)
        try store.save(profile)

        let reloaded = try store.load(defaultValue: UserProfile.defaultValue())

        #expect(reloaded.correctionPatterns == profile.correctionPatterns)
        #expect(reloaded.totalTranscriptions == 9)
        #expect(reloaded.lastUpdated == 1_717_171_718)
        #expect(reloaded.relevantCorrections(input: "把它发到口子空间", limit: 1) == profile.correctionPatterns)
    }

    @Test
    func resultHistoryApplyUserEditPersistsAndReturnsCorrectionContext() throws {
        let temporaryDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: temporaryDirectory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: temporaryDirectory) }

        let store = try HistoryStore(
            url: temporaryDirectory.appendingPathComponent("history.json", isDirectory: false),
            maxRecords: 20
        )
        let record = ResultRecord(
            id: "history-1",
            workflow: .dictation,
            sourceText: "口子空间",
            originalText: "口子空间",
            createdAt: 100,
            updatedAt: 100
        )

        _ = try store.append(record)
        let context = try #require(
            try store.applyUserEdit(
                recordID: record.id,
                editedText: "扣子空间",
                editedAt: 200
            )
        )
        let reloaded = try #require(try store.record(withID: record.id))

        #expect(reloaded.editedText == "扣子空间")
        #expect(reloaded.currentText == "扣子空间")
        #expect(reloaded.updatedAt == 200)
        #expect(context.recordID == record.id)
        #expect(context.workflow == .dictation)
        #expect(context.rawOriginal == nil)
        #expect(context.displayedOriginal == "口子空间")
        #expect(context.correctedText == "扣子空间")
    }

    @Test
    func historyEditorFlowPersistsEditsAndSubmitsUserCorrections() throws {
        let sourceIndex = try AdvancedSourceIndex()
        let appModelSource = try sourceIndex.contents(
            of: "Sources/LightWhisperNativeApp/App/AppModel.swift"
        )

        #expect(
            appModelSource.contains("saveEditedHistoryEntry"),
            "AppModel should expose a history editor save action."
        )
        #expect(
            appModelSource.contains("applyUserEdit"),
            "Edited history text should be persisted through HistoryStore.applyUserEdit."
        )
        #expect(
            appModelSource.contains("ProfileManagementService.submitUserCorrection"),
            "Editing a saved history entry should feed a user correction back into the learning profile."
        )
        #expect(
            appModelSource.contains("persistUserProfile()"),
            "History-driven correction learning should persist the updated user profile."
        )
    }

    @Test
    func statusBarAndToolModeNeedPersistedNativeDefaults() throws {
        let sourceIndex = try AdvancedSourceIndex()

        let hasStatusBarController =
            sourceIndex.anyDocumentContains(all: ["NSStatusItem", "statusItem"])
            || sourceIndex.anyDocumentContains(all: ["MenuBarExtra"])
        let hasPersistedToolMode =
            sourceIndex.anyDocumentContains(all: ["defaultToolMode", "JSONFileStore"])
            || sourceIndex.anyDocumentContains(all: ["activeWorkflow", "UserDefaults"])
            || sourceIndex.anyDocumentContains(all: ["RecordingWorkflow", "persist"])

        #expect(
            hasStatusBarController,
            "Native parity still needs a status bar / tray controller for the mac utility workflow."
        )
        #expect(
            hasPersistedToolMode,
            "Native parity still needs persisted tool mode defaults instead of resetting workflow state on every launch."
        )
    }

    @Test
    func profileFileImportExportRoundTripsRepresentativeFields() throws {
        let temporaryDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: temporaryDirectory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: temporaryDirectory) }

        var profile = UserProfile.defaultValue()
        profile.hotWords = [
            HotWord(text: "Codex", weight: 5, source: .user, useCount: 3, lastUsed: 10),
        ]
        profile.correctionPatterns = [
            CorrectionPattern(
                original: "api key",
                corrected: "API key",
                count: 2,
                lastSeen: 20,
                source: .user
            ),
        ]
        profile.translationTarget = "English"
        profile.assistantHotkey = "cmd+shift+a"
        profile.webSearch = WebSearchConfig(enabled: true, provider: .tavily, maxResults: 6)
        profile.llmProvider.active = "openai"
        profile.llmProvider.reasoningMode = .balanced
        profile.llmProvider.polishReasoningMode = .balanced
        profile.llmProvider.assistantReasoningMode = .light
        profile.llmProvider.openAIAuthMode = .oauth
        profile.llmProvider.openAIFastMode = true

        let url = temporaryDirectory.appendingPathComponent("user_profile.json", isDirectory: false)
        let exported = try ProfileManagementService.exportProfile(profile)
        try exported.write(to: url, atomically: true, encoding: .utf8)
        let imported = try ProfileManagementService.importProfile(
            from: String(contentsOf: url, encoding: .utf8)
        )

        #expect(imported.hotWords == profile.hotWords)
        #expect(imported.correctionPatterns == profile.correctionPatterns)
        #expect(imported.translationTarget == "English")
        #expect(imported.assistantHotkey == "cmd+shift+a")
        #expect(imported.webSearch == profile.webSearch)
        #expect(imported.llmProvider.active == "openai")
        #expect(imported.llmProvider.reasoningMode == .balanced)
        #expect(imported.llmProvider.polishReasoningMode == .balanced)
        #expect(imported.llmProvider.assistantReasoningMode == .light)
        #expect(imported.llmProvider.openAIAuthMode == .oauth)
        #expect(imported.llmProvider.openAIFastMode)
    }

    @Test
    func codexOAuthBrowserLoginNeedsLaunchAndCallbackHooks() throws {
        let sourceIndex = try AdvancedSourceIndex()

        let hasBrowserLaunchHook =
            sourceIndex.anyDocumentContains(all: ["codexoauthservice", "nsworkspace.shared.open"])
            || sourceIndex.anyDocumentContains(all: ["codexoauthservice", "openurl"])
            || sourceIndex.anyDocumentContains(all: ["codex oauth", "browser"])
        let hasLoginCallbackHandling =
            sourceIndex.anyDocumentContains(all: ["codexoauthservice", "redirect_uri"])
            || sourceIndex.anyDocumentContains(all: ["codexoauthservice", "authorization_code"])
            || sourceIndex.anyDocumentContains(all: ["codexoauthservice", "callbackurl"])
            || sourceIndex.anyDocumentContains(all: ["codexoauthservice", "code_verifier"])

        #expect(
            hasBrowserLaunchHook,
            "Native parity still needs a browser-login launcher for OpenAI Codex OAuth."
        )
        #expect(
            hasLoginCallbackHandling,
            "Native parity still needs an OAuth callback/code-exchange hook after browser login."
        )
    }
}

private struct AdvancedSourceIndex {
    private struct SourceDocument {
        let relativePath: String
        let contents: String
    }

    private let documents: [SourceDocument]

    init() throws {
        let packageRoot = Self.packageRoot()
        let sourceRoot = packageRoot.appendingPathComponent("Sources/LightWhisperNativeApp", isDirectory: true)
        let enumerator = FileManager.default.enumerator(
            at: sourceRoot,
            includingPropertiesForKeys: nil
        )

        var loaded: [SourceDocument] = []
        while let fileURL = enumerator?.nextObject() as? URL {
            guard fileURL.pathExtension == "swift" else { continue }
            let contents = try String(contentsOf: fileURL, encoding: .utf8)
            let relativePath = fileURL.path.replacingOccurrences(
                of: packageRoot.path + "/",
                with: ""
            )
            loaded.append(SourceDocument(relativePath: relativePath, contents: contents))
        }

        documents = loaded
    }

    func contents(of relativePath: String) throws -> String {
        guard let document = documents.first(where: { $0.relativePath == relativePath }) else {
            throw NSError(
                domain: "AdvancedParityTests",
                code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Missing source file: \(relativePath)"]
            )
        }
        return document.contents
    }

    func anyDocumentContains(all snippets: [String]) -> Bool {
        documents.contains { document in
            snippets.allSatisfy { snippet in
                document.contents.localizedCaseInsensitiveContains(snippet)
            }
        }
    }

    private static func packageRoot() -> URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
    }
}
