import Foundation

public struct LLMEndpoint: Equatable, Sendable {
    public let provider: String
    public let apiURL: String
    public let model: String
    public let apiFormat: ApiFormat

    public init(provider: String, apiURL: String, model: String, apiFormat: ApiFormat) {
        self.provider = provider
        self.apiURL = apiURL
        self.model = model
        self.apiFormat = apiFormat
    }
}

public enum LLMProviderCatalog {
    public static func endpoint(for config: LLMProviderConfig) -> LLMEndpoint {
        let activeProvider = config.resolveActiveProvider()

        if isPreset(activeProvider) {
            let defaults = defaultParts(for: activeProvider)
            let useCustomEndpoint = activeProvider == "custom"
            return LLMEndpoint(
                provider: activeProvider,
                apiURL: useCustomEndpoint
                    ? normalizeOpenAICompatURL(
                        config.customBaseURL,
                        defaultBaseURL: defaults.baseURL,
                        apiSuffix: defaults.apiSuffix
                    )
                    : normalizeOpenAICompatURL(nil, defaultBaseURL: defaults.baseURL, apiSuffix: defaults.apiSuffix),
                model: config.customModel?.trimmingCharacters(in: .whitespacesAndNewlines).nilIfEmpty ?? defaults.model,
                apiFormat: .openaiCompat
            )
        }

        if let customProvider = config.customProviders.first(where: { $0.id == activeProvider }) {
            let apiURL: String
            switch customProvider.apiFormat {
            case .anthropic:
                apiURL = normalizeAnthropicURL(customProvider.baseURL)
            case .openaiCompat:
                apiURL = normalizeOpenAICompatURL(
                    customProvider.baseURL,
                    defaultBaseURL: "http://127.0.0.1:8000",
                    apiSuffix: "chat/completions"
                )
            }

            return LLMEndpoint(
                provider: customProvider.id,
                apiURL: apiURL,
                model: customProvider.model.trimmingCharacters(in: .whitespacesAndNewlines).nilIfEmpty ?? "gpt-4.1-mini",
                apiFormat: customProvider.apiFormat
            )
        }

        let defaults = defaultParts(for: "cerebras")
        return LLMEndpoint(
            provider: "cerebras",
            apiURL: normalizeOpenAICompatURL(nil, defaultBaseURL: defaults.baseURL, apiSuffix: defaults.apiSuffix),
            model: defaults.model,
            apiFormat: .openaiCompat
        )
    }

    public static func endpoint(
        for provider: String,
        baseURL: String? = nil,
        model: String? = nil,
        apiFormat: ApiFormat = .openaiCompat
    ) -> LLMEndpoint {
        if isPreset(provider) {
            var config = LLMProviderConfig.defaultValue()
            config.active = provider
            config.customBaseURL = baseURL
            config.customModel = model
            return endpoint(for: config)
        }

        var config = LLMProviderConfig.defaultValue()
        config.active = provider
        config.customProviders = [
            CustomProvider(
                id: provider,
                name: provider,
                baseURL: baseURL ?? "",
                model: model ?? "",
                apiFormat: apiFormat
            )
        ]
        return endpoint(for: config)
    }

    public static func assistantEndpoint(for config: LLMProviderConfig) -> LLMEndpoint {
        var resolved = config
        let assistantProvider = config.resolveAssistantProvider()
        if assistantProvider != config.resolveActiveProvider() {
            resolved.active = assistantProvider
            if isPreset(assistantProvider) {
                resolved.customModel = nil
                resolved.customBaseURL = nil
            }
        }

        if let assistantModel = config.resolvedAssistantModel() {
            if isPreset(resolved.resolveActiveProvider()) {
                resolved.customModel = assistantModel
            } else if let index = resolved.customProviders.firstIndex(where: { $0.id == resolved.resolveActiveProvider() }) {
                resolved.customProviders[index].model = assistantModel
            } else {
                resolved.customModel = assistantModel
            }
        }

        return endpoint(for: resolved)
    }

    public static func validationEndpoint(for config: LLMProviderConfig) -> LLMEndpoint {
        var resolved = config
        let validationProvider = config.resolveValidationProvider()
        if validationProvider != config.resolveActiveProvider() {
            resolved.active = validationProvider
            if isPreset(validationProvider) {
                resolved.customModel = nil
                resolved.customBaseURL = nil
            }
        }

        if let validationModel = config.resolvedValidationModel() {
            if isPreset(resolved.resolveActiveProvider()) {
                resolved.customModel = validationModel
            } else if let index = resolved.customProviders.firstIndex(where: { $0.id == resolved.resolveActiveProvider() }) {
                resolved.customProviders[index].model = validationModel
            } else {
                resolved.customModel = validationModel
            }
        }

        return endpoint(for: resolved)
    }

    public static func keychainUser(for provider: String) -> String {
        switch provider {
        case "openai":
            return "openai-api-key"
        case "deepseek":
            return "deepseek-api-key"
        case "siliconflow":
            return "siliconflow-api-key"
        case "custom":
            return "custom-api-key"
        case "cerebras":
            return "cerebras-api-key"
        default:
            return "custom-\(provider)-api-key"
        }
    }

    private static func isPreset(_ provider: String) -> Bool {
        ["cerebras", "openai", "deepseek", "siliconflow", "custom"].contains(provider)
    }

    private static func defaultParts(for provider: String) -> (baseURL: String, model: String, apiSuffix: String) {
        switch provider {
        case "openai":
            return ("https://api.openai.com", "gpt-4.1-mini", "responses")
        case "deepseek":
            return ("https://api.deepseek.com", "deepseek-chat", "chat/completions")
        case "siliconflow":
            return ("https://api.siliconflow.cn", "Qwen/Qwen3-32B", "chat/completions")
        case "custom":
            return ("http://127.0.0.1:8000", "gpt-4.1-mini", "chat/completions")
        default:
            return ("https://api.cerebras.ai", "gpt-oss-120b", "chat/completions")
        }
    }

    private static func normalizeOpenAICompatURL(
        _ input: String?,
        defaultBaseURL: String,
        apiSuffix: String
    ) -> String {
        let raw = input?.trimmingCharacters(in: .whitespacesAndNewlines).nilIfEmpty ?? defaultBaseURL

        if let explicit = raw.stripSuffix("#") {
            return explicit.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        }

        let trimmed = raw.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        let lower = trimmed.lowercased()
        if lower.hasSuffix("/chat/completions") || lower.hasSuffix("/responses") {
            return trimmed
        }
        if lower.hasSuffix("/v1") || lower.hasSuffix("/api/v3") {
            return "\(trimmed)/\(apiSuffix)"
        }
        return "\(trimmed)/v1/\(apiSuffix)"
    }

    private static func normalizeAnthropicURL(_ input: String) -> String {
        let trimmed = input.trimmingCharacters(in: .whitespacesAndNewlines).trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        if trimmed.isEmpty {
            return "https://api.anthropic.com/v1/messages"
        }
        if let explicit = trimmed.stripSuffix("#") {
            return explicit.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        }
        let lower = trimmed.lowercased()
        if lower.hasSuffix("/messages") {
            return trimmed
        }
        if lower.hasSuffix("/v1") {
            return "\(trimmed)/messages"
        }
        return "\(trimmed)/v1/messages"
    }
}

private extension String {
    var nilIfEmpty: String? {
        isEmpty ? nil : self
    }

    func stripSuffix(_ suffix: String) -> String? {
        guard hasSuffix(suffix) else { return nil }
        return String(dropLast(suffix.count))
    }
}
