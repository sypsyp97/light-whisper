import Foundation

enum ResultRecordWorkflow: String, Codable, CaseIterable, Identifiable, Sendable {
    case dictation
    case translation
    case assistant
    case unknown

    var id: String { rawValue }
}

struct ResultRecordCorrection: Codable, Equatable, Identifiable, Sendable {
    var id: String { "\(original)->\(corrected):\(type):\(source.rawValue)" }
    var original: String
    var corrected: String
    var type: String
    var source: CorrectionSource

    init(
        original: String,
        corrected: String,
        type: String = "replacement",
        source: CorrectionSource = .ai
    ) {
        self.original = original
        self.corrected = corrected
        self.type = type
        self.source = source
    }

    init(_ correction: AIPolishCorrection, source: CorrectionSource = .ai) {
        self.init(
            original: correction.original,
            corrected: correction.corrected,
            type: correction.type,
            source: source
        )
    }
}

struct CorrectionSubmissionContext: Equatable, Sendable {
    var recordID: String
    var workflow: ResultRecordWorkflow
    var rawOriginal: String?
    var displayedOriginal: String
    var correctedText: String
}

struct ResultRecord: Codable, Equatable, Identifiable, Sendable {
    var id: String
    var workflow: ResultRecordWorkflow
    var sourceText: String
    var originalText: String
    var editedText: String?
    var translationTarget: String?
    var durationSeconds: Double?
    var charCount: Int?
    var detectedLanguage: String?
    var createdAt: UInt64
    var updatedAt: UInt64
    var engine: String?
    var provider: String?
    var model: String?
    var keyTerms: [String]
    var corrections: [ResultRecordCorrection]
    var metadata: [String: String]
    var lastPastedText: String?
    var lastPastedAt: UInt64?
    var pinned: Bool

    enum CodingKeys: String, CodingKey {
        case id
        case workflow
        case sourceText = "source_text"
        case originalText = "original_text"
        case editedText = "edited_text"
        case translationTarget = "translation_target"
        case durationSeconds = "duration_seconds"
        case charCount = "char_count"
        case detectedLanguage = "detected_language"
        case createdAt = "created_at"
        case updatedAt = "updated_at"
        case engine
        case provider
        case model
        case keyTerms = "key_terms"
        case corrections
        case metadata
        case lastPastedText = "last_pasted_text"
        case lastPastedAt = "last_pasted_at"
        case pinned
    }

    init(
        id: String = UUID().uuidString,
        workflow: ResultRecordWorkflow,
        sourceText: String,
        originalText: String,
        editedText: String? = nil,
        translationTarget: String? = nil,
        durationSeconds: Double? = nil,
        charCount: Int? = nil,
        detectedLanguage: String? = nil,
        createdAt: UInt64 = HistoryTimestamp.now(),
        updatedAt: UInt64? = nil,
        engine: String? = nil,
        provider: String? = nil,
        model: String? = nil,
        keyTerms: [String] = [],
        corrections: [ResultRecordCorrection] = [],
        metadata: [String: String] = [:],
        lastPastedText: String? = nil,
        lastPastedAt: UInt64? = nil,
        pinned: Bool = false
    ) {
        self.id = id
        self.workflow = workflow
        self.sourceText = sourceText
        self.originalText = originalText
        self.editedText = editedText?.trimmedOrNil
        self.translationTarget = translationTarget?.trimmedOrNil
        self.durationSeconds = durationSeconds
        self.charCount = charCount
        self.detectedLanguage = detectedLanguage?.trimmedOrNil
        self.createdAt = createdAt
        self.updatedAt = updatedAt ?? createdAt
        self.engine = engine?.trimmedOrNil
        self.provider = provider?.trimmedOrNil
        self.model = model?.trimmedOrNil
        self.keyTerms = keyTerms.compactMap(\.trimmedOrNil)
        self.corrections = corrections
        self.metadata = metadata
        self.lastPastedText = lastPastedText?.trimmedOrNil
        self.lastPastedAt = lastPastedAt
        self.pinned = pinned
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let createdAt = try container.decodeIfPresent(UInt64.self, forKey: .createdAt)
            ?? HistoryTimestamp.now()
        id = try container.decodeIfPresent(String.self, forKey: .id) ?? UUID().uuidString
        workflow = try container.decodeIfPresent(ResultRecordWorkflow.self, forKey: .workflow) ?? .unknown
        sourceText = try container.decodeIfPresent(String.self, forKey: .sourceText) ?? ""
        originalText = try container.decodeIfPresent(String.self, forKey: .originalText) ?? ""
        editedText = try container.decodeIfPresent(String.self, forKey: .editedText)?.trimmedOrNil
        translationTarget = try container.decodeIfPresent(String.self, forKey: .translationTarget)?.trimmedOrNil
        durationSeconds = try container.decodeIfPresent(Double.self, forKey: .durationSeconds)
        charCount = try container.decodeIfPresent(Int.self, forKey: .charCount)
        detectedLanguage = try container.decodeIfPresent(String.self, forKey: .detectedLanguage)?.trimmedOrNil
        self.createdAt = createdAt
        updatedAt = try container.decodeIfPresent(UInt64.self, forKey: .updatedAt) ?? createdAt
        engine = try container.decodeIfPresent(String.self, forKey: .engine)?.trimmedOrNil
        provider = try container.decodeIfPresent(String.self, forKey: .provider)?.trimmedOrNil
        model = try container.decodeIfPresent(String.self, forKey: .model)?.trimmedOrNil
        keyTerms = try container.decodeIfPresent([String].self, forKey: .keyTerms)?.compactMap(\.trimmedOrNil) ?? []
        corrections = try container.decodeIfPresent([ResultRecordCorrection].self, forKey: .corrections) ?? []
        metadata = try container.decodeIfPresent([String: String].self, forKey: .metadata) ?? [:]
        lastPastedText = try container.decodeIfPresent(String.self, forKey: .lastPastedText)?.trimmedOrNil
        lastPastedAt = try container.decodeIfPresent(UInt64.self, forKey: .lastPastedAt)
        pinned = try container.decodeIfPresent(Bool.self, forKey: .pinned) ?? false
    }

    var currentText: String {
        editedText?.trimmedOrNil ?? originalText
    }

    var hasUserEdit: Bool {
        guard let editedText = editedText?.trimmedOrNil else {
            return false
        }
        return editedText != originalText.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    mutating func applyUserEdit(_ newValue: String?, at: UInt64 = HistoryTimestamp.now()) {
        let normalized = newValue?.trimmedOrNil
        let baseline = originalText.trimmingCharacters(in: .whitespacesAndNewlines)
        editedText = normalized == nil || normalized == baseline ? nil : normalized
        updatedAt = max(updatedAt, at)
    }

    mutating func markPasted(text: String? = nil, at: UInt64 = HistoryTimestamp.now()) {
        lastPastedText = text?.trimmedOrNil ?? currentText.trimmedOrNil
        lastPastedAt = at
        updatedAt = max(updatedAt, at)
    }
}

struct ResultHistorySnapshot: Equatable, Sendable {
    var records: [ResultRecord]
    var totalCount: Int
    var editedCount: Int
    var latestUpdatedAt: UInt64?
}

struct HistoryStore: Sendable {
    let url: URL
    let maxRecords: Int

    init(
        url: URL? = nil,
        maxRecords: Int = 200,
        fileManager: FileManager = .default
    ) throws {
        self.url = try url ?? Self.defaultURL(fileManager: fileManager)
        self.maxRecords = max(1, maxRecords)
    }

    static func defaultURL(fileManager: FileManager = .default) throws -> URL {
        try AppPaths.dataDirectory(fileManager: fileManager)
            .appendingPathComponent("history.json", isDirectory: false)
    }

    func load() throws -> [ResultRecord] {
        try loadDocument().records
    }

    func snapshot(limit: Int? = nil) throws -> ResultHistorySnapshot {
        let records = try load()
        let boundedRecords = limit.map { Array(records.prefix(max(0, $0))) } ?? records
        return ResultHistorySnapshot(
            records: boundedRecords,
            totalCount: records.count,
            editedCount: records.filter(\.hasUserEdit).count,
            latestUpdatedAt: records.map(\.updatedAt).max()
        )
    }

    func save(_ records: [ResultRecord]) throws {
        try writeDocument(ResultHistoryDocument(records: normalized(records)))
    }

    @discardableResult
    func append(_ record: ResultRecord) throws -> [ResultRecord] {
        var records = try load()
        records.removeAll { $0.id == record.id }
        records.insert(record, at: 0)
        let normalizedRecords = normalized(records)
        try writeDocument(ResultHistoryDocument(records: normalizedRecords))
        return normalizedRecords
    }

    @discardableResult
    func replace(_ record: ResultRecord) throws -> [ResultRecord] {
        var records = try load()
        if let index = records.firstIndex(where: { $0.id == record.id }) {
            records[index] = record
        } else {
            records.insert(record, at: 0)
        }
        let normalizedRecords = normalized(records)
        try writeDocument(ResultHistoryDocument(records: normalizedRecords))
        return normalizedRecords
    }

    func record(withID recordID: String) throws -> ResultRecord? {
        try load().first { $0.id == recordID }
    }

    @discardableResult
    func applyUserEdit(
        recordID: String,
        editedText: String?,
        editedAt: UInt64 = HistoryTimestamp.now()
    ) throws -> CorrectionSubmissionContext? {
        var records = try load()
        guard let index = records.firstIndex(where: { $0.id == recordID }) else {
            return nil
        }

        let before = records[index].currentText.trimmingCharacters(in: .whitespacesAndNewlines)
        records[index].applyUserEdit(editedText, at: editedAt)
        let after = records[index].currentText.trimmingCharacters(in: .whitespacesAndNewlines)
        let normalizedRecords = normalized(records)
        try writeDocument(ResultHistoryDocument(records: normalizedRecords))

        guard !before.isEmpty, !after.isEmpty, before != after else {
            return nil
        }

        let rawOriginal = records[index].sourceText.trimmedOrNil
        let contextualRawOriginal = rawOriginal == before ? nil : rawOriginal
        return CorrectionSubmissionContext(
            recordID: recordID,
            workflow: records[index].workflow,
            rawOriginal: contextualRawOriginal,
            displayedOriginal: before,
            correctedText: after
        )
    }

    @discardableResult
    func markPasted(
        recordID: String,
        pastedText: String? = nil,
        at: UInt64 = HistoryTimestamp.now()
    ) throws -> ResultRecord? {
        var records = try load()
        guard let index = records.firstIndex(where: { $0.id == recordID }) else {
            return nil
        }
        records[index].markPasted(text: pastedText, at: at)
        let updatedRecord = records[index]
        try writeDocument(ResultHistoryDocument(records: normalized(records)))
        return updatedRecord
    }

    @discardableResult
    func delete(recordID: String) throws -> [ResultRecord] {
        var records = try load()
        records.removeAll { $0.id == recordID }
        let normalizedRecords = normalized(records)
        try writeDocument(ResultHistoryDocument(records: normalizedRecords))
        return normalizedRecords
    }

    @discardableResult
    func prune(keepingMostRecent limit: Int) throws -> [ResultRecord] {
        let targetLimit = max(0, limit)
        let records = Array(try load().prefix(targetLimit))
        try writeDocument(ResultHistoryDocument(records: records))
        return records
    }

    func clear() throws {
        let parent = url.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: parent, withIntermediateDirectories: true)
        let emptyDocument = ResultHistoryDocument(records: [])
        try writeDocument(emptyDocument)
    }

    private func loadDocument() throws -> ResultHistoryDocument {
        guard FileManager.default.fileExists(atPath: url.path) else {
            return ResultHistoryDocument(records: [])
        }

        let data = try Data(contentsOf: url)
        if data.isEmpty {
            return ResultHistoryDocument(records: [])
        }

        let decoder = JSONDecoder()
        if let document = try? decoder.decode(ResultHistoryDocument.self, from: data) {
            return ResultHistoryDocument(records: normalized(document.records))
        }
        if let records = try? decoder.decode([ResultRecord].self, from: data) {
            return ResultHistoryDocument(records: normalized(records))
        }
        if let legacy = try? decoder.decode([LegacyHistoryItem].self, from: data) {
            return ResultHistoryDocument(records: normalized(legacy.map(\.upgradedRecord)))
        }
        throw HistoryStoreError.invalidDocument(url)
    }

    private func writeDocument(_ document: ResultHistoryDocument) throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        let data = try encoder.encode(document)
        let parent = url.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: parent, withIntermediateDirectories: true)
        try data.write(to: url, options: [.atomic])
    }

    private func normalized(_ records: [ResultRecord]) -> [ResultRecord] {
        records
            .reduce(into: [String: ResultRecord]()) { partial, record in
                let existing = partial[record.id]
                partial[record.id] = bestRecord(current: existing, candidate: record)
            }
            .values
            .sorted(by: isPreferredRecord)
            .prefix(maxRecords)
            .map { $0 }
    }

    private func bestRecord(current: ResultRecord?, candidate: ResultRecord) -> ResultRecord {
        guard let current else {
            return candidate
        }
        if isPreferredRecord(candidate, current) {
            return candidate
        }
        return current
    }

    private func isPreferredRecord(_ lhs: ResultRecord, _ rhs: ResultRecord) -> Bool {
        let lhsRank = (lhs.pinned ? 1 : 0, lhs.updatedAt, lhs.createdAt)
        let rhsRank = (rhs.pinned ? 1 : 0, rhs.updatedAt, rhs.createdAt)
        if lhsRank != rhsRank {
            return lhsRank > rhsRank
        }
        return lhs.id > rhs.id
    }
}

private struct ResultHistoryDocument: Codable {
    var schemaVersion: Int
    var records: [ResultRecord]

    init(schemaVersion: Int = 1, records: [ResultRecord]) {
        self.schemaVersion = schemaVersion
        self.records = records
    }
}

private struct LegacyHistoryItem: Decodable {
    var id: String
    var text: String
    var originalText: String
    var timestamp: Double?

    enum CodingKeys: String, CodingKey {
        case id
        case text
        case originalText
        case timestamp
    }

    var upgradedRecord: ResultRecord {
        let createdAt = timestamp.map(HistoryTimestamp.coerceToSeconds) ?? HistoryTimestamp.now()
        return ResultRecord(
            id: id,
            workflow: .dictation,
            sourceText: originalText,
            originalText: text,
            createdAt: createdAt,
            updatedAt: createdAt
        )
    }
}

private enum HistoryStoreError: LocalizedError {
    case invalidDocument(URL)

    var errorDescription: String? {
        switch self {
        case .invalidDocument(let url):
            return "The history document at \(url.path) could not be decoded."
        }
    }
}

private enum HistoryTimestamp {
    static func now() -> UInt64 {
        UInt64(Date().timeIntervalSince1970.rounded(.down))
    }

    static func coerceToSeconds(_ value: Double) -> UInt64 {
        if value > 1_000_000_000_000 {
            return UInt64((value / 1_000).rounded(.down))
        }
        return UInt64(value.rounded(.down))
    }
}

private extension String {
    var trimmedOrNil: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}
