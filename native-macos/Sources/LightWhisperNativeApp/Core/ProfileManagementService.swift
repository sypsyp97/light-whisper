import Foundation

enum ProfileImportStrategy: String, Codable, CaseIterable, Identifiable, Sendable {
    case replace
    case mergePreferImported = "merge_prefer_imported"
    case mergePreferExisting = "merge_prefer_existing"

    var id: String { rawValue }
}

struct ProfileCleanupStats: Equatable, Sendable {
    var removedHotWords: Int = 0
    var removedCorrections: Int = 0
}

struct LearnedCorrectionPair: Equatable, Hashable, Identifiable, Sendable {
    var id: String { "\(original)->\(corrected)" }
    var original: String
    var corrected: String
}

struct CorrectionLearningResult: Equatable, Sendable {
    var learnedCorrections: [LearnedCorrectionPair]
    var promotedHotWords: [String]
    var totalCorrectionPatterns: Int
}

struct CorrectionAuditCandidate: Equatable, Identifiable, Sendable {
    var id: String { "\(original)->\(corrected)" }
    var original: String
    var corrected: String
    var count: UInt32
    var lastSeen: UInt64
    var ordinal: Int
}

struct CorrectionAuditBatch: Equatable, Identifiable, Sendable {
    var id: String { "batch-\(batchNumber)" }
    var batchNumber: Int
    var rules: [CorrectionAuditCandidate]

    var prompt: String {
        let rulesText = rules
            .map { "\($0.ordinal). \"\($0.original)\" -> \"\($0.corrected)\"" }
            .joined(separator: "\n")

        return """
        Below are \(rules.count) AI-learned ASR correction rules. Review each rule.

        Reasonable rules:
        - homophone or near-homophone fixes
        - technical term casing or formatting fixes
        - stable ASR misrecognition repair

        Unreasonable rules:
        - semantically unrelated replacement
        - conversational fragments accidentally learned as rules
        - over-generalized common-word replacement

        Rules:
        \(rulesText)

        Return JSON array of invalid rule numbers, for example [2,5]. Return [] if all rules are valid.
        """
    }
}

enum ProfileManagementError: LocalizedError {
    case invalidImportPayload(String)

    var errorDescription: String? {
        switch self {
        case .invalidImportPayload(let reason):
            return "The user profile payload is invalid: \(reason)"
        }
    }
}

enum ProfileManagementService {
    private static let maxCorrectionPatterns = 500
    private static let maxHotWords = 300
    private static let maxSegmentChars = 12
    private static let maxHotWordChars = 24
    private static let maxUserHotWordChars = 80

    static func load(fileManager: FileManager = .default) throws -> UserProfile {
        let store = try JSONFileStore<UserProfile>(url: AppPaths.userProfileURL(fileManager: fileManager))
        var profile = try store.load(defaultValue: UserProfile.defaultValue())
        _ = normalize(&profile)
        return profile
    }

    static func save(_ profile: UserProfile, fileManager: FileManager = .default) throws {
        var normalizedProfile = profile
        _ = normalize(&normalizedProfile)
        let store = try JSONFileStore<UserProfile>(url: AppPaths.userProfileURL(fileManager: fileManager))
        try store.save(normalizedProfile)
    }

    @discardableResult
    static func normalize(_ profile: inout UserProfile) -> ProfileCleanupStats {
        UserProfileNormalizer.normalize(&profile)
        return cleanup(&profile)
    }

    static func exportProfile(_ profile: UserProfile) throws -> String {
        var normalizedProfile = profile
        _ = normalize(&normalizedProfile)
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        let data = try encoder.encode(normalizedProfile)
        guard let json = String(data: data, encoding: .utf8) else {
            throw ProfileManagementError.invalidImportPayload("Could not encode UTF-8 text.")
        }
        return json
    }

    static func importProfile(
        from json: String,
        into existingProfile: UserProfile? = nil,
        strategy: ProfileImportStrategy = .replace
    ) throws -> UserProfile {
        guard let data = json.data(using: .utf8) else {
            throw ProfileManagementError.invalidImportPayload("Input is not valid UTF-8.")
        }

        var importedProfile: UserProfile
        do {
            importedProfile = try JSONDecoder().decode(UserProfile.self, from: data)
        } catch {
            throw ProfileManagementError.invalidImportPayload(error.localizedDescription)
        }

        let mergedProfile: UserProfile
        if let existingProfile {
            mergedProfile = merge(existing: existingProfile, imported: importedProfile, strategy: strategy)
        } else {
            mergedProfile = importedProfile
        }

        var normalizedProfile = mergedProfile
        _ = normalize(&normalizedProfile)
        return normalizedProfile
    }

    static func merge(
        existing: UserProfile,
        imported: UserProfile,
        strategy: ProfileImportStrategy
    ) -> UserProfile {
        switch strategy {
        case .replace:
            var replaced = imported
            _ = normalize(&replaced)
            return replaced
        case .mergePreferImported:
            return mergedProfile(primary: imported, secondary: existing)
        case .mergePreferExisting:
            return mergedProfile(primary: existing, secondary: imported)
        }
    }

    static func addHotWord(profile: inout UserProfile, text: String, weight: UInt8) {
        guard let (normalizedText, normalizedKey) = normalizeHotWordKey(text) else {
            return
        }

        let now = nowSecs()
        profile.blockedHotWords.removeAll { $0 == normalizedKey }

        if let index = profile.hotWords.firstIndex(where: {
            normalizeHotWordKey($0.text)?.1 == normalizedKey
        }) {
            profile.hotWords[index].text = normalizedText
            profile.hotWords[index].weight = max(1, min(5, weight))
            profile.hotWords[index].source = .user
            profile.hotWords[index].lastUsed = now
        } else {
            profile.hotWords.append(
                HotWord(
                    text: normalizedText,
                    weight: max(1, min(5, weight)),
                    source: .user,
                    useCount: 0,
                    lastUsed: now
                )
            )
        }

        _ = cleanup(&profile)
        profile.lastUpdated = now
    }

    static func removeHotWord(profile: inout UserProfile, text: String) {
        let now = nowSecs()
        if let (_, key) = normalizeHotWordKey(text) {
            if !profile.blockedHotWords.contains(key) {
                profile.blockedHotWords.append(key)
            }
            profile.hotWords.removeAll {
                normalizeHotWordKey($0.text)?.1 == key
            }
            profile.vocabFrequency = profile.vocabFrequency.filter { normalizeHotWordKey($0.key)?.1 != key }
        } else {
            profile.hotWords.removeAll { $0.text == text }
        }

        _ = cleanup(&profile)
        profile.lastUpdated = now
    }

    @discardableResult
    static func removeCorrection(
        profile: inout UserProfile,
        original: String,
        corrected: String
    ) -> Bool {
        let before = profile.correctionPatterns.count
        profile.correctionPatterns.removeAll {
            $0.original == original && $0.corrected == corrected
        }
        let removed = before != profile.correctionPatterns.count
        if removed {
            profile.lastUpdated = nowSecs()
        }
        return removed
    }

    @discardableResult
    static func submitUserCorrection(
        profile: inout UserProfile,
        original: String,
        corrected: String,
        rawOriginal: String? = nil
    ) -> CorrectionLearningResult {
        submitUserCorrection(
            profile: &profile,
            submission: CorrectionSubmissionContext(
                recordID: UUID().uuidString,
                workflow: .dictation,
                rawOriginal: rawOriginal,
                displayedOriginal: original,
                correctedText: corrected
            )
        )
    }

    @discardableResult
    static func submitUserCorrection(
        profile: inout UserProfile,
        submission: CorrectionSubmissionContext
    ) -> CorrectionLearningResult {
        let corrected = submission.correctedText.trimmingCharacters(in: .whitespacesAndNewlines)
        let original = submission.displayedOriginal.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !original.isEmpty, !corrected.isEmpty, original != corrected else {
            return CorrectionLearningResult(
                learnedCorrections: [],
                promotedHotWords: [],
                totalCorrectionPatterns: profile.correctionPatterns.count
            )
        }

        let baselines = [submission.rawOriginal?.trimmedOrNil, original]
            .compactMap { $0 }
        let pairs = collectDiffCorrectionPairs(baselines: baselines, corrected: corrected)

        if !pairs.isEmpty {
            return learnFromStructured(
                profile: &profile,
                corrections: pairs,
                keyTerms: [],
                source: .user
            )
        }

        return learnFromCorrection(
            profile: &profile,
            original: original,
            polished: corrected,
            source: .user
        )
    }

    @discardableResult
    static func learnFromCorrection(
        profile: inout UserProfile,
        original: String,
        polished: String,
        source: CorrectionSource
    ) -> CorrectionLearningResult {
        let original = original.trimmingCharacters(in: .whitespacesAndNewlines)
        let polished = polished.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !original.isEmpty, !polished.isEmpty, original != polished else {
            return CorrectionLearningResult(
                learnedCorrections: [],
                promotedHotWords: [],
                totalCorrectionPatterns: profile.correctionPatterns.count
            )
        }

        let now = nowSecs()
        let initialCount: UInt32 = source == .user ? 3 : 1
        profile.totalTranscriptions = profile.totalTranscriptions.saturatingAdd(1)
        profile.lastUpdated = now

        let pairs = collectDiffCorrectionPairs(baselines: [original], corrected: polished)
        var learnedPairs: [LearnedCorrectionPair] = []
        for pair in pairs {
            if upsertCorrection(
                patterns: &profile.correctionPatterns,
                original: pair.original,
                corrected: pair.corrected,
                initialCount: initialCount,
                source: source,
                now: now
            ) {
                learnedPairs.append(pair)
            }
        }

        let promoted = finalizeLearning(&profile)
        return CorrectionLearningResult(
            learnedCorrections: learnedPairs,
            promotedHotWords: promoted,
            totalCorrectionPatterns: profile.correctionPatterns.count
        )
    }

    @discardableResult
    static func learnFromStructured(
        profile: inout UserProfile,
        corrections: [AIPolishCorrection],
        keyTerms: [String],
        source: CorrectionSource = .ai
    ) -> CorrectionLearningResult {
        learnFromStructured(
            profile: &profile,
            corrections: corrections.map {
                LearnedCorrectionPair(original: $0.original, corrected: $0.corrected)
            },
            keyTerms: keyTerms,
            source: source
        )
    }

    @discardableResult
    static func learnFromPolishResult(
        profile: inout UserProfile,
        result: AIPolishResult,
        source: CorrectionSource = .ai
    ) -> CorrectionLearningResult {
        learnFromStructured(
            profile: &profile,
            corrections: result.corrections,
            keyTerms: result.keyTerms,
            source: source
        )
    }

    @discardableResult
    static func learnFromStructured(
        profile: inout UserProfile,
        corrections: [LearnedCorrectionPair],
        keyTerms: [String],
        source: CorrectionSource = .ai
    ) -> CorrectionLearningResult {
        let now = nowSecs()
        let initialCount: UInt32 = source == .user ? 3 : 1
        profile.totalTranscriptions = profile.totalTranscriptions.saturatingAdd(1)
        profile.lastUpdated = now

        var learnedPairs: [LearnedCorrectionPair] = []
        for pair in corrections {
            if upsertCorrection(
                patterns: &profile.correctionPatterns,
                original: pair.original,
                corrected: pair.corrected,
                initialCount: initialCount,
                source: source,
                now: now
            ) {
                learnedPairs.append(pair)
            }
        }

        updateVocabFrequency(
            vocab: &profile.vocabFrequency,
            words: keyTerms,
            now: now
        )
        let promoted = finalizeLearning(&profile)
        return CorrectionLearningResult(
            learnedCorrections: learnedPairs,
            promotedHotWords: promoted,
            totalCorrectionPatterns: profile.correctionPatterns.count
        )
    }

    static func auditCandidates(from profile: UserProfile) -> [CorrectionAuditCandidate] {
        profile.correctionPatterns
            .filter { $0.source == .ai }
            .sorted {
                if $0.count == $1.count {
                    return $0.lastSeen > $1.lastSeen
                }
                return $0.count > $1.count
            }
            .enumerated()
            .map { offset, pattern in
                CorrectionAuditCandidate(
                    original: pattern.original,
                    corrected: pattern.corrected,
                    count: pattern.count,
                    lastSeen: pattern.lastSeen,
                    ordinal: offset + 1
                )
            }
    }

    static func buildAuditBatches(
        from profile: UserProfile,
        batchSize: Int = 40
    ) -> [CorrectionAuditBatch] {
        let normalizedSize = max(1, batchSize)
        let candidates = auditCandidates(from: profile)
        guard !candidates.isEmpty else {
            return []
        }

        return stride(from: 0, to: candidates.count, by: normalizedSize).enumerated().map { index, start in
            let slice = candidates[start..<min(start + normalizedSize, candidates.count)]
            return CorrectionAuditBatch(batchNumber: index + 1, rules: Array(slice))
        }
    }

    static func parseAuditInvalidIndices(from raw: String) -> [Int] {
        let normalized = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let data = normalized.data(using: .utf8) else {
            return []
        }

        if let values = try? JSONSerialization.jsonObject(with: data) as? [Any] {
            return values.compactMap(Self.toInt)
        }
        if
            let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            let values = object.values.first(where: { $0 is [Any] }) as? [Any]
        {
            return values.compactMap(Self.toInt)
        }
        return []
    }

    @discardableResult
    static func applyAuditInvalidations(
        profile: inout UserProfile,
        invalidatedPairs: [LearnedCorrectionPair],
        validatedAt: UInt64 = nowSecs()
    ) -> Int {
        let invalidated = Set(invalidatedPairs)
        guard !invalidated.isEmpty else {
            markValidationRun(profile: &profile, at: validatedAt)
            return 0
        }

        let before = profile.correctionPatterns.count
        profile.correctionPatterns.removeAll { pattern in
            pattern.source == .ai
                && invalidated.contains(
                    LearnedCorrectionPair(original: pattern.original, corrected: pattern.corrected)
                )
        }
        let removed = before - profile.correctionPatterns.count
        markValidationRun(profile: &profile, at: validatedAt)
        return removed
    }

    static func markValidationRun(profile: inout UserProfile, at: UInt64 = nowSecs()) {
        profile.lastCorrectionValidation = at
        profile.lastUpdated = max(profile.lastUpdated, at)
    }

    @discardableResult
    private static func cleanup(_ profile: inout UserProfile) -> ProfileCleanupStats {
        sanitizeBlockedHotWords(&profile)
        let removedHotWords = sanitizeHotWords(&profile)
        let removedCorrections = sanitizeCorrections(&profile) + limitCorrectionPatterns(&profile)
        if removedHotWords > 0 || removedCorrections > 0 {
            profile.lastUpdated = nowSecs()
        }
        return ProfileCleanupStats(
            removedHotWords: removedHotWords,
            removedCorrections: removedCorrections
        )
    }

    private static func mergedProfile(primary: UserProfile, secondary: UserProfile) -> UserProfile {
        var merged = primary
        merged.hotWords = mergeHotWords(primary: primary.hotWords, secondary: secondary.hotWords)
        merged.correctionPatterns = mergeCorrections(
            primary: primary.correctionPatterns,
            secondary: secondary.correctionPatterns
        )
        merged.vocabFrequency = mergeVocabFrequency(
            primary: primary.vocabFrequency,
            secondary: secondary.vocabFrequency
        )
        merged.totalTranscriptions = primary.totalTranscriptions.saturatingAdd(secondary.totalTranscriptions)
        merged.lastUpdated = max(primary.lastUpdated, secondary.lastUpdated)
        merged.blockedHotWords = Array(
            Set(primary.blockedHotWords + secondary.blockedHotWords)
        ).sorted()
        merged.llmProvider = mergeProviderConfig(primary: primary.llmProvider, secondary: secondary.llmProvider)
        merged.translationTarget = primary.translationTarget?.trimmedOrNil ?? secondary.translationTarget?.trimmedOrNil
        merged.translationHotkey = primary.translationHotkey?.trimmedOrNil ?? secondary.translationHotkey?.trimmedOrNil
        merged.customPrompt = primary.customPrompt?.trimmedOrNil ?? secondary.customPrompt?.trimmedOrNil
        merged.assistantHotkey = primary.assistantHotkey?.trimmedOrNil ?? secondary.assistantHotkey?.trimmedOrNil
        merged.assistantSystemPrompt = primary.assistantSystemPrompt?.trimmedOrNil
            ?? secondary.assistantSystemPrompt?.trimmedOrNil
        merged.assistantScreenContextEnabled = primary.assistantScreenContextEnabled || secondary.assistantScreenContextEnabled
        merged.aiPolishScreenContextEnabled = primary.aiPolishScreenContextEnabled || secondary.aiPolishScreenContextEnabled
        merged.webSearch = primary.webSearch
        merged.correctionValidationEnabled = primary.correctionValidationEnabled || secondary.correctionValidationEnabled
        merged.lastCorrectionValidation = max(
            primary.lastCorrectionValidation,
            secondary.lastCorrectionValidation
        )
        _ = normalize(&merged)
        return merged
    }

    private static func mergeHotWords(primary: [HotWord], secondary: [HotWord]) -> [HotWord] {
        var merged: [String: HotWord] = [:]
        for candidate in secondary + primary {
            guard let (normalizedText, normalizedKey) = normalizeHotWordKey(candidate.text) else {
                continue
            }

            var normalizedCandidate = candidate
            normalizedCandidate.text = normalizedText
            normalizedCandidate.weight = max(1, min(5, candidate.weight))

            if var existing = merged[normalizedKey] {
                mergeHotWord(existing: &existing, candidate: normalizedCandidate)
                merged[normalizedKey] = existing
            } else {
                merged[normalizedKey] = normalizedCandidate
            }
        }

        return merged.values.sorted { lhs, rhs in
            hotWordPriority(lhs) > hotWordPriority(rhs)
        }
    }

    private static func mergeCorrections(
        primary: [CorrectionPattern],
        secondary: [CorrectionPattern]
    ) -> [CorrectionPattern] {
        var merged: [String: CorrectionPattern] = [:]
        for candidate in secondary + primary {
            let key = "\(candidate.original)->\(candidate.corrected)"
            if var existing = merged[key] {
                existing.count = existing.count.saturatingAdd(candidate.count)
                existing.lastSeen = max(existing.lastSeen, candidate.lastSeen)
                if candidate.source == .user {
                    existing.source = .user
                }
                merged[key] = existing
            } else {
                merged[key] = candidate
            }
        }
        return Array(merged.values)
    }

    private static func mergeVocabFrequency(
        primary: [String: VocabEntry],
        secondary: [String: VocabEntry]
    ) -> [String: VocabEntry] {
        var merged = secondary
        for (word, entry) in primary {
            if let existing = merged[word] {
                merged[word] = VocabEntry(
                    count: existing.count.saturatingAdd(entry.count),
                    lastSeen: max(existing.lastSeen, entry.lastSeen)
                )
            } else {
                merged[word] = entry
            }
        }
        return merged
    }

    private static func mergeProviderConfig(
        primary: LLMProviderConfig,
        secondary: LLMProviderConfig
    ) -> LLMProviderConfig {
        var merged = primary
        let secondaryProviders = secondary.customProviders.filter { secondaryProvider in
            !primary.customProviders.contains(where: { $0.id == secondaryProvider.id })
        }
        merged.customProviders.append(contentsOf: secondaryProviders)
        return merged
    }

    private static func sanitizeBlockedHotWords(_ profile: inout UserProfile) {
        var seen = Set<String>()
        profile.blockedHotWords = profile.blockedHotWords
            .compactMap { normalizeHotWordKey($0)?.1 }
            .filter { seen.insert($0).inserted }
            .sorted()
    }

    private static func sanitizeHotWords(_ profile: inout UserProfile) -> Int {
        let before = profile.hotWords.count
        var merged: [String: HotWord] = [:]

        for candidate in profile.hotWords {
            guard let (normalizedText, normalizedKey) = normalizeHotWordKey(candidate.text) else {
                continue
            }
            if profile.blockedHotWords.contains(normalizedKey) {
                continue
            }

            var normalizedCandidate = candidate
            normalizedCandidate.text = normalizedText
            normalizedCandidate.weight = max(1, min(5, candidate.weight))
            if !isReasonableHotWord(normalizedCandidate.text, source: normalizedCandidate.source) {
                continue
            }

            if var existing = merged[normalizedKey] {
                mergeHotWord(existing: &existing, candidate: normalizedCandidate)
                merged[normalizedKey] = existing
            } else {
                merged[normalizedKey] = normalizedCandidate
            }
        }

        profile.hotWords = merged.values
            .sorted { hotWordPriority($0) > hotWordPriority($1) }
            .prefix(maxHotWords)
            .map { $0 }

        return before.saturatingSubtract(profile.hotWords.count)
    }

    private static func sanitizeCorrections(_ profile: inout UserProfile) -> Int {
        let before = profile.correctionPatterns.count
        let reversePairs = Set(
            profile.correctionPatterns.compactMap { pattern -> LearnedCorrectionPair? in
                guard profile.correctionPatterns.contains(where: {
                    $0.original == pattern.corrected && $0.corrected == pattern.original
                }) else {
                    return nil
                }
                return LearnedCorrectionPair(original: pattern.original, corrected: pattern.corrected)
            }
        )

        let now = nowSecs()
        profile.correctionPatterns.removeAll { pattern in
            if pattern.source == .user {
                return false
            }

            let originalLength = pattern.original.count
            let correctedLength = pattern.corrected.count
            if originalLength > 15 || correctedLength > 15 {
                return true
            }
            if originalLength == 1 && correctedLength != 1 {
                return true
            }

            let longer = max(originalLength, correctedLength)
            let shorter = min(originalLength, correctedLength)
            if shorter >= 2 && longer > shorter * 3 {
                return true
            }

            if reversePairs.contains(LearnedCorrectionPair(original: pattern.original, corrected: pattern.corrected)) {
                return true
            }

            if pattern.count <= 1, now.saturatingSubtract(pattern.lastSeen) > 24 * 60 * 60 {
                return true
            }

            return false
        }

        return before.saturatingSubtract(profile.correctionPatterns.count)
    }

    private static func limitCorrectionPatterns(_ profile: inout UserProfile) -> Int {
        guard profile.correctionPatterns.count > maxCorrectionPatterns else {
            return 0
        }

        let before = profile.correctionPatterns.count
        profile.correctionPatterns.sort {
            if $0.count == $1.count {
                return $0.lastSeen > $1.lastSeen
            }
            return $0.count > $1.count
        }
        profile.correctionPatterns = Array(profile.correctionPatterns.prefix(maxCorrectionPatterns))
        return before.saturatingSubtract(profile.correctionPatterns.count)
    }

    @discardableResult
    private static func upsertCorrection(
        patterns: inout [CorrectionPattern],
        original: String,
        corrected: String,
        initialCount: UInt32,
        source: CorrectionSource,
        now: UInt64
    ) -> Bool {
        let original = original.trimmingCharacters(in: .whitespacesAndNewlines)
        let corrected = corrected.trimmingCharacters(in: .whitespacesAndNewlines)

        let originalLength = original.count
        let correctedLength = corrected.count
        guard
            !original.isEmpty,
            !corrected.isEmpty,
            original != corrected,
            originalLength <= maxSegmentChars,
            correctedLength <= maxSegmentChars
        else {
            return false
        }

        if originalLength == 1 && correctedLength != 1 {
            return false
        }

        let longer = max(originalLength, correctedLength)
        let shorter = min(originalLength, correctedLength)
        if shorter >= 2 && longer > shorter * 3 {
            return false
        }

        if patterns.contains(where: { $0.original == corrected && $0.corrected == original }) {
            return false
        }

        if let index = patterns.firstIndex(where: { $0.original == original && $0.corrected == corrected }) {
            patterns[index].count = patterns[index].count.saturatingAdd(1)
            patterns[index].lastSeen = now
            if source == .user {
                patterns[index].source = .user
            }
            return true
        }

        patterns.append(
            CorrectionPattern(
                original: original,
                corrected: corrected,
                count: initialCount,
                lastSeen: now,
                source: source
            )
        )
        return true
    }

    private static func updateVocabFrequency(
        vocab: inout [String: VocabEntry],
        words: [String],
        now: UInt64
    ) {
        for word in words.compactMap(\.trimmedOrNil) {
            guard word.count >= 2, isPotentialHotWord(word) else {
                continue
            }
            let existing = vocab[word] ?? VocabEntry(count: 0, lastSeen: 0)
            vocab[word] = VocabEntry(
                count: existing.count.saturatingAdd(1),
                lastSeen: max(existing.lastSeen, now)
            )
        }
    }

    private static func finalizeLearning(_ profile: inout UserProfile) -> [String] {
        let beforeHotWords = Set(profile.hotWords.map(\.text))
        promoteVocabToHotWords(&profile, threshold: 3)
        _ = cleanup(&profile)
        let afterHotWords = Set(profile.hotWords.map(\.text))
        return Array(afterHotWords.subtracting(beforeHotWords)).sorted()
    }

    private static func promoteVocabToHotWords(_ profile: inout UserProfile, threshold: UInt32) {
        let existing = Set(profile.hotWords.map { normalizeHotWordKey($0.text)?.1 ?? $0.text.lowercased() })

        let learned = profile.vocabFrequency.compactMap { word, entry -> HotWord? in
            guard entry.count >= threshold else {
                return nil
            }
            guard let (normalizedText, normalizedKey) = normalizeHotWordKey(word) else {
                return nil
            }
            guard !existing.contains(normalizedKey) else {
                return nil
            }
            guard !profile.blockedHotWords.contains(normalizedKey) else {
                return nil
            }
            guard normalizedText.count >= 2, isPotentialHotWord(normalizedText) else {
                return nil
            }
            return HotWord(
                text: normalizedText,
                weight: 2,
                source: .learned,
                useCount: entry.count,
                lastUsed: entry.lastSeen
            )
        }

        profile.hotWords.append(contentsOf: learned)
    }

    private static func collectDiffCorrectionPairs(
        baselines: [String],
        corrected: String
    ) -> [LearnedCorrectionPair] {
        guard !corrected.isEmpty else {
            return []
        }

        var seen = Set<LearnedCorrectionPair>()
        var pairs: [LearnedCorrectionPair] = []
        for baseline in baselines where !baseline.isEmpty && baseline != corrected {
            for pair in extractDiffSegments(original: baseline, polished: corrected) {
                if seen.insert(pair).inserted {
                    pairs.append(pair)
                }
            }
        }
        return pairs
    }

    private static func extractDiffSegments(
        original: String,
        polished: String
    ) -> [LearnedCorrectionPair] {
        let originalScalars = Array(original)
        let polishedScalars = Array(polished)
        let originalLength = originalScalars.count
        let polishedLength = polishedScalars.count

        var pairs: [LearnedCorrectionPair] = []
        var originalIndex = 0
        var polishedIndex = 0

        while originalIndex < originalLength && polishedIndex < polishedLength {
            if originalScalars[originalIndex] == polishedScalars[polishedIndex] {
                originalIndex += 1
                polishedIndex += 1
                continue
            }

            let maxSearch = 20
            var found = false
            var nextOriginal = originalIndex + 1
            var nextPolished = polishedIndex + 1

            search: for deltaOriginal in 0..<min(maxSearch, originalLength - originalIndex) {
                for deltaPolished in 0..<min(maxSearch, polishedLength - polishedIndex) {
                    if (deltaOriginal > 0 || deltaPolished > 0)
                        && originalScalars[originalIndex + deltaOriginal] == polishedScalars[polishedIndex + deltaPolished]
                    {
                        nextOriginal = originalIndex + deltaOriginal
                        nextPolished = polishedIndex + deltaPolished
                        found = true
                        break search
                    }
                }
            }

            guard found else {
                break
            }

            let originalSegment = String(originalScalars[originalIndex..<nextOriginal])
            let polishedSegment = String(polishedScalars[polishedIndex..<nextPolished])
            if !originalSegment.isEmpty, !polishedSegment.isEmpty, originalSegment.count <= 30 {
                pairs.append(
                    LearnedCorrectionPair(original: originalSegment, corrected: polishedSegment)
                )
            }

            originalIndex = nextOriginal
            polishedIndex = nextPolished
        }

        return pairs
    }

    private static func normalizeWhitespace(_ value: String) -> String {
        value
            .split(whereSeparator: \.isWhitespace)
            .joined(separator: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private static func normalizeHotWordKey(_ text: String) -> (String, String)? {
        let normalized = normalizeWhitespace(text)
        guard !normalized.isEmpty else {
            return nil
        }
        return (normalized, normalized.lowercased())
    }

    private static func hotWordPriority(_ word: HotWord) -> (Int, UInt8, UInt32, UInt64, Int) {
        let sourcePriority = word.source == .user ? 1 : 0
        return (
            sourcePriority,
            word.weight,
            word.useCount,
            word.lastUsed,
            word.text.count
        )
    }

    private static func mergeHotWord(existing: inout HotWord, candidate: HotWord) {
        if hotWordPriority(candidate) > hotWordPriority(existing) {
            existing.text = candidate.text
        }
        existing.weight = max(existing.weight, candidate.weight)
        existing.useCount = max(existing.useCount, candidate.useCount)
        existing.lastUsed = max(existing.lastUsed, candidate.lastUsed)
        if candidate.source == .user {
            existing.source = .user
        }
    }

    private static func isReasonableHotWord(_ text: String, source: HotWordSource) -> Bool {
        let charCount = text.count
        if source == .user {
            return (1...maxUserHotWordChars).contains(charCount)
                && !text.contains(where: { $0 == "\n" || $0 == "\r" || $0 == "\t" })
        }
        if !(2...maxHotWordChars).contains(charCount) {
            return false
        }
        if containsSentencePunctuation(text) {
            return false
        }
        if text.split(separator: " ").count > 3 {
            return false
        }
        if source == .learned && learnedHotWordLooksLikeSentence(text) {
            return false
        }
        return isPotentialHotWord(text)
    }

    private static func containsSentencePunctuation(_ text: String) -> Bool {
        text.contains { character in
            switch character {
            case "，", "。", "！", "？", "；", "：", "、", ",", ".", "!", "?", ";", ":", "\n", "\r", "\t":
                return true
            default:
                return false
            }
        }
    }

    private static func learnedHotWordLooksLikeSentence(_ text: String) -> Bool {
        let actionLikeCharacters: Set<Character> = ["请", "帮", "写", "说", "问", "想", "要", "给", "把", "做", "发", "改"]
        let actionCount = text.reduce(into: 0) { partial, character in
            if actionLikeCharacters.contains(character) {
                partial += 1
            }
        }
        let hasASCII = text.contains { $0.isASCII && $0.isLetter }
        return !hasASCII && text.count >= 6 && actionCount >= 2
    }

    private static func isPotentialHotWord(_ word: String) -> Bool {
        let stopWords: Set<String> = [
            "的", "了", "是", "在", "我", "有", "和", "就", "不", "人", "都", "一", "一个", "上", "也",
            "很", "到", "说", "要", "去", "你", "会", "着", "没有", "看", "好", "自己", "这", "他",
            "她", "它", "们", "那", "个", "什么", "怎么", "这个", "那个", "但是", "因为", "所以",
            "如果", "可以", "已经", "还是", "或者", "然后", "其实", "应该", "可能", "比较", "现在",
            "知道", "觉得", "时候", "这样", "那样",
        ]
        guard !stopWords.contains(word) else {
            return false
        }
        return word.contains { character in
            character.isNumber || character.isLetter || ("\u{4e00}"..."\u{9fff}").contains(character)
        }
    }

    private static func nowSecs() -> UInt64 {
        UInt64(Date().timeIntervalSince1970.rounded(.down))
    }

    private static func toInt(_ value: Any) -> Int? {
        if let int = value as? Int {
            return int
        }
        if let number = value as? NSNumber {
            return number.intValue
        }
        if let string = value as? String {
            return Int(string)
        }
        return nil
    }
}

private extension String {
    var trimmedOrNil: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}

private extension UInt64 {
    func saturatingAdd(_ other: UInt64) -> UInt64 {
        let (sum, overflow) = addingReportingOverflow(other)
        return overflow ? UInt64.max : sum
    }

    func saturatingSubtract(_ other: UInt64) -> UInt64 {
        let (difference, overflow) = subtractingReportingOverflow(other)
        return overflow ? 0 : difference
    }
}

private extension UInt32 {
    func saturatingAdd(_ other: UInt32) -> UInt32 {
        let (sum, overflow) = addingReportingOverflow(other)
        return overflow ? UInt32.max : sum
    }
}

private extension Int {
    func saturatingSubtract(_ other: Int) -> Int {
        Swift.max(0, self - other)
    }
}
