import AppKit
import Foundation

struct AppUpdateInfo: Equatable, Sendable {
    let available: Bool
    let currentVersion: String
    let latestVersion: String?
    let notes: String?
    let publishedAt: String?
    let releaseURL: String?
}

enum UpdaterService {
    private static let latestReleaseURL = URL(string: "https://api.github.com/repos/sypsyp97/light-whisper/releases/latest")!
    private static let releasesPageURL = "https://github.com/sypsyp97/light-whisper/releases"
    private static let userAgent = "light-whisper-native"

    static func checkForUpdates(
        currentVersion: String,
        session: URLSession = .shared
    ) async throws -> AppUpdateInfo {
        var request = URLRequest(url: latestReleaseURL)
        request.setValue(userAgent, forHTTPHeaderField: "User-Agent")
        request.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        let (data, response) = try await session.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse else {
            throw LLMTransportError.invalidResponse
        }
        guard (200..<300).contains(httpResponse.statusCode) else {
            throw LLMTransportError.requestFailed("GitHub release HTTP \(httpResponse.statusCode): \(String(decoding: data, as: UTF8.self))")
        }

        let release = try JSONDecoder().decode(GitHubRelease.self, from: data)
        let latestVersion = normalizeVersion(release.tagName)
        return AppUpdateInfo(
            available: isVersionNewer(latest: latestVersion, current: currentVersion),
            currentVersion: currentVersion,
            latestVersion: latestVersion,
            notes: release.body?.nilIfBlank,
            publishedAt: release.publishedAt,
            releaseURL: release.htmlURL
        )
    }

    static func openReleasePage(_ url: String? = nil) {
        let target = url?.nilIfBlank ?? releasesPageURL
        NSWorkspace.shared.open(URL(string: target)!)
    }

    static func normalizeVersion(_ value: String) -> String {
        value.trimmingCharacters(in: .whitespacesAndNewlines).trimmingPrefix("v")
    }

    static func isVersionNewer(latest: String, current: String) -> Bool {
        let latestParts = parseVersion(latest)
        let currentParts = parseVersion(current)
        let count = max(latestParts.count, currentParts.count)
        for index in 0..<count {
            let latestPart = latestParts.indices.contains(index) ? latestParts[index] : 0
            let currentPart = currentParts.indices.contains(index) ? currentParts[index] : 0
            if latestPart > currentPart { return true }
            if latestPart < currentPart { return false }
        }
        return false
    }

    private static func parseVersion(_ value: String) -> [Int] {
        normalizeVersion(value)
            .split(separator: ".")
            .map { part in
                let digits = part.prefix { $0.isNumber }
                return Int(digits) ?? 0
            }
    }
}

private struct GitHubRelease: Decodable {
    let tagName: String
    let body: String?
    let publishedAt: String?
    let htmlURL: String

    enum CodingKeys: String, CodingKey {
        case tagName = "tag_name"
        case body
        case publishedAt = "published_at"
        case htmlURL = "html_url"
    }
}

private extension String {
    func trimmingPrefix(_ prefix: String) -> String {
        guard hasPrefix(prefix) else { return self }
        return String(dropFirst(prefix.count))
    }

    var nilIfBlank: String? {
        trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : self
    }
}
