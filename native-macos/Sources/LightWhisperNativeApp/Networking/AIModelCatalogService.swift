import Foundation

struct AIModelCatalogEntry: Codable, Equatable, Identifiable, Sendable {
    var id: String
    var ownedBy: String?

    enum CodingKeys: String, CodingKey {
        case id
        case ownedBy = "owned_by"
    }
}

struct AIModelCatalogResult: Equatable, Sendable {
    var models: [AIModelCatalogEntry]
    var sourceURL: String
    var usedFallback: Bool
}

enum AIModelCatalogError: LocalizedError {
    case invalidURL(String)
    case invalidResponse(String)
    case requestFailed(Int, String)

    var errorDescription: String? {
        switch self {
        case .invalidURL(let value):
            return "Invalid model catalog URL: \(value)"
        case .invalidResponse(let message):
            return message
        case .requestFailed(let status, let body):
            return "Model catalog HTTP \(status): \(body)"
        }
    }
}

enum AIModelCatalogService {
    static func listModels(
        provider: String,
        config: LLMProviderConfig,
        baseURL: String? = nil,
        apiKey: String? = nil,
        keychainStore: KeychainStore = KeychainStore(),
        fileManager: FileManager = .default,
        session: URLSession = .shared
    ) async throws -> AIModelCatalogResult {
        let resolvedAPIKey = try await CodexOAuthService.resolveProviderAPIKey(
            provider: provider,
            config: config,
            manualAPIKey: apiKey,
            keychainStore: keychainStore,
            fileManager: fileManager
        ).trimmingCharacters(in: .whitespacesAndNewlines)

        guard !resolvedAPIKey.isEmpty else {
            throw NativeLLMServiceError.missingAPIKey(provider)
        }

        if provider == CodexOAuthService.openAIProvider,
           CodexOAuthService.decodeChatGPTBearerToken(resolvedAPIKey) != nil
        {
            return AIModelCatalogResult(
                models: codexOAuthModels(),
                sourceURL: CodexOAuthService.chatGPTCodexResponsesURL,
                usedFallback: true
            )
        }

        let sourceURL = modelsURL(config: config, provider: provider, baseURL: baseURL)
        let isAnthropic = apiFormat(config: config, provider: provider) == .anthropic

        if sourceURL.isEmpty {
            return AIModelCatalogResult(
                models: isAnthropic ? anthropicModels() : [],
                sourceURL: sourceURL,
                usedFallback: isAnthropic
            )
        }

        guard let url = URL(string: sourceURL) else {
            throw AIModelCatalogError.invalidURL(sourceURL)
        }

        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.timeoutInterval = 12
        for (name, value) in try authorizationHeaders(
            apiFormat: apiFormat(config: config, provider: provider),
            apiKey: resolvedAPIKey
        ) {
            request.setValue(value, forHTTPHeaderField: name)
        }

        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await session.data(for: request)
        } catch {
            if isAnthropic {
                return AIModelCatalogResult(
                    models: anthropicModels(),
                    sourceURL: sourceURL,
                    usedFallback: true
                )
            }
            throw error
        }

        guard let httpResponse = response as? HTTPURLResponse else {
            throw AIModelCatalogError.invalidResponse("The model catalog response was unreadable.")
        }

        guard (200..<300).contains(httpResponse.statusCode) else {
            if isAnthropic {
                return AIModelCatalogResult(
                    models: anthropicModels(),
                    sourceURL: sourceURL,
                    usedFallback: true
                )
            }
            throw AIModelCatalogError.requestFailed(
                httpResponse.statusCode,
                String(decoding: data, as: UTF8.self)
            )
        }

        let models = try parseModels(from: data)
        return AIModelCatalogResult(
            models: normalize(models),
            sourceURL: sourceURL,
            usedFallback: false
        )
    }

    static func modelsURL(
        config: LLMProviderConfig,
        provider: String,
        baseURL: String? = nil
    ) -> String {
        if !isPreset(provider),
           let customProvider = config.customProviders.first(where: { $0.id == provider })
        {
            let effectiveURL = baseURL ?? customProvider.baseURL
            if customProvider.apiFormat == .anthropic {
                return normalizeAnthropicModelsURL(effectiveURL)
            }
            return normalizeModelsURL(effectiveURL, defaultBaseURL: customProvider.baseURL)
        }

        let defaultBaseURL = defaultBaseURL(for: provider)
        if provider == "custom" {
            return normalizeModelsURL(baseURL, defaultBaseURL: defaultBaseURL)
        }
        return normalizeModelsURL(nil, defaultBaseURL: defaultBaseURL)
    }

    private static func parseModels(from data: Data) throws -> [AIModelCatalogEntry] {
        guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw AIModelCatalogError.invalidResponse("The model catalog payload was not valid JSON.")
        }
        guard let entries = json["data"] as? [[String: Any]] else {
            throw AIModelCatalogError.invalidResponse("The model catalog payload did not contain a data array.")
        }

        return entries.compactMap { item in
            guard let id = (item["id"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines),
                  !id.isEmpty
            else {
                return nil
            }
            return AIModelCatalogEntry(
                id: id,
                ownedBy: (item["owned_by"] as? String)?.trimmedOrNil
            )
        }
    }

    private static func normalize(_ models: [AIModelCatalogEntry]) -> [AIModelCatalogEntry] {
        let sorted = models.sorted {
            $0.id.localizedCaseInsensitiveCompare($1.id) == .orderedAscending
        }

        var seen = Set<String>()
        return sorted.filter { seen.insert($0.id).inserted }
    }

    private static func authorizationHeaders(
        apiFormat: ApiFormat,
        apiKey: String
    ) throws -> [String: String] {
        switch apiFormat {
        case .anthropic:
            return [
                "x-api-key": apiKey,
                "anthropic-version": "2023-06-01",
                "Content-Type": "application/json",
            ]
        case .openaiCompat:
            if let token = CodexOAuthService.decodeChatGPTBearerToken(apiKey) {
                var headers = [
                    "Authorization": "Bearer \(token.accessToken)",
                    "Content-Type": "application/json",
                    "originator": CodexOAuthService.originator,
                    "User-Agent": CodexOAuthService.chatGPTBearerUserAgent,
                ]
                if let accountID = token.accountID?.trimmedOrNil {
                    headers["ChatGPT-Account-ID"] = accountID
                }
                return headers
            }

            let bearerAPIKey = CodexOAuthService.decodeOAuthAPIKey(apiKey) ?? apiKey
            return [
                "Authorization": "Bearer \(bearerAPIKey)",
                "Content-Type": "application/json",
            ]
        }
    }

    private static func apiFormat(
        config: LLMProviderConfig,
        provider: String
    ) -> ApiFormat {
        if let customProvider = config.customProviders.first(where: { $0.id == provider }) {
            return customProvider.apiFormat
        }
        return .openaiCompat
    }

    private static func normalizeModelsURL(
        _ input: String?,
        defaultBaseURL: String
    ) -> String {
        let raw = input?.trimmedOrNil ?? defaultBaseURL.trimmedOrNil ?? ""
        guard !raw.isEmpty else {
            return ""
        }

        let trimmed = raw
            .trimmingCharacters(in: CharacterSet(charactersIn: "#"))
            .trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        let lower = trimmed.lowercased()

        if lower.hasSuffix("/models") {
            return trimmed
        }
        if lower.hasSuffix("/chat/completions") {
            return trimmed.dropping(suffix: "/chat/completions").trimmingCharacters(in: CharacterSet(charactersIn: "/")) + "/models"
        }
        if lower.hasSuffix("/responses") {
            return trimmed.dropping(suffix: "/responses").trimmingCharacters(in: CharacterSet(charactersIn: "/")) + "/models"
        }
        if lower.hasSuffix("/v1") || lower.hasSuffix("/api/v3") {
            return "\(trimmed)/models"
        }

        return "\(trimmed)/v1/models"
    }

    private static func normalizeAnthropicModelsURL(_ baseURL: String) -> String {
        let trimmed = baseURL.trimmingCharacters(in: .whitespacesAndNewlines).trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        guard !trimmed.isEmpty else {
            return "https://api.anthropic.com/v1/models"
        }

        let lower = trimmed.lowercased()
        if lower.hasSuffix("/v1/models") {
            return trimmed
        }
        if lower.hasSuffix("/v1/messages") {
            return trimmed.dropping(suffix: "/messages").trimmingCharacters(in: CharacterSet(charactersIn: "/")) + "/models"
        }
        if lower.hasSuffix("/v1") {
            return "\(trimmed)/models"
        }
        return "\(trimmed)/v1/models"
    }

    private static func isPreset(_ provider: String) -> Bool {
        ["cerebras", "openai", "deepseek", "siliconflow", "custom"].contains(provider)
    }

    private static func defaultBaseURL(for provider: String) -> String {
        switch provider {
        case "openai":
            return "https://api.openai.com"
        case "deepseek":
            return "https://api.deepseek.com"
        case "siliconflow":
            return "https://api.siliconflow.cn"
        case "custom":
            return "http://127.0.0.1:8000"
        default:
            return "https://api.cerebras.ai"
        }
    }

    private static func anthropicModels() -> [AIModelCatalogEntry] {
        [
            "claude-opus-4-6",
            "claude-sonnet-4-6",
            "claude-haiku-4-5-20251001",
            "claude-sonnet-4-5-20250929",
            "claude-sonnet-4-20250514",
        ].map {
            AIModelCatalogEntry(id: $0, ownedBy: "anthropic")
        }
    }

    private static func codexOAuthModels() -> [AIModelCatalogEntry] {
        [
            "gpt-5.1-codex",
            "gpt-5.1-codex-max",
            "gpt-5.1-codex-mini",
            "gpt-5.2",
            "gpt-5.2-codex",
            "gpt-5.3-codex",
            "gpt-5.4",
            "gpt-5.4-mini",
        ].map {
            AIModelCatalogEntry(id: $0, ownedBy: "openai")
        }
    }
}

private extension String {
    var trimmedOrNil: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    func dropping(suffix: String) -> String {
        guard hasSuffix(suffix) else { return self }
        return String(dropLast(suffix.count))
    }
}
