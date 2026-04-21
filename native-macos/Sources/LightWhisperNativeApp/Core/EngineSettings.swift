import Foundation

public enum EngineKind: String, Codable, CaseIterable, Identifiable, Sendable {
    case glmAsr = "glm-asr"
    case alibabaAsr = "alibaba-asr"

    public var id: String { rawValue }

    public static func normalized(_ engineValue: String) -> EngineKind {
        switch engineValue.trimmingCharacters(in: .whitespacesAndNewlines) {
        case EngineKind.glmAsr.rawValue:
            return .glmAsr
        case "alibaba-asr", "local", "sensevoice", "whisper":
            return .alibabaAsr
        default:
            return .alibabaAsr
        }
    }
}

public enum OnlineRegion: String, Codable, CaseIterable, Identifiable, Sendable {
    case international
    case domestic

    public var id: String { rawValue }
}

public struct EngineSettings: Codable, Equatable, Sendable {
    public static let defaultAlibabaModel = "qwen3-asr-flash"

    public var engine: EngineKind
    public var glmRegion: OnlineRegion
    public var alibabaRegion: OnlineRegion
    public var alibabaModel: String

    public init(
        engine: EngineKind = AppPaths.defaultEngine,
        glmRegion: OnlineRegion = .international,
        alibabaRegion: OnlineRegion = .international,
        alibabaModel: String = EngineSettings.defaultAlibabaModel
    ) {
        self.engine = engine
        self.glmRegion = glmRegion
        self.alibabaRegion = alibabaRegion
        self.alibabaModel = alibabaModel
    }

    public static func normalized(engineValue: String) -> EngineKind {
        EngineKind.normalized(engineValue)
    }

    public static func normalized(engineValue: String?) -> EngineKind {
        normalized(engineValue: engineValue ?? "")
    }

    public func onlineASRKeychainUser() -> String {
        switch engine {
        case .glmAsr:
            return "glm-asr-api-key"
        case .alibabaAsr:
            return alibabaRegion == .domestic ? "alibaba-asr-cn-api-key" : "alibaba-asr-intl-api-key"
        }
    }

    public var activeRegion: OnlineRegion {
        engine == .alibabaAsr ? alibabaRegion : glmRegion
    }

    public var activeEndpointBaseURL: String {
        switch engine {
        case .glmAsr:
            return glmRegion == .domestic ? "https://open.bigmodel.cn" : "https://api.z.ai"
        case .alibabaAsr:
            return alibabaRegion == .domestic
                ? "https://dashscope.aliyuncs.com"
                : "https://dashscope-intl.aliyuncs.com"
        }
    }

    public var alibabaModelUsesOmniChat: Bool {
        alibabaModel.localizedCaseInsensitiveContains("omni")
    }

    enum CodingKeys: String, CodingKey {
        case engine
        case glmRegion = "glm_endpoint"
        case alibabaRegion = "alibaba_region"
        case alibabaModel = "alibaba_model"
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        engine = Self.normalized(engineValue: try container.decodeIfPresent(String.self, forKey: .engine))
        glmRegion = try container.decodeIfPresent(OnlineRegion.self, forKey: .glmRegion) ?? .international
        alibabaRegion = try container.decodeIfPresent(OnlineRegion.self, forKey: .alibabaRegion) ?? .international
        alibabaModel = try container.decodeIfPresent(String.self, forKey: .alibabaModel)?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nonEmpty ?? Self.defaultAlibabaModel
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(engine.rawValue, forKey: .engine)
        try container.encode(glmRegion, forKey: .glmRegion)
        try container.encode(alibabaRegion, forKey: .alibabaRegion)
        try container.encode(alibabaModel, forKey: .alibabaModel)
    }
}

private extension String {
    var nonEmpty: String? {
        isEmpty ? nil : self
    }
}
