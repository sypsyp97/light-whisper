import AppKit
import Carbon
import Foundation

enum GlobalHotkeyError: LocalizedError {
    case invalidShortcut

    var errorDescription: String? {
        switch self {
        case .invalidShortcut:
            return "The shortcut is not supported yet."
        }
    }
}

@MainActor
final class GlobalHotkeyService {
    enum Kind: String, CaseIterable, Identifiable {
        case dictation
        case translation
        case assistant

        var id: String { rawValue }
    }

    enum RegistrationMode: String, Equatable, Sendable {
        case keyDownOnly
        case keyDownAndUp
        case holdToTalk

        fileprivate var tracksKeyRelease: Bool {
            switch self {
            case .keyDownOnly:
                return false
            case .keyDownAndUp, .holdToTalk:
                return true
            }
        }

        fileprivate var suppressesRepeatedKeyDown: Bool {
            switch self {
            case .keyDownOnly:
                return false
            case .keyDownAndUp, .holdToTalk:
                return true
            }
        }
    }

    enum TriggerPhase: String, Equatable, Sendable {
        case keyDown
        case keyUp
    }

    struct RegisteredShortcut: Identifiable, Equatable, Sendable {
        let kind: Kind
        let shortcut: String
        let mode: RegistrationMode
        let hasKeyUpHandler: Bool
        let isPressed: Bool

        var id: Kind { kind }
    }

    struct Diagnostics: Equatable, Sendable {
        let registeredShortcuts: [RegisteredShortcut]
        let activePresses: [Kind]
        let monitorsInstalled: Bool
    }

    private struct Registration {
        let kind: Kind
        let descriptor: HotkeyDescriptor
        let shortcut: String
        let mode: RegistrationMode
        let onKeyDown: (() -> Void)?
        let onKeyUp: (() -> Void)?
    }

    private var registered: [Kind: Registration] = [:]
    private var activePresses: Set<Kind> = []
    private var globalKeyDownMonitor: Any?
    private var globalKeyUpMonitor: Any?
    private var localKeyDownMonitor: Any?
    private var localKeyUpMonitor: Any?

    var registeredShortcuts: [RegisteredShortcut] {
        registered.values
            .map { registration in
                RegisteredShortcut(
                    kind: registration.kind,
                    shortcut: registration.shortcut,
                    mode: registration.mode,
                    hasKeyUpHandler: registration.onKeyUp != nil,
                    isPressed: activePresses.contains(registration.kind)
                )
            }
            .sorted { $0.kind.rawValue < $1.kind.rawValue }
    }

    var diagnostics: Diagnostics {
        Diagnostics(
            registeredShortcuts: registeredShortcuts,
            activePresses: activePresses.sorted { $0.rawValue < $1.rawValue },
            monitorsInstalled: monitorsInstalled
        )
    }

    func register(kind: Kind, shortcut: String, callback: @escaping () -> Void) throws {
        try register(
            kind: kind,
            shortcut: shortcut,
            mode: .keyDownOnly,
            onKeyDown: callback,
            onKeyUp: nil
        )
    }

    func register(
        kind: Kind,
        shortcut: String,
        mode: RegistrationMode,
        onKeyDown: (() -> Void)?,
        onKeyUp: (() -> Void)?
    ) throws {
        let descriptor = try HotkeyDescriptor(shortcut: shortcut)
        registered[kind] = Registration(
            kind: kind,
            descriptor: descriptor,
            shortcut: descriptor.canonicalShortcut,
            mode: mode,
            onKeyDown: onKeyDown,
            onKeyUp: onKeyUp
        )
        activePresses.remove(kind)
        installEventMonitorsIfNeeded()
    }

    func registerHoldToTalk(
        kind: Kind,
        shortcut: String,
        onPress: @escaping () -> Void,
        onRelease: @escaping () -> Void
    ) throws {
        try register(
            kind: kind,
            shortcut: shortcut,
            mode: .holdToTalk,
            onKeyDown: onPress,
            onKeyUp: onRelease
        )
    }

    func unregister(kind: Kind) {
        registered.removeValue(forKey: kind)
        activePresses.remove(kind)
    }

    func unregisterAll() {
        registered.removeAll()
        activePresses.removeAll()
    }

    func trigger(kind: Kind) {
        trigger(kind: kind, phase: .keyDown)
    }

    func trigger(kind: Kind, phase: TriggerPhase) {
        guard let registration = registered[kind] else {
            return
        }

        switch phase {
        case .keyDown:
            fireKeyDown(for: registration, isARepeat: false)
        case .keyUp:
            fireKeyUp(for: registration)
        }
    }

    func registration(for kind: Kind) -> RegisteredShortcut? {
        registeredShortcuts.first { $0.kind == kind }
    }

    private func installEventMonitorsIfNeeded() {
        guard !monitorsInstalled else {
            return
        }

        globalKeyDownMonitor = NSEvent.addGlobalMonitorForEvents(matching: .keyDown) { [weak self] event in
            Task { @MainActor in
                self?.handle(event: event, phase: .keyDown)
            }
        }

        globalKeyUpMonitor = NSEvent.addGlobalMonitorForEvents(matching: .keyUp) { [weak self] event in
            Task { @MainActor in
                self?.handle(event: event, phase: .keyUp)
            }
        }

        localKeyDownMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            self?.handle(event: event, phase: .keyDown)
            return event
        }

        localKeyUpMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyUp) { [weak self] event in
            self?.handle(event: event, phase: .keyUp)
            return event
        }
    }

    private var monitorsInstalled: Bool {
        globalKeyDownMonitor != nil &&
            globalKeyUpMonitor != nil &&
            localKeyDownMonitor != nil &&
            localKeyUpMonitor != nil
    }

    private func handle(event: NSEvent, phase: TriggerPhase) {
        for registration in registered.values where registration.descriptor.matches(event, phase: phase) {
            switch phase {
            case .keyDown:
                fireKeyDown(for: registration, isARepeat: event.isARepeat)
            case .keyUp:
                fireKeyUp(for: registration)
            }
            break
        }
    }

    private func fireKeyDown(for registration: Registration, isARepeat: Bool) {
        if registration.mode.suppressesRepeatedKeyDown {
            guard !isARepeat, !activePresses.contains(registration.kind) else {
                return
            }
            activePresses.insert(registration.kind)
        }

        registration.onKeyDown?()
    }

    private func fireKeyUp(for registration: Registration) {
        guard registration.mode.tracksKeyRelease else {
            return
        }

        guard activePresses.remove(registration.kind) != nil else {
            return
        }

        registration.onKeyUp?()
    }
}

private struct HotkeyDescriptor {
    let keyCode: UInt16
    let modifiers: UInt32
    let eventModifiers: NSEvent.ModifierFlags
    let canonicalShortcut: String
    private let keyToken: String

    init(shortcut: String) throws {
        let tokens = shortcut
            .split(separator: "+")
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() }
        guard let keyToken = tokens.last, !keyToken.isEmpty else {
            throw GlobalHotkeyError.invalidShortcut
        }

        var modifiers: UInt32 = 0
        for token in tokens.dropLast() {
            switch token {
            case "cmd", "command":
                modifiers |= UInt32(cmdKey)
            case "option", "alt":
                modifiers |= UInt32(optionKey)
            case "ctrl", "control":
                modifiers |= UInt32(controlKey)
            case "shift":
                modifiers |= UInt32(shiftKey)
            default:
                break
            }
        }

        guard let keyCode = Self.keyCode(for: keyToken) else {
            throw GlobalHotkeyError.invalidShortcut
        }
        self.keyCode = keyCode
        self.modifiers = modifiers
        self.eventModifiers = Self.eventModifiers(for: modifiers)
        self.keyToken = keyToken
        self.canonicalShortcut = Self.canonicalShortcut(for: modifiers, keyToken: keyToken)
    }

    private static func keyCode(for token: String) -> UInt16? {
        switch token {
        case "f1": return UInt16(kVK_F1)
        case "f2": return UInt16(kVK_F2)
        case "f3": return UInt16(kVK_F3)
        case "f4": return UInt16(kVK_F4)
        case "f5": return UInt16(kVK_F5)
        case "f6": return UInt16(kVK_F6)
        case "f7": return UInt16(kVK_F7)
        case "f8": return UInt16(kVK_F8)
        case "f9": return UInt16(kVK_F9)
        case "f10": return UInt16(kVK_F10)
        case "f11": return UInt16(kVK_F11)
        case "f12": return UInt16(kVK_F12)
        case "a": return UInt16(kVK_ANSI_A)
        case "b": return UInt16(kVK_ANSI_B)
        case "c": return UInt16(kVK_ANSI_C)
        case "d": return UInt16(kVK_ANSI_D)
        case "e": return UInt16(kVK_ANSI_E)
        case "f": return UInt16(kVK_ANSI_F)
        case "g": return UInt16(kVK_ANSI_G)
        case "h": return UInt16(kVK_ANSI_H)
        case "i": return UInt16(kVK_ANSI_I)
        case "j": return UInt16(kVK_ANSI_J)
        case "k": return UInt16(kVK_ANSI_K)
        case "l": return UInt16(kVK_ANSI_L)
        case "m": return UInt16(kVK_ANSI_M)
        case "n": return UInt16(kVK_ANSI_N)
        case "o": return UInt16(kVK_ANSI_O)
        case "p": return UInt16(kVK_ANSI_P)
        case "q": return UInt16(kVK_ANSI_Q)
        case "r": return UInt16(kVK_ANSI_R)
        case "s": return UInt16(kVK_ANSI_S)
        case "t": return UInt16(kVK_ANSI_T)
        case "u": return UInt16(kVK_ANSI_U)
        case "v": return UInt16(kVK_ANSI_V)
        case "w": return UInt16(kVK_ANSI_W)
        case "x": return UInt16(kVK_ANSI_X)
        case "y": return UInt16(kVK_ANSI_Y)
        case "z": return UInt16(kVK_ANSI_Z)
        case "0": return UInt16(kVK_ANSI_0)
        case "1": return UInt16(kVK_ANSI_1)
        case "2": return UInt16(kVK_ANSI_2)
        case "3": return UInt16(kVK_ANSI_3)
        case "4": return UInt16(kVK_ANSI_4)
        case "5": return UInt16(kVK_ANSI_5)
        case "6": return UInt16(kVK_ANSI_6)
        case "7": return UInt16(kVK_ANSI_7)
        case "8": return UInt16(kVK_ANSI_8)
        case "9": return UInt16(kVK_ANSI_9)
        case "space": return UInt16(kVK_Space)
        default: return nil
        }
    }

    private static func eventModifiers(for carbonModifiers: UInt32) -> NSEvent.ModifierFlags {
        var flags: NSEvent.ModifierFlags = []
        if carbonModifiers & UInt32(cmdKey) != 0 {
            flags.insert(.command)
        }
        if carbonModifiers & UInt32(optionKey) != 0 {
            flags.insert(.option)
        }
        if carbonModifiers & UInt32(controlKey) != 0 {
            flags.insert(.control)
        }
        if carbonModifiers & UInt32(shiftKey) != 0 {
            flags.insert(.shift)
        }
        return flags
    }

    private static func canonicalShortcut(for carbonModifiers: UInt32, keyToken: String) -> String {
        var tokens: [String] = []
        if carbonModifiers & UInt32(controlKey) != 0 {
            tokens.append("ctrl")
        }
        if carbonModifiers & UInt32(optionKey) != 0 {
            tokens.append("alt")
        }
        if carbonModifiers & UInt32(shiftKey) != 0 {
            tokens.append("shift")
        }
        if carbonModifiers & UInt32(cmdKey) != 0 {
            tokens.append("cmd")
        }
        tokens.append(keyToken)
        return tokens.joined(separator: "+")
    }

    func matches(_ event: NSEvent, phase: GlobalHotkeyService.TriggerPhase) -> Bool {
        let eventFlags = event.modifierFlags.intersection([.command, .option, .control, .shift])
        let expectedType: NSEvent.EventType = phase == .keyDown ? .keyDown : .keyUp
        return event.type == expectedType && event.keyCode == keyCode && eventFlags == eventModifiers
    }
}
