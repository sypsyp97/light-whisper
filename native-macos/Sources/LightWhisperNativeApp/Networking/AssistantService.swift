import Foundation

struct AssistantRequest: Sendable {
    var request: String
    var profile: UserProfile
    var appContext: String?
    var selectedText: String?
    var includeScreenContext: Bool
    var manualAPIKey: String?
    var sharedFallbackAPIKey: String?
    var endpointOverride: LLMEndpoint?
    var webSearchConfigOverride: WebSearchConfig?
    var webSearchQuery: String?
    var webSearchAPIKey: String?

    init(
        request: String,
        profile: UserProfile,
        appContext: String? = nil,
        selectedText: String? = nil,
        includeScreenContext: Bool = false,
        manualAPIKey: String? = nil,
        sharedFallbackAPIKey: String? = nil,
        endpointOverride: LLMEndpoint? = nil,
        webSearchConfigOverride: WebSearchConfig? = nil,
        webSearchQuery: String? = nil,
        webSearchAPIKey: String? = nil
    ) {
        self.request = request
        self.profile = profile
        self.appContext = appContext
        self.selectedText = selectedText
        self.includeScreenContext = includeScreenContext
        self.manualAPIKey = manualAPIKey
        self.sharedFallbackAPIKey = sharedFallbackAPIKey
        self.endpointOverride = endpointOverride
        self.webSearchConfigOverride = webSearchConfigOverride
        self.webSearchQuery = webSearchQuery
        self.webSearchAPIKey = webSearchAPIKey
    }
}

struct AssistantResponse: Equatable, Sendable {
    let request: String
    let content: String
    let rawResponse: String
    let endpoint: LLMEndpoint
    let searchProvider: WebSearchProvider?
    let searchResults: [SearchResult]
    let searchContext: String?
    let warnings: [String]
    let usedScreenContext: Bool

    var usedWebSearch: Bool {
        searchProvider == .modelNative || !searchResults.isEmpty
    }
}

enum AssistantService {
    static func resolveAPIKey(
        profile: UserProfile,
        assistantAPIKey: String,
        polishAPIKey: String,
        storedKeys: [String: String]
    ) -> String {
        if let assistantAPIKey = assistantAPIKey.trimmedOrNil {
            return assistantAPIKey
        }

        let assistantProvider = profile.llmProvider.resolveAssistantProvider()
        let assistantKeyUser = LLMProviderCatalog.keychainUser(for: assistantProvider)
        if let storedAssistantKey = storedKeys[assistantKeyUser]?.trimmedOrNil {
            return storedAssistantKey
        }

        if assistantProvider == profile.llmProvider.resolveActiveProvider() {
            if let polishAPIKey = polishAPIKey.trimmedOrNil {
                return polishAPIKey
            }

            let activeKeyUser = LLMProviderCatalog.keychainUser(for: profile.llmProvider.resolveActiveProvider())
            if let storedSharedKey = storedKeys[activeKeyUser]?.trimmedOrNil {
                return storedSharedKey
            }
        }

        return ""
    }

    static func resolveAPIKey(
        profile: UserProfile,
        keychainStore: KeychainStore,
        assistantAPIKey: String?,
        polishAPIKey: String?
    ) throws -> String {
        let assistantProvider = profile.llmProvider.resolveAssistantProvider()
        if let assistantAPIKey = assistantAPIKey?.trimmedOrNil {
            return assistantAPIKey
        }
        if let storedAssistantKey = try keychainStore.string(
            for: LLMProviderCatalog.keychainUser(for: assistantProvider)
        )?.trimmedOrNil {
            return storedAssistantKey
        }

        if assistantProvider == profile.llmProvider.resolveActiveProvider() {
            if let polishAPIKey = polishAPIKey?.trimmedOrNil {
                return polishAPIKey
            }
            if let storedSharedKey = try keychainStore.string(
                for: LLMProviderCatalog.keychainUser(for: profile.llmProvider.resolveActiveProvider())
            )?.trimmedOrNil {
                return storedSharedKey
            }
        }

        throw NativeLLMServiceError.missingAPIKey(assistantProvider)
    }

    static func buildSystemPrompt(
        profile: UserProfile,
        webSearchResults: [SearchResult]
    ) -> String {
        var systemPrompt = AssistantPromptBuilder.buildSystemPrompt(profile: profile)
        if !webSearchResults.isEmpty {
            systemPrompt += "\n\n" + WebSearchService.renderContext(webSearchResults)
        }
        return systemPrompt
    }

    static func generate(
        request: AssistantRequest,
        keychainStore: KeychainStore = KeychainStore(),
        session: URLSession = .shared
    ) async throws -> AssistantResponse {
        let spokenRequest = request.request.trimmedOrNil
        guard let spokenRequest else {
            throw NativeLLMServiceError.missingInput("request")
        }

        let endpoint = request.endpointOverride ?? LLMProviderCatalog.assistantEndpoint(for: request.profile.llmProvider)
        let sharedFallbackAPIKey = shouldUseSharedFallbackKey(
            profile: request.profile,
            assistantProvider: endpoint.provider
        ) ? request.sharedFallbackAPIKey : nil
        let apiKey = try await NativeLLMRequestSupport.resolveProviderAPIKey(
            provider: endpoint.provider,
            config: request.profile.llmProvider,
            manualAPIKey: request.manualAPIKey,
            fallbackAPIKey: sharedFallbackAPIKey,
            keychainStore: keychainStore
        )

        var warnings: [String] = []
        let searchOutcome = try await resolveSearchOutcome(
            request: request,
            fallbackQuery: spokenRequest,
            keychainStore: keychainStore,
            session: session
        )
        warnings.append(contentsOf: searchOutcome.warnings)

        let systemPrompt = buildSystemPrompt(
            profile: request.profile,
            webSearchResults: searchOutcome.results
        )

        let userPrompt = AssistantPromptBuilder.renderUserContent(
            appContext: request.appContext,
            request: spokenRequest,
            selectedText: request.selectedText,
            hasScreenContext: request.includeScreenContext
        )
        var warningsFromFallbacks: [String] = []
        let useNativeWebSearch = (request.webSearchConfigOverride ?? request.profile.webSearch).enabled
            && (request.webSearchConfigOverride ?? request.profile.webSearch).provider == .modelNative
        var capturedScreenContext: [LLMTransportImageInput] = []
        if request.includeScreenContext {
            do {
                capturedScreenContext = try await ScreenContextService.captureFullScreenContext().map {
                    LLMTransportImageInput(mimeType: $0.mimeType, dataBase64: $0.dataBase64)
                }
            } catch {
                warningsFromFallbacks.append("Screen context capture failed: \(error.localizedDescription)")
            }
        }

        var requestImages = capturedScreenContext
        var requestUsesNativeWebSearch = useNativeWebSearch
        let rawResponse: String
        while true {
            do {
                rawResponse = try await LLMTransport.send(
                    endpoint: endpoint,
                    apiKey: apiKey,
                    system: systemPrompt,
                    user: userPrompt,
                    jsonOutput: false,
                    fastMode: endpoint.provider == "openai" && request.profile.llmProvider.openAIFastMode,
                    images: requestImages,
                    webSearch: requestUsesNativeWebSearch,
                    session: session
                )
                break
            } catch {
                let message = error.localizedDescription
                if !requestImages.isEmpty,
                   LLMTransport.looksLikeImageInputUnsupportedError(message)
                {
                    requestImages = []
                    warningsFromFallbacks.append("The selected assistant model does not support image input; retried without screen context.")
                    continue
                }
                if requestUsesNativeWebSearch,
                   LLMTransport.looksLikeWebSearchUnsupportedError(message)
                {
                    requestUsesNativeWebSearch = false
                    warningsFromFallbacks.append("The selected assistant model does not support model-native web search; retried without it.")
                    continue
                }
                throw error
            }
        }
        let content = rawResponse.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !content.isEmpty else {
            throw NativeLLMServiceError.emptyResponse("assistant")
        }

        return AssistantResponse(
            request: spokenRequest,
            content: content,
            rawResponse: rawResponse,
            endpoint: endpoint,
            searchProvider: searchOutcome.provider,
            searchResults: searchOutcome.results,
            searchContext: searchOutcome.searchContext,
            warnings: warnings + warningsFromFallbacks,
            usedScreenContext: !requestImages.isEmpty
        )
    }

    private static func shouldUseSharedFallbackKey(
        profile: UserProfile,
        assistantProvider: String
    ) -> Bool {
        assistantProvider == profile.llmProvider.resolveActiveProvider()
    }

    private static func resolveSearchOutcome(
        request: AssistantRequest,
        fallbackQuery: String,
        keychainStore: KeychainStore,
        session: URLSession
    ) async throws -> SearchOutcome {
        let config = request.webSearchConfigOverride ?? request.profile.webSearch
        guard config.enabled else {
            return SearchOutcome(provider: nil, results: [], searchContext: nil, warnings: [])
        }

        let query = request.webSearchQuery?.trimmedOrNil ?? fallbackQuery
        guard !query.isEmpty else {
            return SearchOutcome(
                provider: config.provider,
                results: [],
                searchContext: nil,
                warnings: ["Web search is enabled, but no search query was available."]
            )
        }

        switch config.provider {
        case .modelNative:
            return SearchOutcome(
                provider: .modelNative,
                results: [],
                searchContext: nil,
                warnings: []
            )
        case .exa:
            do {
                let results = try await WebSearchService.exaSearch(
                    query: query,
                    maxResults: config.maxResults,
                    session: session
                )
                let context = results.isEmpty ? nil : WebSearchService.renderContext(results)
                return SearchOutcome(provider: .exa, results: results, searchContext: context, warnings: [])
            } catch {
                return SearchOutcome(
                    provider: .exa,
                    results: [],
                    searchContext: nil,
                    warnings: ["Exa web search failed: \(error.localizedDescription)"]
                )
            }
        case .tavily:
            do {
                let apiKey = try NativeLLMRequestSupport.resolveTavilyAPIKey(
                    manualAPIKey: request.webSearchAPIKey,
                    keychainStore: keychainStore
                )
                let results = try await WebSearchService.tavilySearch(
                    apiKey: apiKey,
                    query: query,
                    maxResults: config.maxResults,
                    session: session
                )
                let context = results.isEmpty ? nil : WebSearchService.renderContext(results)
                return SearchOutcome(provider: .tavily, results: results, searchContext: context, warnings: [])
            } catch {
                return SearchOutcome(
                    provider: .tavily,
                    results: [],
                    searchContext: nil,
                    warnings: ["Tavily web search failed: \(error.localizedDescription)"]
                )
            }
        }
    }

    private struct SearchOutcome: Sendable {
        let provider: WebSearchProvider?
        let results: [SearchResult]
        let searchContext: String?
        let warnings: [String]
    }
}

private extension String {
    var trimmedOrNil: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}
