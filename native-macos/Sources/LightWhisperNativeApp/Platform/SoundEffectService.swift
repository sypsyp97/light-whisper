import AppKit
import Foundation

@MainActor
final class SoundEffectService {
    enum Cue: String, CaseIterable, Identifiable {
        case recordingStarted
        case recordingStopped
        case processingFinished
        case processingFailed
        case permissionRequired
        case hotkeyRejected

        var id: String { rawValue }

        fileprivate var systemSoundName: String {
            switch self {
            case .recordingStarted:
                return "Tink"
            case .recordingStopped:
                return "Pop"
            case .processingFinished:
                return "Glass"
            case .processingFailed:
                return "Basso"
            case .permissionRequired:
                return "Funk"
            case .hotkeyRejected:
                return "Morse"
            }
        }

        fileprivate var fallsBackToBeep: Bool {
            switch self {
            case .processingFailed, .permissionRequired, .hotkeyRejected:
                return true
            case .recordingStarted, .recordingStopped, .processingFinished:
                return false
            }
        }
    }

    private static let soundDirectory = URL(fileURLWithPath: "/System/Library/Sounds", isDirectory: true)
    private var cache: [Cue: NSSound] = [:]

    func preload() {
        for cue in Cue.allCases {
            _ = sound(for: cue)
        }
    }

    @discardableResult
    func play(_ cue: Cue, enabled: Bool = true) -> Bool {
        guard enabled else {
            return false
        }

        if let sound = sound(for: cue) {
            sound.stop()
            return sound.play()
        }

        if cue.fallsBackToBeep {
            NSSound.beep()
            return true
        }

        return false
    }

    func stopAll() {
        for sound in cache.values {
            sound.stop()
        }
    }

    @discardableResult
    func playTestPattern(enabled: Bool = true) -> Bool {
        let started = play(.recordingStarted, enabled: enabled)
        let stopped = play(.recordingStopped, enabled: enabled)
        return started || stopped
    }

    private func sound(for cue: Cue) -> NSSound? {
        if let cached = cache[cue] {
            return cached
        }

        let fileURL = Self.soundDirectory.appendingPathComponent("\(cue.systemSoundName).aiff")
        let loadedSound = NSSound(contentsOf: fileURL, byReference: true)
            ?? NSSound(named: NSSound.Name(cue.systemSoundName))

        if let loadedSound {
            cache[cue] = loadedSound
        }

        return loadedSound
    }
}
