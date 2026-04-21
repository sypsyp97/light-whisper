import Foundation

struct ScreenContextImage: Equatable, Sendable {
    var label: String
    var mimeType: String
    var dataBase64: String
}

enum ScreenContextService {
    static func captureFullScreenContext(
        fileManager: FileManager = .default
    ) async throws -> [ScreenContextImage] {
        try await MainActor.run {
            try PermissionsService.ensureScreenCaptureAccess()
        }

        let tempURL = fileManager.temporaryDirectory.appendingPathComponent(
            "light-whisper-screen-\(UUID().uuidString).jpg",
            isDirectory: false
        )

        defer {
            try? fileManager.removeItem(at: tempURL)
        }

        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
        process.arguments = ["-x", "-t", "jpg", tempURL.path]
        try process.run()
        process.waitUntilExit()

        guard process.terminationStatus == 0 else {
            throw PermissionsError.screenCaptureDenied
        }

        let bytes = try Data(contentsOf: tempURL)
        guard !bytes.isEmpty else {
            return []
        }

        return [
            ScreenContextImage(
                label: "Current Screen",
                mimeType: "image/jpeg",
                dataBase64: bytes.base64EncodedString()
            ),
        ]
    }
}
