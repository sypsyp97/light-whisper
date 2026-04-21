import Foundation

public enum AppPaths {
    public static let bundleIdentifier = "com.light-whisper.desktop"
    public static let legacyBundleIdentifier = "com.light-whisper.app"
    public static let defaultEngine: EngineKind = .alibabaAsr

    public static func dataDirectory(fileManager: FileManager = .default) throws -> URL {
        let baseDirectory = applicationSupportDirectoryURL(fileManager: fileManager)
        let appDirectory = baseDirectory.appendingPathComponent(bundleIdentifier, isDirectory: true)
        let legacyDirectory = baseDirectory.appendingPathComponent(legacyBundleIdentifier, isDirectory: true)

        let mergeLegacy = (try? migrateLegacySupportDirectoryIfNeeded(
            appDirectory: appDirectory,
            legacyDirectory: legacyDirectory,
            fileManager: fileManager
        )) ?? false
        try fileManager.createDirectory(at: appDirectory, withIntermediateDirectories: true)
        cleanupLegacyState(fileManager: fileManager, removeLegacySupportDirectory: mergeLegacy)
        return appDirectory
    }

    public static let dataDirectoryURL: URL = (try? AppPaths.dataDirectory()) ?? AppPaths.applicationSupportDirectoryURL
        .appendingPathComponent(bundleIdentifier, isDirectory: true)

    public static var applicationSupportDirectoryURL: URL {
        applicationSupportDirectoryURL(fileManager: .default)
    }

    public static var legacyDataDirectoryURL: URL {
        applicationSupportDirectoryURL.appendingPathComponent(legacyBundleIdentifier, isDirectory: true)
    }

    @discardableResult
    public static func migrateLegacySupportDirectoryIfNeeded(
        appDirectory: URL,
        legacyDirectory: URL,
        fileManager: FileManager = .default
    ) throws -> Bool {
        guard fileManager.fileExists(atPath: legacyDirectory.path) else {
            return false
        }

        guard fileManager.fileExists(atPath: appDirectory.path) else {
            try fileManager.moveItem(at: legacyDirectory, to: appDirectory)
            return true
        }

        let legacyChildren = try fileManager.contentsOfDirectory(
            at: legacyDirectory,
            includingPropertiesForKeys: [.isDirectoryKey],
            options: []
        )
        var hasUnresolvedConflicts = false

        for childURL in legacyChildren {
            let values = try childURL.resourceValues(forKeys: [.isDirectoryKey])
            let targetURL = appDirectory.appendingPathComponent(
                childURL.lastPathComponent,
                isDirectory: values.isDirectory ?? false
            )

            if fileManager.fileExists(atPath: targetURL.path) {
                hasUnresolvedConflicts = true
                continue
            }

            try fileManager.moveItem(at: childURL, to: targetURL)
        }

        let remainingChildren = try fileManager.contentsOfDirectory(
            at: legacyDirectory,
            includingPropertiesForKeys: nil,
            options: []
        )
        if !remainingChildren.isEmpty || hasUnresolvedConflicts {
            return false
        }

        try fileManager.removeItem(at: legacyDirectory)
        return true
    }

    public static func cleanupLegacyState(
        fileManager: FileManager = .default,
        removeLegacySupportDirectory: Bool = false
    ) {
        var staleURLs = [
            cacheDirectoryURL(fileManager: fileManager)
                .appendingPathComponent(legacyBundleIdentifier, isDirectory: true),
            preferencePlistURL(for: legacyBundleIdentifier),
        ]

        if removeLegacySupportDirectory {
            staleURLs.insert(
                applicationSupportDirectoryURL(fileManager: fileManager)
                    .appendingPathComponent(legacyBundleIdentifier, isDirectory: true),
                at: 0
            )
        }

        for url in staleURLs where fileManager.fileExists(atPath: url.path) {
            try? fileManager.removeItem(at: url)
        }
    }

    public static var engineURL: URL {
        dataDirectoryURL.appendingPathComponent("engine.json", isDirectory: false)
    }

    public static var engineConfigURL: URL {
        engineURL
    }

    public static var userProfileURL: URL {
        dataDirectoryURL.appendingPathComponent("user_profile.json", isDirectory: false)
    }

    public static var transcriptHistoryURL: URL {
        dataDirectoryURL.appendingPathComponent("transcript_history.json", isDirectory: false)
    }

    public static func engineSettingsURL(fileManager: FileManager = .default) throws -> URL {
        try dataDirectory(fileManager: fileManager).appendingPathComponent("engine.json", isDirectory: false)
    }

    public static func userProfileURL(fileManager: FileManager = .default) throws -> URL {
        try dataDirectory(fileManager: fileManager).appendingPathComponent("user_profile.json", isDirectory: false)
    }

    public static func transcriptHistoryURL(fileManager: FileManager = .default) throws -> URL {
        try dataDirectory(fileManager: fileManager).appendingPathComponent("transcript_history.json", isDirectory: false)
    }

    private static func applicationSupportDirectoryURL(fileManager: FileManager) -> URL {
        fileManager.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSHomeDirectory())
                .appendingPathComponent("Library", isDirectory: true)
                .appendingPathComponent("Application Support", isDirectory: true)
    }

    private static func cacheDirectoryURL(fileManager: FileManager) -> URL {
        fileManager.urls(for: .cachesDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSHomeDirectory())
                .appendingPathComponent("Library", isDirectory: true)
                .appendingPathComponent("Caches", isDirectory: true)
    }

    private static func preferencePlistURL(for identifier: String) -> URL {
        URL(fileURLWithPath: NSHomeDirectory())
            .appendingPathComponent("Library", isDirectory: true)
            .appendingPathComponent("Preferences", isDirectory: true)
            .appendingPathComponent("\(identifier).plist", isDirectory: false)
    }
}

public struct JSONFileStore<Value: Codable> {
    let url: URL
    private let decoder = JSONDecoder()
    private let encoder = JSONEncoder()

    init(url: URL) {
        self.url = url
    }

    func load(defaultValue: @autoclosure () -> Value) throws -> Value {
        guard FileManager.default.fileExists(atPath: url.path) else {
            return defaultValue()
        }

        let data = try Data(contentsOf: url)
        return try decoder.decode(Value.self, from: data)
    }

    func save(_ value: Value) throws {
        let data = try encoder.encode(value)
        let parent = url.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: parent, withIntermediateDirectories: true)
        try data.write(to: url, options: [.atomic])
    }
}
