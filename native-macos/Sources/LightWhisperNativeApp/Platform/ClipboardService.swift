import AppKit
import ApplicationServices
import Foundation

enum ClipboardError: LocalizedError {
    case selectionUnavailable
    case pasteFailed

    var errorDescription: String? {
        switch self {
        case .selectionUnavailable:
            return "Unable to read selected text from the focused element."
        case .pasteFailed:
            return "Unable to paste text into the frontmost app."
        }
    }
}

@MainActor
struct ClipboardService {
    func copy(_ text: String) {
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(text, forType: .string)
    }

    func paste(_ text: String) async throws {
        try PermissionsService.ensureAccessibilityAccess()
        try await PermissionsService.ensureAutomationAccess()
        copy(text)

        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/osascript")
        process.arguments = [
            "-e",
            "tell application \"System Events\" to keystroke \"v\" using command down",
        ]
        try process.run()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else {
            throw ClipboardError.pasteFailed
        }
    }

    func selectedText() throws -> String? {
        try PermissionsService.ensureAccessibilityAccess()

        let systemWide = AXUIElementCreateSystemWide()
        var focusedObject: CFTypeRef?
        let focusedResult = AXUIElementCopyAttributeValue(
            systemWide,
            kAXFocusedUIElementAttribute as CFString,
            &focusedObject
        )
        guard focusedResult == .success, let focusedObject, CFGetTypeID(focusedObject) == AXUIElementGetTypeID() else {
            return nil
        }

        let focusedElement = unsafeDowncast(focusedObject as AnyObject, to: AXUIElement.self)
        var selectedObject: CFTypeRef?
        let selectedResult = AXUIElementCopyAttributeValue(
            focusedElement,
            kAXSelectedTextAttribute as CFString,
            &selectedObject
        )
        if selectedResult == .success, let selectedText = selectedObject as? String {
            let trimmed = selectedText.trimmingCharacters(in: .whitespacesAndNewlines)
            return trimmed.isEmpty ? nil : trimmed
        }

        var selectedRangeObject: CFTypeRef?
        let rangeResult = AXUIElementCopyAttributeValue(
            focusedElement,
            kAXSelectedTextRangeAttribute as CFString,
            &selectedRangeObject
        )
        guard rangeResult == .success,
              let selectedRangeObject,
              CFGetTypeID(selectedRangeObject) == AXValueGetTypeID()
        else {
            return nil
        }

        var selectedRange = CFRange(location: 0, length: 0)
        let didResolveRange = AXValueGetValue(
            selectedRangeObject as! AXValue,
            .cfRange,
            &selectedRange
        )
        guard didResolveRange, selectedRange.length > 0 else {
            return nil
        }

        guard let rangeValue = AXValueCreate(.cfRange, &selectedRange) else {
            return nil
        }

        var rangedTextObject: CFTypeRef?
        let rangedTextStatus = AXUIElementCopyParameterizedAttributeValue(
            focusedElement,
            kAXStringForRangeParameterizedAttribute as CFString,
            rangeValue,
            &rangedTextObject
        )
        guard rangedTextStatus == .success,
              let rangedText = rangedTextObject as? String
        else {
            return nil
        }

        let trimmed = rangedText.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}
