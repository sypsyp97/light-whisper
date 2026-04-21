import AppKit
import Foundation

public enum PlatformPermission: String, CaseIterable, Sendable {
    case microphone
    case accessibility
    case automation
    case screenCapture
}

public enum PlatformPermissionStatus: String, Sendable {
    case granted
    case denied
    case notDetermined
}

public enum LightWhisperPlatformError: LocalizedError, Sendable {
    case unsupported(String)
    case permissionDenied(permission: PlatformPermission, message: String)
    case invalidConfiguration(String)
    case operationFailed(String)

    public var errorDescription: String? {
        switch self {
        case .unsupported(let message),
             .invalidConfiguration(let message),
             .operationFailed(let message):
            return message
        case .permissionDenied(let permission, let message):
            return "\(permission.rawValue): \(message)"
        }
    }
}

extension NSScreen {
    fileprivate static var lightWhisperBestOverlayScreen: NSScreen? {
        NSScreen.main ?? NSScreen.screens.first
    }
}
