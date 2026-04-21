import Foundation
import Testing
@testable import LightWhisperNativeApp

@Suite("Compatibility")
struct CompatibilityTests {
    private func decode<T: Decodable>(_: T.Type, from json: String) throws -> T {
        let data = Data(json.utf8)
        let decoder = JSONDecoder()
        return try decoder.decode(T.self, from: data)
    }

    private func encodeJSONObject<T: Encodable>(_ value: T) throws -> [String: Any] {
        let data = try JSONEncoder().encode(value)
        let object = try JSONSerialization.jsonObject(with: data, options: [])
        guard let dictionary = object as? [String: Any] else {
            throw NSError(
                domain: "CompatibilityTests",
                code: 1,
                userInfo: [NSLocalizedDescriptionKey: "Expected top-level JSON object"]
            )
        }
        return dictionary
    }

    @Test
    func legacyEnginesNormalizeToAlibaba() {
        #expect(EngineSettings.normalized(engineValue: "local") == .alibabaAsr)
        #expect(EngineSettings.normalized(engineValue: "sensevoice") == .alibabaAsr)
        #expect(EngineSettings.normalized(engineValue: "whisper") == .alibabaAsr)
        #expect(EngineSettings.normalized(engineValue: "unknown") == .alibabaAsr)
        #expect(EngineSettings.normalized(engineValue: "glm-asr") == .glmAsr)
    }

    @Test
    func engineDefaultsStayOnAlibabaASR() {
        let settings = EngineSettings()

        #expect(AppPaths.defaultEngine == .alibabaAsr)
        #expect(settings.engine == .alibabaAsr)
        #expect(settings.alibabaModel == "qwen3-asr-flash")
    }

    @Test
    func onlineAsrKeychainUserTracksEngineAndRegion() {
        var settings = EngineSettings()

        settings.engine = .glmAsr
        settings.glmRegion = .international
        #expect(settings.onlineASRKeychainUser() == "glm-asr-api-key")

        settings.engine = .alibabaAsr
        settings.alibabaRegion = .international
        #expect(settings.onlineASRKeychainUser() == "alibaba-asr-intl-api-key")

        settings.alibabaRegion = .domestic
        #expect(settings.onlineASRKeychainUser() == "alibaba-asr-cn-api-key")
    }

    @Test
    func builtinProviderEndpointsNormalizeAsExpected() throws {
        let openAIConfig: LLMProviderConfig = try decode(
            LLMProviderConfig.self,
            from: """
            {
              "active": "openai",
              "reasoning_mode": "provider_default",
              "custom_providers": []
            }
            """
        )
        let deepseekConfig: LLMProviderConfig = try decode(
            LLMProviderConfig.self,
            from: """
            {
              "active": "deepseek",
              "reasoning_mode": "provider_default",
              "custom_providers": []
            }
            """
        )
        let cerebrasConfig: LLMProviderConfig = try decode(
            LLMProviderConfig.self,
            from: """
            {
              "active": "cerebras",
              "reasoning_mode": "provider_default",
              "custom_providers": []
            }
            """
        )
        let customPresetConfig: LLMProviderConfig = try decode(
            LLMProviderConfig.self,
            from: """
            {
              "active": "custom",
              "custom_base_url": "https://example.com",
              "custom_model": "foo-model",
              "reasoning_mode": "provider_default",
              "custom_providers": []
            }
            """
        )

        let openAIEndpoint = LLMProviderCatalog.endpoint(for: openAIConfig)
        let deepseekEndpoint = LLMProviderCatalog.endpoint(for: deepseekConfig)
        let cerebrasEndpoint = LLMProviderCatalog.endpoint(for: cerebrasConfig)
        let customPresetEndpoint = LLMProviderCatalog.endpoint(for: customPresetConfig)

        #expect(openAIEndpoint.apiURL == "https://api.openai.com/v1/responses")
        #expect(deepseekEndpoint.apiURL == "https://api.deepseek.com/v1/chat/completions")
        #expect(cerebrasEndpoint.apiURL == "https://api.cerebras.ai/v1/chat/completions")
        #expect(customPresetEndpoint.apiURL == "https://example.com/v1/chat/completions")
    }

    @Test
    func customProviderUrlsRespectFormatAndExplicitSuffixes() throws {
        let anthropicConfig: LLMProviderConfig = try decode(
            LLMProviderConfig.self,
            from: """
            {
              "active": "anthropic_custom",
              "reasoning_mode": "provider_default",
              "custom_providers": [
                {
                  "id": "anthropic_custom",
                  "name": "Anthropic Custom",
                  "base_url": "https://api.anthropic.com",
                  "model": "claude-3-7-sonnet-latest",
                  "api_format": "anthropic"
                }
              ]
            }
            """
        )
        let openAICompatConfig: LLMProviderConfig = try decode(
            LLMProviderConfig.self,
            from: """
            {
              "active": "openai_custom",
              "reasoning_mode": "provider_default",
              "custom_providers": [
                {
                  "id": "openai_custom",
                  "name": "OpenAI Custom",
                  "base_url": "https://example.com",
                  "model": "gpt-4.1-mini",
                  "api_format": "openai_compat"
                }
              ]
            }
            """
        )
        let explicitSuffixConfig: LLMProviderConfig = try decode(
            LLMProviderConfig.self,
            from: """
            {
              "active": "explicit",
              "reasoning_mode": "provider_default",
              "custom_providers": [
                {
                  "id": "explicit",
                  "name": "Explicit",
                  "base_url": "https://example.com/v1/chat/completions#",
                  "model": "gpt-4.1-mini",
                  "api_format": "openai_compat"
                }
              ]
            }
            """
        )

        let anthropicEndpoint = LLMProviderCatalog.endpoint(for: anthropicConfig)
        let openAICompatEndpoint = LLMProviderCatalog.endpoint(for: openAICompatConfig)
        let explicitSuffixEndpoint = LLMProviderCatalog.endpoint(for: explicitSuffixConfig)

        #expect(anthropicEndpoint.apiURL == "https://api.anthropic.com/v1/messages")
        #expect(openAICompatEndpoint.apiURL == "https://example.com/v1/chat/completions")
        #expect(explicitSuffixEndpoint.apiURL == "https://example.com/v1/chat/completions")
    }

    @Test
    func trailingHashDisablesSuffixingForCustomProviderBaseUrls() throws {
        let anthropicConfig: LLMProviderConfig = try decode(
            LLMProviderConfig.self,
            from: """
            {
              "active": "anthropic_custom",
              "reasoning_mode": "provider_default",
              "custom_providers": [
                {
                  "id": "anthropic_custom",
                  "name": "Anthropic Custom",
                  "base_url": "https://api.anthropic.com/v1/messages#",
                  "model": "claude-3-7-sonnet-latest",
                  "api_format": "anthropic"
                }
              ]
            }
            """
        )
        let openAICompatConfig: LLMProviderConfig = try decode(
            LLMProviderConfig.self,
            from: """
            {
              "active": "openai_custom",
              "reasoning_mode": "provider_default",
              "custom_providers": [
                {
                  "id": "openai_custom",
                  "name": "OpenAI Custom",
                  "base_url": "https://example.com#",
                  "model": "gpt-4.1-mini",
                  "api_format": "openai_compat"
                }
              ]
            }
            """
        )

        #expect(LLMProviderCatalog.endpoint(for: anthropicConfig).apiURL == "https://api.anthropic.com/v1/messages")
        #expect(LLMProviderCatalog.endpoint(for: openAICompatConfig).apiURL == "https://example.com")
    }

    @Test
    func keychainUsersFollowProviderNames() {
        #expect(LLMProviderCatalog.keychainUser(for: "openai") == "openai-api-key")
        #expect(LLMProviderCatalog.keychainUser(for: "deepseek") == "deepseek-api-key")
        #expect(LLMProviderCatalog.keychainUser(for: "siliconflow") == "siliconflow-api-key")
        #expect(LLMProviderCatalog.keychainUser(for: "custom") == "custom-api-key")
        #expect(LLMProviderCatalog.keychainUser(for: "cerebras") == "cerebras-api-key")
        #expect(LLMProviderCatalog.keychainUser(for: "my-provider") == "custom-my-provider-api-key")
    }

    @Test
    func legacyCustomProviderFieldsMigrateIntoCustomProviders() throws {
        let legacyProfileJSON = """
        {
          "llm_provider": {
            "active": "custom",
            "custom_base_url": "https://legacy.example.com",
            "custom_model": "legacy-model",
            "reasoning_mode": "light",
            "custom_providers": []
          }
        }
        """
        var profile: UserProfile = try decode(UserProfile.self, from: legacyProfileJSON)

        UserProfileNormalizer.normalize(&profile)

        let json = try encodeJSONObject(profile)
        let llmProvider = try #require(json["llm_provider"] as? [String: Any])
        let customProviders = try #require(llmProvider["custom_providers"] as? [[String: Any]])
        let migrated = try #require(customProviders.first)

        #expect(llmProvider["active"] as? String == "custom_migrated")
        #expect(migrated["id"] as? String == "custom_migrated")
        #expect(migrated["base_url"] as? String == "https://legacy.example.com")
        #expect(migrated["model"] as? String == "legacy-model")
        #expect(llmProvider["custom_base_url"] == nil)
        #expect(llmProvider["custom_model"] == nil)
    }

    @Test
    func missingSplitReasoningModesFallBackToLegacyMode() throws {
        let profileJSON = """
        {
          "llm_provider": {
            "active": "openai",
            "reasoning_mode": "balanced",
            "custom_providers": []
          }
        }
        """
        var profile: UserProfile = try decode(UserProfile.self, from: profileJSON)

        UserProfileNormalizer.normalize(&profile)

        let json = try encodeJSONObject(profile)
        let llmProvider = try #require(json["llm_provider"] as? [String: Any])

        #expect(llmProvider["polish_reasoning_mode"] as? String == "balanced")
        #expect(llmProvider["assistant_reasoning_mode"] as? String == "balanced")
    }

    @Test
    func userProfileDefaultsKeepWebSearchDisabledWithModelNativeAndFiveResults() {
        let profile = UserProfile.defaultValue()

        #expect(profile.webSearch.enabled == false)
        #expect(profile.webSearch.provider == .modelNative)
        #expect(profile.webSearch.maxResults == 5)
    }
}
