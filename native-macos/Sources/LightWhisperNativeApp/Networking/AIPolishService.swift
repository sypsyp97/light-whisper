import Foundation

enum NativeLLMServiceError: LocalizedError {
    case missingAPIKey(String)
    case missingInput(String)
    case emptyResponse(String)

    var errorDescription: String? {
        switch self {
        case .missingAPIKey(let provider):
            return "The API key for \(provider) is not configured."
        case .missingInput(let name):
            return "The \(name) input is required."
        case .emptyResponse(let service):
            return "The \(service) service returned an empty response."
        }
    }
}

enum AIPolishTranslationBehavior: Equatable, Sendable {
    case inheritProfile
    case disabled
    case target(String)
}

struct AIPolishCorrection: Codable, Equatable, Sendable {
    let original: String
    let corrected: String
    let type: String
}

struct AIPolishDecision: Equatable, Sendable {
    let shouldPolish: Bool
    let translationTarget: String?
}

struct AIPolishResult: Equatable, Sendable {
    let originalText: String
    let polishedText: String
    let corrections: [AIPolishCorrection]
    let keyTerms: [String]
    let rawResponse: String
    let endpoint: LLMEndpoint
    let usedScreenContext: Bool

    var didChange: Bool {
        polishedText != originalText
    }
}

struct AIPolishRequest: Sendable {
    var text: String
    var profile: UserProfile
    var appContext: String?
    var includeScreenContext: Bool
    var translationBehavior: AIPolishTranslationBehavior
    var manualAPIKey: String?
    var endpointOverride: LLMEndpoint?

    init(
        text: String,
        profile: UserProfile,
        appContext: String? = nil,
        includeScreenContext: Bool = false,
        translationBehavior: AIPolishTranslationBehavior = .inheritProfile,
        manualAPIKey: String? = nil,
        endpointOverride: LLMEndpoint? = nil
    ) {
        self.text = text
        self.profile = profile
        self.appContext = appContext
        self.includeScreenContext = includeScreenContext
        self.translationBehavior = translationBehavior
        self.manualAPIKey = manualAPIKey
        self.endpointOverride = endpointOverride
    }
}

struct AIPolishEditResult: Equatable, Sendable {
    let originalText: String
    let editedText: String
    let rawResponse: String
    let endpoint: LLMEndpoint

    var didChange: Bool {
        editedText != originalText
    }
}

struct AIPolishEditRequest: Sendable {
    var selectedText: String
    var instruction: String
    var profile: UserProfile
    var manualAPIKey: String?
    var endpointOverride: LLMEndpoint?

    init(
        selectedText: String,
        instruction: String,
        profile: UserProfile,
        manualAPIKey: String? = nil,
        endpointOverride: LLMEndpoint? = nil
    ) {
        self.selectedText = selectedText
        self.instruction = instruction
        self.profile = profile
        self.manualAPIKey = manualAPIKey
        self.endpointOverride = endpointOverride
    }
}

enum AIPolishService {
    static func processingDecision(
        transcript: String,
        profile: UserProfile,
        apiKey: String,
        translationTargetOverride: String?
    ) -> AIPolishDecision {
        let translationTarget: String?
        if let override = translationTargetOverride?.trimmedOrNil {
            translationTarget = override
        } else {
            translationTarget = profile.translationTarget?.trimmedOrNil
        }
        let hasCustomPrompt = profile.customPrompt?.trimmedOrNil != nil
        let hasRelevantCorrections = !profile.relevantCorrections(input: transcript, limit: 1).isEmpty
        let shouldPolish =
            profile.aiPolishEnabled
            && transcript.trimmedOrNil != nil
            && apiKey.trimmedOrNil != nil
            && (translationTarget != nil || hasCustomPrompt || hasRelevantCorrections)

        return AIPolishDecision(
            shouldPolish: shouldPolish,
            translationTarget: translationTarget
        )
    }

    static func polish(
        request: AIPolishRequest,
        keychainStore: KeychainStore = KeychainStore(),
        session: URLSession = .shared
    ) async throws -> AIPolishResult {
        let text = request.text.trimmedOrNil
        guard let text else {
            throw NativeLLMServiceError.missingInput("text")
        }

        let effectiveProfile = profile(
            from: request.profile,
            translationBehavior: request.translationBehavior
        )
        let endpoint = request.endpointOverride ?? LLMProviderCatalog.endpoint(for: effectiveProfile.llmProvider)
        let apiKey = try await NativeLLMRequestSupport.resolveProviderAPIKey(
            provider: endpoint.provider,
            config: effectiveProfile.llmProvider,
            manualAPIKey: request.manualAPIKey,
            keychainStore: keychainStore
        )

        let translationOverride: String? = switch request.translationBehavior {
        case .inheritProfile, .disabled:
            nil
        case .target(let value):
            value
        }

        let systemPrompt = AIPolishPromptBuilder.buildSystemPrompt(
            profile: effectiveProfile,
            inputText: text,
            translationTargetOverride: translationOverride
        )
        let userPrompt = renderUserContent(
            appContext: request.appContext,
            text: text,
            hasScreenContext: request.includeScreenContext
        )
        var capturedScreenContext: [LLMTransportImageInput] = []
        if request.includeScreenContext {
            do {
                capturedScreenContext = try await ScreenContextService.captureFullScreenContext().map {
                    LLMTransportImageInput(mimeType: $0.mimeType, dataBase64: $0.dataBase64)
                }
            } catch {
                capturedScreenContext = []
            }
        }

        var usedScreenContext = !capturedScreenContext.isEmpty
        let rawResponse: String
        do {
            rawResponse = try await LLMTransport.send(
                endpoint: endpoint,
                apiKey: apiKey,
                system: systemPrompt,
                user: userPrompt,
                jsonOutput: true,
                fastMode: endpoint.provider == "openai" && effectiveProfile.llmProvider.openAIFastMode,
                images: capturedScreenContext,
                session: session
            )
        } catch {
            if !capturedScreenContext.isEmpty,
               LLMTransport.looksLikeImageInputUnsupportedError(error.localizedDescription)
            {
                usedScreenContext = false
                rawResponse = try await LLMTransport.send(
                    endpoint: endpoint,
                    apiKey: apiKey,
                    system: systemPrompt,
                    user: userPrompt,
                    jsonOutput: true,
                    fastMode: endpoint.provider == "openai" && effectiveProfile.llmProvider.openAIFastMode,
                    images: [],
                    session: session
                )
            } else {
                throw error
            }
        }
        let trimmedResponse = rawResponse.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedResponse.isEmpty else {
            throw NativeLLMServiceError.emptyResponse("AI polish")
        }

        if let structured = NativeLLMStructuredPayload.decode(AIPolishPayload.self, from: trimmedResponse) {
            return AIPolishResult(
                originalText: text,
                polishedText: structured.polished,
                corrections: structured.corrections,
                keyTerms: structured.keyTerms,
                rawResponse: rawResponse,
                endpoint: endpoint,
                usedScreenContext: usedScreenContext
            )
        }

        return AIPolishResult(
            originalText: text,
            polishedText: trimmedResponse,
            corrections: [],
            keyTerms: [],
            rawResponse: rawResponse,
            endpoint: endpoint,
            usedScreenContext: usedScreenContext
        )
    }

    static func editSelectedText(
        request: AIPolishEditRequest,
        keychainStore: KeychainStore = KeychainStore(),
        session: URLSession = .shared
    ) async throws -> AIPolishEditResult {
        let selectedText = request.selectedText.trimmedOrNil
        guard let selectedText else {
            throw NativeLLMServiceError.missingInput("selectedText")
        }

        let instruction = request.instruction.trimmedOrNil
        guard let instruction else {
            throw NativeLLMServiceError.missingInput("instruction")
        }

        let endpoint = request.endpointOverride ?? LLMProviderCatalog.endpoint(for: request.profile.llmProvider)
        let apiKey = try await NativeLLMRequestSupport.resolveProviderAPIKey(
            provider: endpoint.provider,
            config: request.profile.llmProvider,
            manualAPIKey: request.manualAPIKey,
            keychainStore: keychainStore
        )
        let userPrompt = [
            PromptXML.wrap("selected_text", selectedText),
            PromptXML.wrap("edit_instruction", instruction),
        ].joined(separator: "\n\n")
        let rawResponse = try await LLMTransport.send(
            endpoint: endpoint,
            apiKey: apiKey,
            system: editSystemPrompt,
            user: userPrompt,
            jsonOutput: true,
            fastMode: endpoint.provider == "openai" && request.profile.llmProvider.openAIFastMode,
            session: session
        )
        let trimmedResponse = rawResponse.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedResponse.isEmpty else {
            throw NativeLLMServiceError.emptyResponse("AI text editing")
        }

        let editedText = NativeLLMStructuredPayload.extractEditResult(from: trimmedResponse) ?? trimmedResponse
        return AIPolishEditResult(
            originalText: selectedText,
            editedText: editedText,
            rawResponse: rawResponse,
            endpoint: endpoint
        )
    }

    static func renderUserContent(
        appContext: String?,
        text: String,
        hasScreenContext: Bool
    ) -> String {
        var sections: [String] = []
        if let appContext = appContext?.trimmedOrNil {
            sections.append(appContext)
        }
        if hasScreenContext {
            sections.append(PromptXML.wrap(
                "screen_context",
                "Screen context is attached. Use it only when correcting the current ASR text."
            ))
        }
        sections.append(PromptXML.wrap("asr_text", text))
        return sections.joined(separator: "\n\n")
    }

    private static let editSystemPrompt = """
    <role>
    You are a text editing assistant. The user selected a piece of text and dictated an edit instruction.
    Return the fully edited text only.
    </role>

    <instructions>
    1. Output JSON only.
    2. Treat <edit_instruction> as the requested operation and <selected_text> as the source text to transform.
    3. Follow rewrite, translation, summary, expansion, shortening, tone, formatting, and explanation requests directly.
    4. Preserve formatting unless the instruction explicitly asks to change it.
    5. If the instruction is ambiguous, make the smallest safe edit that satisfies it.
    </instructions>

    <output_format>
    <![CDATA[
    {"result":"Edited full text"}
    ]]>
    </output_format>
    """

    private static func profile(
        from profile: UserProfile,
        translationBehavior: AIPolishTranslationBehavior
    ) -> UserProfile {
        var resolved = profile
        switch translationBehavior {
        case .inheritProfile:
            break
        case .disabled:
            resolved.translationTarget = nil
        case .target(let target):
            resolved.translationTarget = target.trimmedOrNil
        }
        return resolved
    }
}

enum NativeLLMRequestSupport {
    static let tavilyKeychainUser = "web-search-tavily-key"

    static func resolveProviderAPIKey(
        provider: String,
        config: LLMProviderConfig,
        manualAPIKey: String?,
        fallbackAPIKey: String? = nil,
        keychainStore: KeychainStore,
        session: URLSession = .shared
    ) async throws -> String {
        let preferredManualAPIKey = manualAPIKey?.trimmedOrNil ?? fallbackAPIKey?.trimmedOrNil
        let resolved = try await CodexOAuthService.resolveProviderAPIKey(
            provider: provider,
            config: config,
            manualAPIKey: preferredManualAPIKey,
            keychainStore: keychainStore,
            session: session
        ).trimmingCharacters(in: .whitespacesAndNewlines)
        guard !resolved.isEmpty else {
            throw NativeLLMServiceError.missingAPIKey(provider)
        }
        return resolved
    }

    static func resolveTavilyAPIKey(
        manualAPIKey: String?,
        keychainStore: KeychainStore
    ) throws -> String {
        if let manualAPIKey = manualAPIKey?.trimmedOrNil {
            return manualAPIKey
        }
        if let stored = try keychainStore.string(for: tavilyKeychainUser)?.trimmedOrNil {
            return stored
        }
        throw NativeLLMServiceError.missingAPIKey("tavily")
    }
}

enum NativeLLMStructuredPayload {
    static func decode<T: Decodable>(_ type: T.Type, from raw: String) -> T? {
        let normalized = normalize(raw)
        guard let data = normalized.data(using: .utf8) else {
            return nil
        }
        let decoder = JSONDecoder()
        if let decoded = try? decoder.decode(T.self, from: data) {
            return decoded
        }
        if let wrapped = try? decoder.decode([T].self, from: data) {
            return wrapped.first
        }
        return nil
    }

    static func extractEditResult(from raw: String) -> String? {
        decode(EditPayload.self, from: raw)?.result
    }

    private static func normalize(_ raw: String) -> String {
        var normalized = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        while true {
            let next = stripCDATAWrapper(stripXMLWrapper(stripMarkdownCodeBlock(normalized)))
            if next == normalized {
                break
            }
            normalized = next
        }
        return extractJSONSegment(from: normalized) ?? normalized
    }

    private static func stripMarkdownCodeBlock(_ value: String) -> String {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.hasPrefix("```") else {
            return trimmed
        }
        guard let newline = trimmed.firstIndex(of: "\n") else {
            return trimmed
        }
        let afterFirstLine = trimmed[trimmed.index(after: newline)...]
        let content = afterFirstLine.trimmingCharacters(in: .whitespacesAndNewlines)
        guard content.hasSuffix("```") else {
            return content
        }
        return String(content.dropLast(3)).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private static func stripCDATAWrapper(_ value: String) -> String {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.hasPrefix("<![CDATA["), trimmed.hasSuffix("]]>") else {
            return trimmed
        }
        return String(trimmed.dropFirst(9).dropLast(3)).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private static func stripXMLWrapper(_ value: String) -> String {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        for tag in ["output", "response", "result"] {
            let prefix = "<\(tag)>"
            let suffix = "</\(tag)>"
            if trimmed.hasPrefix(prefix), trimmed.hasSuffix(suffix) {
                return String(trimmed.dropFirst(prefix.count).dropLast(suffix.count))
                    .trimmingCharacters(in: .whitespacesAndNewlines)
            }
        }
        return trimmed
    }

    private static func extractJSONSegment(from value: String) -> String? {
        let indices = Array(value.indices)
        for startPosition in indices.indices {
            let startIndex = indices[startPosition]
            let startCharacter = value[startIndex]
            let matchingCharacter: Character
            switch startCharacter {
            case "{":
                matchingCharacter = "}"
            case "[":
                matchingCharacter = "]"
            default:
                continue
            }

            var depth = 0
            var inString = false
            var escaped = false

            for currentPosition in startPosition..<indices.count {
                let currentIndex = indices[currentPosition]
                let character = value[currentIndex]

                if inString {
                    if escaped {
                        escaped = false
                        continue
                    }
                    switch character {
                    case "\\":
                        escaped = true
                    case "\"":
                        inString = false
                    default:
                        break
                    }
                    continue
                }

                switch character {
                case "\"":
                    inString = true
                case startCharacter:
                    depth += 1
                case matchingCharacter:
                    depth -= 1
                    if depth == 0 {
                        let endIndex = value.index(after: currentIndex)
                        return String(value[startIndex..<endIndex]).trimmingCharacters(in: .whitespacesAndNewlines)
                    }
                default:
                    break
                }
            }
        }
        return nil
    }

    private struct EditPayload: Decodable {
        let result: String
    }
}

private struct AIPolishPayload: Decodable {
    let polished: String
    let corrections: [AIPolishCorrection]
    let keyTerms: [String]

    enum CodingKeys: String, CodingKey {
        case polished
        case corrections
        case keyTerms = "key_terms"
    }
}

private extension String {
    var trimmedOrNil: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}
