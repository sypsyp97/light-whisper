import ApplicationServices
import AppKit
import AVFoundation
import CoreGraphics
import Foundation

enum PermissionsError: LocalizedError {
    case microphoneDenied
    case accessibilityDenied
    case automationDenied
    case screenCaptureDenied

    var errorDescription: String? {
        switch self {
        case .microphoneDenied:
            return "Microphone access is required."
        case .accessibilityDenied:
            return "Accessibility access is required."
        case .automationDenied:
            return "Automation permission for System Events is required."
        case .screenCaptureDenied:
            return "Screen recording permission is required."
        }
    }
}

@MainActor
enum PermissionsService {
    static func requestMicrophoneAccess() async -> Bool {
        await withCheckedContinuation { continuation in
            AVCaptureDevice.requestAccess(for: .audio) { granted in
                continuation.resume(returning: granted)
            }
        }
    }

    static func ensureMicrophoneAccess() async throws {
        guard await requestMicrophoneAccess() else {
            throw PermissionsError.microphoneDenied
        }
    }

    static func accessibilityTrusted(prompt: Bool) -> Bool {
        let trusted: Bool
        if prompt {
            // Mirror kAXTrustedCheckOptionPrompt without reading the shared CFString global in Swift 6.
            let promptKey = "AXTrustedCheckOptionPrompt" as CFString
            let options = [
                promptKey as String: true,
            ] as CFDictionary
            trusted = AXIsProcessTrustedWithOptions(options)
        } else {
            trusted = AXIsProcessTrusted()
        }
        if !trusted, prompt {
            NSWorkspace.shared.open(
                URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")!
            )
        }
        return trusted
    }

    static func ensureAccessibilityAccess(prompt: Bool = true) throws {
        guard accessibilityTrusted(prompt: prompt) else {
            throw PermissionsError.accessibilityDenied
        }
    }

    static func ensureAutomationAccess() async throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/osascript")
        process.arguments = ["-e", "tell application \"System Events\" to get name of first application process"]
        do {
            try process.run()
            process.waitUntilExit()
        } catch {
            throw PermissionsError.automationDenied
        }

        guard process.terminationStatus == 0 else {
            throw PermissionsError.automationDenied
        }
    }

    static func hasScreenCaptureAccess() -> Bool {
        CGPreflightScreenCaptureAccess()
    }

    static func ensureScreenCaptureAccess() throws {
        if hasScreenCaptureAccess() {
            return
        }
        guard CGRequestScreenCaptureAccess() else {
            throw PermissionsError.screenCaptureDenied
        }
    }
}
