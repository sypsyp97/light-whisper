import Foundation

public enum HotWordSource: String, Codable, Equatable, Sendable {
    case user
    case learned
}

public enum CorrectionSource: String, Codable, Equatable, Sendable {
    case ai
    case user
}

public struct HotWord: Codable, Equatable, Identifiable, Sendable {
    public var id: String { text }
    public var text: String
    public var weight: UInt8
    public var source: HotWordSource
    public var useCount: UInt32
    public var lastUsed: UInt64

    public init(
        text: String,
        weight: UInt8,
        source: HotWordSource,
        useCount: UInt32,
        lastUsed: UInt64
    ) {
        self.text = text
        self.weight = weight
        self.source = source
        self.useCount = useCount
        self.lastUsed = lastUsed
    }

    enum CodingKeys: String, CodingKey {
        case text
        case weight
        case source
        case useCount = "use_count"
        case lastUsed = "last_used"
    }
}

public struct CorrectionPattern: Codable, Equatable, Identifiable, Sendable {
    public var id: String { "\(original)->\(corrected)" }
    public var original: String
    public var corrected: String
    public var count: UInt32
    public var lastSeen: UInt64
    public var source: CorrectionSource

    enum CodingKeys: String, CodingKey {
        case original
        case corrected
        case count
        case lastSeen = "last_seen"
        case source
    }

    public init(
        original: String,
        corrected: String,
        count: UInt32,
        lastSeen: UInt64,
        source: CorrectionSource = .ai
    ) {
        self.original = original
        self.corrected = corrected
        self.count = count
        self.lastSeen = lastSeen
        self.source = source
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        original = try container.decode(String.self, forKey: .original)
        corrected = try container.decode(String.self, forKey: .corrected)
        count = try container.decode(UInt32.self, forKey: .count)
        lastSeen = try container.decode(UInt64.self, forKey: .lastSeen)
        source = try container.decodeIfPresent(CorrectionSource.self, forKey: .source) ?? .ai
    }
}

public struct VocabEntry: Codable, Equatable, Sendable {
    public var count: UInt32
    public var lastSeen: UInt64

    public init(count: UInt32, lastSeen: UInt64) {
        self.count = count
        self.lastSeen = lastSeen
    }

    enum CodingKeys: String, CodingKey {
        case count
        case lastSeen = "last_seen"
    }
}

public enum WebSearchProvider: String, Codable, CaseIterable, Equatable, Identifiable, Sendable {
    case modelNative = "model_native"
    case exa
    case tavily

    public var id: String { rawValue }
}

public struct WebSearchConfig: Codable, Equatable, Sendable {
    public var enabled: Bool
    public var provider: WebSearchProvider
    public var maxResults: UInt8

    enum CodingKeys: String, CodingKey {
        case enabled
        case provider
        case maxResults = "max_results"
    }

    public init(enabled: Bool = false, provider: WebSearchProvider = .modelNative, maxResults: UInt8 = 5) {
        self.enabled = enabled
        self.provider = provider
        self.maxResults = maxResults
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        enabled = try container.decodeIfPresent(Bool.self, forKey: .enabled) ?? false
        provider = try container.decodeIfPresent(WebSearchProvider.self, forKey: .provider) ?? .modelNative
        maxResults = try container.decodeIfPresent(UInt8.self, forKey: .maxResults) ?? 5
    }
}

public enum ApiFormat: String, Codable, CaseIterable, Equatable, Identifiable, Sendable {
    case openaiCompat = "openai_compat"
    case anthropic

    public var id: String { rawValue }
}

public enum OpenAIAuthMode: String, Codable, CaseIterable, Equatable, Identifiable, Sendable {
    case apiKey = "api_key"
    case oauth

    public var id: String { rawValue }
}

public enum LLMReasoningMode: String, Codable, CaseIterable, Equatable, Identifiable, Sendable {
    case providerDefault = "provider_default"
    case off
    case light
    case balanced
    case deep

    public var id: String { rawValue }
}

public struct CustomProvider: Codable, Equatable, Identifiable, Sendable {
    public var id: String
    public var name: String
    public var baseURL: String
    public var model: String
    public var apiFormat: ApiFormat

    enum CodingKeys: String, CodingKey {
        case id
        case name
        case baseURL = "base_url"
        case model
        case apiFormat = "api_format"
    }

    public init(
        id: String,
        name: String,
        baseURL: String,
        model: String,
        apiFormat: ApiFormat = .openaiCompat
    ) {
        self.id = id
        self.name = name
        self.baseURL = baseURL
        self.model = model
        self.apiFormat = apiFormat
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        name = try container.decode(String.self, forKey: .name)
        baseURL = try container.decode(String.self, forKey: .baseURL)
        model = try container.decode(String.self, forKey: .model)
        apiFormat = try container.decodeIfPresent(ApiFormat.self, forKey: .apiFormat) ?? .openaiCompat
    }
}

public struct LLMProviderConfig: Codable, Equatable, Sendable {
    public var active: String
    public var customBaseURL: String?
    public var customModel: String?
    public var reasoningMode: LLMReasoningMode
    public var polishReasoningMode: LLMReasoningMode?
    public var assistantReasoningMode: LLMReasoningMode?
    public var assistantUseSeparateModel: Bool
    public var assistantModel: String?
    public var assistantProvider: String?
    public var customProviders: [CustomProvider]
    public var validationUseSeparateModel: Bool
    public var validationProvider: String?
    public var validationModel: String?
    public var openAIAuthMode: OpenAIAuthMode?
    public var openAIFastMode: Bool

    enum CodingKeys: String, CodingKey {
        case active
        case customBaseURL = "custom_base_url"
        case customModel = "custom_model"
        case reasoningMode = "reasoning_mode"
        case polishReasoningMode = "polish_reasoning_mode"
        case assistantReasoningMode = "assistant_reasoning_mode"
        case assistantUseSeparateModel = "assistant_use_separate_model"
        case assistantModel = "assistant_model"
        case assistantProvider = "assistant_provider"
        case customProviders = "custom_providers"
        case validationUseSeparateModel = "validation_use_separate_model"
        case validationProvider = "validation_provider"
        case validationModel = "validation_model"
        case openAIAuthMode = "openai_auth_mode"
        case openAIFastMode = "openai_fast_mode"
    }

    public static func defaultValue() -> LLMProviderConfig {
        LLMProviderConfig(
            active: "cerebras",
            customBaseURL: nil,
            customModel: nil,
            reasoningMode: .providerDefault,
            polishReasoningMode: nil,
            assistantReasoningMode: nil,
            assistantUseSeparateModel: false,
            assistantModel: nil,
            assistantProvider: nil,
            customProviders: [],
            validationUseSeparateModel: false,
            validationProvider: nil,
            validationModel: nil,
            openAIAuthMode: nil,
            openAIFastMode: false
        )
    }

    public init(
        active: String,
        customBaseURL: String?,
        customModel: String?,
        reasoningMode: LLMReasoningMode,
        polishReasoningMode: LLMReasoningMode?,
        assistantReasoningMode: LLMReasoningMode?,
        assistantUseSeparateModel: Bool,
        assistantModel: String?,
        assistantProvider: String?,
        customProviders: [CustomProvider],
        validationUseSeparateModel: Bool,
        validationProvider: String?,
        validationModel: String?,
        openAIAuthMode: OpenAIAuthMode?,
        openAIFastMode: Bool
    ) {
        self.active = active
        self.customBaseURL = customBaseURL
        self.customModel = customModel
        self.reasoningMode = reasoningMode
        self.polishReasoningMode = polishReasoningMode
        self.assistantReasoningMode = assistantReasoningMode
        self.assistantUseSeparateModel = assistantUseSeparateModel
        self.assistantModel = assistantModel
        self.assistantProvider = assistantProvider
        self.customProviders = customProviders
        self.validationUseSeparateModel = validationUseSeparateModel
        self.validationProvider = validationProvider
        self.validationModel = validationModel
        self.openAIAuthMode = openAIAuthMode
        self.openAIFastMode = openAIFastMode
    }

    public init() {
        self = Self.defaultValue()
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let defaults = Self.defaultValue()
        active = try container.decodeIfPresent(String.self, forKey: .active) ?? defaults.active
        customBaseURL = try container.decodeIfPresent(String.self, forKey: .customBaseURL)
        customModel = try container.decodeIfPresent(String.self, forKey: .customModel)
        reasoningMode = try container.decodeIfPresent(LLMReasoningMode.self, forKey: .reasoningMode) ?? defaults.reasoningMode
        polishReasoningMode = try container.decodeIfPresent(LLMReasoningMode.self, forKey: .polishReasoningMode)
        assistantReasoningMode = try container.decodeIfPresent(LLMReasoningMode.self, forKey: .assistantReasoningMode)
        assistantUseSeparateModel = try container.decodeIfPresent(Bool.self, forKey: .assistantUseSeparateModel) ?? false
        assistantModel = try container.decodeIfPresent(String.self, forKey: .assistantModel)
        assistantProvider = try container.decodeIfPresent(String.self, forKey: .assistantProvider)
        customProviders = try container.decodeIfPresent([CustomProvider].self, forKey: .customProviders) ?? []
        validationUseSeparateModel = try container.decodeIfPresent(Bool.self, forKey: .validationUseSeparateModel) ?? false
        validationProvider = try container.decodeIfPresent(String.self, forKey: .validationProvider)
        validationModel = try container.decodeIfPresent(String.self, forKey: .validationModel)
        openAIAuthMode = try container.decodeIfPresent(OpenAIAuthMode.self, forKey: .openAIAuthMode)
        openAIFastMode = try container.decodeIfPresent(Bool.self, forKey: .openAIFastMode) ?? false
    }

    public func resolveActiveProvider() -> String {
        if Self.isBuiltinProvider(active) || customProviders.contains(where: { $0.id == active }) {
            return active
        }
        return customProviders.last?.id ?? "cerebras"
    }

    public func resolveAssistantProvider() -> String {
        guard assistantUseSeparateModel, let assistantProvider, !assistantProvider.isEmpty else {
            return resolveActiveProvider()
        }
        if Self.isBuiltinProvider(assistantProvider) || customProviders.contains(where: { $0.id == assistantProvider }) {
            return assistantProvider
        }
        return resolveActiveProvider()
    }

    public func resolveValidationProvider() -> String {
        guard validationUseSeparateModel, let validationProvider, !validationProvider.isEmpty else {
            return resolveActiveProvider()
        }
        if Self.isBuiltinProvider(validationProvider) || customProviders.contains(where: { $0.id == validationProvider }) {
            return validationProvider
        }
        return resolveActiveProvider()
    }

    public func resolvedPolishReasoningMode() -> LLMReasoningMode {
        polishReasoningMode ?? reasoningMode
    }

    public func resolvedAssistantReasoningMode() -> LLMReasoningMode {
        assistantReasoningMode ?? reasoningMode
    }

    public func resolvedAssistantModel() -> String? {
        guard assistantUseSeparateModel else {
            return nil
        }
        return assistantModel?.trimmingCharacters(in: .whitespacesAndNewlines).nilIfEmpty
    }

    public func resolvedValidationModel() -> String? {
        guard validationUseSeparateModel else {
            return nil
        }
        return validationModel?.trimmingCharacters(in: .whitespacesAndNewlines).nilIfEmpty
    }

    public static func isBuiltinProvider(_ value: String) -> Bool {
        ["cerebras", "openai", "deepseek", "siliconflow", "custom"].contains(value)
    }
}

public struct UserProfile: Codable, Equatable, Sendable {
    public var hotWords: [HotWord]
    public var correctionPatterns: [CorrectionPattern]
    public var vocabFrequency: [String: VocabEntry]
    public var totalTranscriptions: UInt64
    public var lastUpdated: UInt64
    public var llmProvider: LLMProviderConfig
    public var aiPolishEnabled: Bool
    public var dictationHotkey: String?
    public var translationTarget: String?
    public var translationHotkey: String?
    public var customPrompt: String?
    public var assistantHotkey: String?
    public var assistantSystemPrompt: String?
    public var assistantScreenContextEnabled: Bool
    public var aiPolishScreenContextEnabled: Bool
    public var blockedHotWords: [String]
    public var webSearch: WebSearchConfig
    public var correctionValidationEnabled: Bool
    public var lastCorrectionValidation: UInt64

    enum CodingKeys: String, CodingKey {
        case hotWords = "hot_words"
        case correctionPatterns = "correction_patterns"
        case vocabFrequency = "vocab_frequency"
        case totalTranscriptions = "total_transcriptions"
        case lastUpdated = "last_updated"
        case llmProvider = "llm_provider"
        case aiPolishEnabled = "ai_polish_enabled"
        case dictationHotkey = "dictation_hotkey"
        case translationTarget = "translation_target"
        case translationHotkey = "translation_hotkey"
        case customPrompt = "custom_prompt"
        case assistantHotkey = "assistant_hotkey"
        case assistantSystemPrompt = "assistant_system_prompt"
        case assistantScreenContextEnabled = "assistant_screen_context_enabled"
        case aiPolishScreenContextEnabled = "ai_polish_screen_context_enabled"
        case blockedHotWords = "blocked_hot_words"
        case webSearch = "web_search"
        case correctionValidationEnabled = "correction_validation_enabled"
        case lastCorrectionValidation = "last_correction_validation"
    }

    public static func defaultValue() -> UserProfile {
        UserProfile(
            hotWords: [],
            correctionPatterns: [],
            vocabFrequency: [:],
            totalTranscriptions: 0,
            lastUpdated: 0,
            llmProvider: LLMProviderConfig.defaultValue(),
            aiPolishEnabled: true,
            dictationHotkey: "f2",
            translationTarget: nil,
            translationHotkey: nil,
            customPrompt: nil,
            assistantHotkey: nil,
            assistantSystemPrompt: nil,
            assistantScreenContextEnabled: false,
            aiPolishScreenContextEnabled: false,
            blockedHotWords: [],
            webSearch: WebSearchConfig(),
            correctionValidationEnabled: false,
            lastCorrectionValidation: 0
        )
    }

    public init() {
        self = Self.defaultValue()
    }

    public init(
        hotWords: [HotWord],
        correctionPatterns: [CorrectionPattern],
        vocabFrequency: [String: VocabEntry],
        totalTranscriptions: UInt64,
        lastUpdated: UInt64,
        llmProvider: LLMProviderConfig,
        aiPolishEnabled: Bool,
        dictationHotkey: String?,
        translationTarget: String?,
        translationHotkey: String?,
        customPrompt: String?,
        assistantHotkey: String?,
        assistantSystemPrompt: String?,
        assistantScreenContextEnabled: Bool,
        aiPolishScreenContextEnabled: Bool,
        blockedHotWords: [String],
        webSearch: WebSearchConfig,
        correctionValidationEnabled: Bool,
        lastCorrectionValidation: UInt64
    ) {
        self.hotWords = hotWords
        self.correctionPatterns = correctionPatterns
        self.vocabFrequency = vocabFrequency
        self.totalTranscriptions = totalTranscriptions
        self.lastUpdated = lastUpdated
        self.llmProvider = llmProvider
        self.aiPolishEnabled = aiPolishEnabled
        self.dictationHotkey = dictationHotkey
        self.translationTarget = translationTarget
        self.translationHotkey = translationHotkey
        self.customPrompt = customPrompt
        self.assistantHotkey = assistantHotkey
        self.assistantSystemPrompt = assistantSystemPrompt
        self.assistantScreenContextEnabled = assistantScreenContextEnabled
        self.aiPolishScreenContextEnabled = aiPolishScreenContextEnabled
        self.blockedHotWords = blockedHotWords
        self.webSearch = webSearch
        self.correctionValidationEnabled = correctionValidationEnabled
        self.lastCorrectionValidation = lastCorrectionValidation
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let defaults = Self.defaultValue()
        hotWords = try container.decodeIfPresent([HotWord].self, forKey: .hotWords) ?? defaults.hotWords
        correctionPatterns = try container.decodeIfPresent([CorrectionPattern].self, forKey: .correctionPatterns)
            ?? defaults.correctionPatterns
        vocabFrequency = try container.decodeIfPresent([String: VocabEntry].self, forKey: .vocabFrequency)
            ?? defaults.vocabFrequency
        totalTranscriptions = try container.decodeIfPresent(UInt64.self, forKey: .totalTranscriptions)
            ?? defaults.totalTranscriptions
        lastUpdated = try container.decodeIfPresent(UInt64.self, forKey: .lastUpdated) ?? defaults.lastUpdated
        llmProvider = try container.decodeIfPresent(LLMProviderConfig.self, forKey: .llmProvider) ?? defaults.llmProvider
        aiPolishEnabled = try container.decodeIfPresent(Bool.self, forKey: .aiPolishEnabled)
            ?? defaults.aiPolishEnabled
        dictationHotkey = try container.decodeIfPresent(String.self, forKey: .dictationHotkey)
            ?? defaults.dictationHotkey
        translationTarget = try container.decodeIfPresent(String.self, forKey: .translationTarget)
        translationHotkey = try container.decodeIfPresent(String.self, forKey: .translationHotkey)
        customPrompt = try container.decodeIfPresent(String.self, forKey: .customPrompt)
        assistantHotkey = try container.decodeIfPresent(String.self, forKey: .assistantHotkey)
        assistantSystemPrompt = try container.decodeIfPresent(String.self, forKey: .assistantSystemPrompt)
        assistantScreenContextEnabled = try container.decodeIfPresent(Bool.self, forKey: .assistantScreenContextEnabled)
            ?? defaults.assistantScreenContextEnabled
        aiPolishScreenContextEnabled = try container.decodeIfPresent(Bool.self, forKey: .aiPolishScreenContextEnabled)
            ?? defaults.aiPolishScreenContextEnabled
        blockedHotWords = try container.decodeIfPresent([String].self, forKey: .blockedHotWords)
            ?? defaults.blockedHotWords
        webSearch = try container.decodeIfPresent(WebSearchConfig.self, forKey: .webSearch) ?? defaults.webSearch
        correctionValidationEnabled = try container.decodeIfPresent(Bool.self, forKey: .correctionValidationEnabled)
            ?? defaults.correctionValidationEnabled
        lastCorrectionValidation = try container.decodeIfPresent(UInt64.self, forKey: .lastCorrectionValidation)
            ?? defaults.lastCorrectionValidation
    }

    public func hotWordTexts(limit: Int) -> [String] {
        hotWords
            .sorted {
                if $0.weight == $1.weight {
                    return $0.useCount > $1.useCount
                }
                return $0.weight > $1.weight
            }
            .prefix(limit)
            .map(\.text)
    }

    public func relevantCorrections(input: String, limit: Int) -> [CorrectionPattern] {
        correctionPatterns
            .filter { !$0.original.isEmpty && input.contains($0.original) }
            .sorted {
                if $0.source != $1.source {
                    return $0.source == .user
                }
                return $0.count > $1.count
            }
            .prefix(limit)
            .map { $0 }
    }
}

public enum UserProfileNormalizer {
    public static func normalize(_ profile: inout UserProfile) {
        migrateCustomProvider(&profile.llmProvider)
        migrateReasoningModes(&profile.llmProvider)
    }

    private static func migrateReasoningModes(_ config: inout LLMProviderConfig) {
        if config.polishReasoningMode == nil {
            config.polishReasoningMode = config.reasoningMode
        }
        if config.assistantReasoningMode == nil {
            config.assistantReasoningMode = config.reasoningMode
        }
    }

    private static func migrateCustomProvider(_ config: inout LLMProviderConfig) {
        guard config.active == "custom", config.customProviders.isEmpty else {
            return
        }

        let baseURL = config.customBaseURL?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let model = config.customModel?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        guard !baseURL.isEmpty || !model.isEmpty else {
            return
        }

        config.customProviders = [
            CustomProvider(
                id: "custom_migrated",
                name: "Custom Compatible",
                baseURL: baseURL,
                model: model,
                apiFormat: .openaiCompat
            ),
        ]
        config.active = "custom_migrated"
        config.customBaseURL = nil
        config.customModel = nil
    }
}

private extension String {
    var nilIfEmpty: String? {
        isEmpty ? nil : self
    }
}
