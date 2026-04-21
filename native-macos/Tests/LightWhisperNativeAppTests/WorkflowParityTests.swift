import Foundation
import Testing
@testable import LightWhisperNativeApp

@Suite("Workflow Parity")
struct WorkflowParityTests {
    @Test
    func dictationTranslationRoutesTranscriptThroughAIPolishWhenTranslationTargetIsSet() throws {
        var profile = UserProfile.defaultValue()
        profile.translationTarget = "English"
        let sourceIndex = try TestSourceIndex()
        let decision = AIPolishService.processingDecision(
            transcript: "你好，世界",
            profile: profile,
            apiKey: "polish-key",
            translationTargetOverride: nil
        )

        let prompt = AIPolishPromptBuilder.buildSystemPrompt(
            profile: profile,
            inputText: "你好，世界",
            translationTargetOverride: nil
        )
        let coordinatorSource = try sourceIndex.contents(
            of: "Sources/LightWhisperNativeApp/App/DictationCoordinator.swift"
        )
        let coordinatorUsesAIPolishService = coordinatorSource.contains("AIPolishService")
        let coordinatorChecksTranslationTarget = coordinatorSource.contains("translationTarget")

        #expect(decision.shouldPolish)
        #expect(decision.translationTarget == "English")
        #expect(prompt.contains("<translation_requirement>"))
        #expect(prompt.contains("<target_language><![CDATA[English]]></target_language>"))
        #expect(
            coordinatorUsesAIPolishService,
            "DictationCoordinator should route translated dictation through AIPolishService."
        )
        #expect(
            coordinatorChecksTranslationTarget,
            "DictationCoordinator should check translationTarget before finalizing the transcript."
        )
    }

    @Test
    func dictationSkipsAIPolishWhenNoTranslationPromptOrCorrectionsExist() throws {
        let profile = UserProfile.defaultValue()
        let inputText = "plain transcript"
        let sourceIndex = try TestSourceIndex()
        let decision = AIPolishService.processingDecision(
            transcript: inputText,
            profile: profile,
            apiKey: "polish-key",
            translationTargetOverride: nil
        )
        let coordinatorSource = try sourceIndex.contents(
            of: "Sources/LightWhisperNativeApp/App/DictationCoordinator.swift"
        )
        let coordinatorChecksProcessingDecision = coordinatorSource.contains("processingDecision")
        let coordinatorBranchesOnShouldPolish = coordinatorSource.contains("shouldPolish")

        #expect(!decision.shouldPolish)
        #expect(
            coordinatorChecksProcessingDecision,
            "DictationCoordinator should ask AIPolishService whether the transcript needs AI polish."
        )
        #expect(
            coordinatorBranchesOnShouldPolish,
            "DictationCoordinator should short-circuit to the raw transcript when no AI polish inputs exist."
        )
    }

    @Test
    func assistantFallsBackToActiveProviderKeyWhenProvidersMatch() {
        var config = LLMProviderConfig.defaultValue()
        config.active = "deepseek"
        config.assistantUseSeparateModel = true
        config.assistantProvider = "deepseek"
        var profile = UserProfile.defaultValue()
        profile.llmProvider = config

        let resolved = AssistantService.resolveAPIKey(
            profile: profile,
            assistantAPIKey: "",
            polishAPIKey: "shared-active-key",
            storedKeys: [:]
        )

        #expect(resolved == "shared-active-key")
    }

    @Test
    func assistantIncludesRenderedWebSearchContextWhenThirdPartyResultsExist() {
        let searchResults = [
            SearchResult(
                title: "Realtime Weather",
                url: "https://example.com/weather",
                content: "Vienna will be sunny."
            ),
        ]
        let prompt = AssistantService.buildSystemPrompt(
            profile: UserProfile.defaultValue(),
            webSearchResults: searchResults
        )

        #expect(prompt.contains("<web_search_results>"))
        #expect(prompt.contains("<title><![CDATA[Realtime Weather]]></title>"))
        #expect(prompt.contains("<url><![CDATA[https://example.com/weather]]></url>"))
        #expect(prompt.contains("<content><![CDATA[Vienna will be sunny.]]></content>"))
    }
}

private struct TestSourceIndex {
    private struct SourceDocument {
        let relativePath: String
        let contents: String
    }

    private let documents: [SourceDocument]

    init() throws {
        let sourceRoot = Self.packageRoot()
            .appendingPathComponent("Sources/LightWhisperNativeApp", isDirectory: true)
        let enumerator = FileManager.default.enumerator(
            at: sourceRoot,
            includingPropertiesForKeys: nil
        )

        var loaded: [SourceDocument] = []
        while let fileURL = enumerator?.nextObject() as? URL {
            guard fileURL.pathExtension == "swift" else { continue }
            let contents = try String(contentsOf: fileURL, encoding: .utf8)
            let relativePath = fileURL.path.replacingOccurrences(
                of: Self.packageRoot().path + "/",
                with: ""
            )
            loaded.append(SourceDocument(relativePath: relativePath, contents: contents))
        }

        documents = loaded
    }

    func contents(of relativePath: String) throws -> String {
        guard let document = documents.first(where: { $0.relativePath == relativePath }) else {
            throw NSError(
                domain: "WorkflowParityTests",
                code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Missing source file: \(relativePath)"]
            )
        }
        return document.contents
    }

    private static func packageRoot() -> URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
    }
}
