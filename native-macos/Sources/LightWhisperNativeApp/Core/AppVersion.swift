import Foundation

enum AppVersion {
    static func current(
        bundle: Bundle = .main,
        fileManager: FileManager = .default
    ) -> String {
        if let bundleVersion = bundle.infoDictionary?["CFBundleShortVersionString"] as? String,
           let normalized = bundleVersion.trimmedOrNil {
            return normalized
        }

        if let sourceVersion = sourceVersion(fileManager: fileManager) {
            return sourceVersion
        }

        return "0.0.0"
    }

    static func sourceVersion(fileManager: FileManager = .default) -> String? {
        let packageURL = repositoryRootURL(fileManager: fileManager)
            .appendingPathComponent("package.json", isDirectory: false)
        guard let data = fileManager.contents(atPath: packageURL.path),
              let package = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let version = package["version"] as? String
        else {
            return nil
        }
        return version.trimmedOrNil
    }

    private static func repositoryRootURL(fileManager: FileManager) -> URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
    }
}

private extension String {
    var trimmedOrNil: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}
