import Foundation
import Testing
@testable import LightWhisperNativeApp

@Suite("Service Parity")
struct ServiceParityTests {
    @Test
    func updaterNormalizesAndComparesSemanticVersions() {
        #expect(UpdaterService.normalizeVersion("v1.3.3") == "1.3.3")
        #expect(UpdaterService.normalizeVersion("  v2.0.0-beta1  ") == "2.0.0-beta1")
        #expect(UpdaterService.isVersionNewer(latest: "v1.10.0", current: "1.9.9"))
        #expect(UpdaterService.isVersionNewer(latest: "1.3.4", current: "1.3.3"))
        #expect(!UpdaterService.isVersionNewer(latest: "1.3.3", current: "1.3.3"))
        #expect(!UpdaterService.isVersionNewer(latest: "1.3.3", current: "1.4.0"))
    }

    @Test
    func webSearchContextRendersStructuredResults() {
        let context = WebSearchService.renderContext([
            SearchResult(title: "First Result", url: "https://example.com/1", content: "Alpha"),
            SearchResult(title: "Second Result", url: "https://example.com/2", content: "Beta"),
        ])

        #expect(context.contains("<web_search_results>"))
        #expect(context.contains("<title><![CDATA[First Result]]></title>"))
        #expect(context.contains("<url><![CDATA[https://example.com/2]]></url>"))
        #expect(context.contains("<content><![CDATA[Beta]]></content>"))
    }

    @Test
    func assistantPromptRendersSelectedTextAndScreenContext() {
        let rendered = AssistantPromptBuilder.renderUserContent(
            appContext: "<app_context><![CDATA[Code]]></app_context>",
            request: "Rewrite this reply",
            selectedText: "Original text",
            hasScreenContext: true
        )

        #expect(rendered.contains("<app_context><![CDATA[Code]]></app_context>"))
        #expect(rendered.contains("<selected_text><![CDATA[Original text]]></selected_text>"))
        #expect(rendered.contains("<screen_context><![CDATA[Screen context is attached. Use it only when relevant to the user request.]]></screen_context>"))
        #expect(rendered.contains("<user_request><![CDATA[Rewrite this reply]]></user_request>"))
    }

    @Test
    func aiPolishPromptInjectsHotWordsCorrectionsAndTranslationRequirement() {
        var profile = UserProfile.defaultValue()
        profile.hotWords = [
            HotWord(text: "Codex", weight: 5, source: .user, useCount: 10, lastUsed: 1),
        ]
        profile.correctionPatterns = [
            CorrectionPattern(original: "口子空间", corrected: "扣子空间", count: 3, lastSeen: 1, source: .user),
            CorrectionPattern(original: "web hook", corrected: "Webhook", count: 2, lastSeen: 1, source: .ai),
        ]

        let prompt = AIPolishPromptBuilder.buildSystemPrompt(
            profile: profile,
            inputText: "把这个接口挂到口子空间的 web hook 上",
            translationTargetOverride: "English"
        )

        #expect(prompt.contains("<user_terms>"))
        #expect(prompt.contains("<term><![CDATA[Codex]]></term>"))
        #expect(prompt.contains("<confirmed_by_user>"))
        #expect(prompt.contains("<original><![CDATA[口子空间]]></original>"))
        #expect(prompt.contains("<learned_by_ai>"))
        #expect(prompt.contains("<corrected><![CDATA[Webhook]]></corrected>"))
        #expect(prompt.contains("<translation_requirement>"))
        #expect(prompt.contains("<target_language><![CDATA[English]]></target_language>"))
    }

    @Test
    func llmTransportBuildsOpenAICompatAndAnthropicBodies() throws {
        let openAIEndpoint = LLMEndpoint(
            provider: "openai",
            apiURL: "https://api.openai.com/v1/responses",
            model: "gpt-4.1-mini",
            apiFormat: .openaiCompat
        )
        let anthropicEndpoint = LLMEndpoint(
            provider: "anthropic-custom",
            apiURL: "https://api.anthropic.com/v1/messages",
            model: "claude-sonnet-4-0",
            apiFormat: .anthropic
        )

        let openAIData = try LLMTransport.openAICompatBody(
            endpoint: openAIEndpoint,
            system: "System prompt",
            user: "User prompt",
            jsonOutput: true,
            apiKey: "sk-test",
            fastMode: false
        )
        let anthropicData = try LLMTransport.anthropicBody(
            endpoint: anthropicEndpoint,
            system: "System prompt",
            user: "User prompt",
            jsonOutput: false
        )

        let openAI = try #require(JSONSerialization.jsonObject(with: openAIData) as? [String: Any])
        let anthropic = try #require(JSONSerialization.jsonObject(with: anthropicData) as? [String: Any])

        #expect(openAI["model"] as? String == "gpt-4.1-mini")
        #expect((openAI["input"] as? [[String: Any]])?.count == 2)
        #expect(openAI["text"] != nil)

        #expect(anthropic["model"] as? String == "claude-sonnet-4-0")
        #expect(anthropic["system"] as? String == "System prompt")
        #expect((anthropic["messages"] as? [[String: Any]])?.first?["role"] as? String == "user")
    }
}
