import SwiftUI

enum RecordingWorkflow: String, CaseIterable, Identifiable, Sendable {
    case dictation
    case translation
    case assistant

    var id: String { rawValue }

    var resultWorkflow: ResultRecordWorkflow {
        switch self {
        case .dictation:
            return .dictation
        case .translation:
            return .translation
        case .assistant:
            return .assistant
        }
    }
}

enum ActivityStatusTone: Equatable, Sendable {
    case ready
    case processing
    case recording
}

enum SettingsSectionID: String, CaseIterable, Identifiable, Sendable {
    case tooling
    case speech
    case polish
    case assistant
    case providers
    case search
    case updater
    case validation
    case oauth

    var id: String { rawValue }

    var title: String {
        switch self {
        case .tooling:
            return "Tooling"
        case .speech:
            return "Speech"
        case .polish:
            return "AI Polish"
        case .assistant:
            return "Assistant"
        case .providers:
            return "Providers"
        case .search:
            return "Search"
        case .updater:
            return "Updater"
        case .validation:
            return "Validation"
        case .oauth:
            return "OpenAI OAuth"
        }
    }

    var summary: String {
        switch self {
        case .tooling:
            return "Workflow defaults and global shortcuts."
        case .speech:
            return "Online ASR engine, region, and microphone input."
        case .polish:
            return "Post-processing, translation, and correction learning."
        case .assistant:
            return "Assistant workflow behavior and context."
        case .providers:
            return "LLM provider endpoints, models, and credentials."
        case .search:
            return "Web search source and result limits."
        case .updater:
            return "Release checks and install status."
        case .validation:
            return "Correction validation and cleanup."
        case .oauth:
            return "OpenAI account login state."
        }
    }

    var symbolName: String {
        switch self {
        case .tooling:
            return "keyboard"
        case .speech:
            return "waveform"
        case .polish:
            return "wand.and.stars"
        case .assistant:
            return "sparkles"
        case .providers:
            return "network"
        case .search:
            return "magnifyingglass"
        case .updater:
            return "arrow.down.circle"
        case .validation:
            return "checkmark.seal"
        case .oauth:
            return "person.badge.key"
        }
    }
}

enum AppChrome {
    struct WorkflowPresentation: Equatable, Sendable {
        var title: String
        var eyebrow: String
        var summary: String
        var symbolName: String
        var accent: InterfaceAccentRole
    }

    static func workflowPresentation(for workflow: RecordingWorkflow) -> WorkflowPresentation {
        switch workflow {
        case .dictation:
            return WorkflowPresentation(
                title: "Dictation",
                eyebrow: "Speech to Text",
                summary: "Capture spoken text and paste it into the focused app.",
                symbolName: "mic",
                accent: .rust
            )
        case .translation:
            return WorkflowPresentation(
                title: "Translation",
                eyebrow: "Translate",
                summary: "Transcribe speech and polish it toward the configured target language.",
                symbolName: "globe",
                accent: .amber
            )
        case .assistant:
            return WorkflowPresentation(
                title: "Assistant",
                eyebrow: "Context",
                summary: "Use selected text, app context, and optional screen context to answer requests.",
                symbolName: "sparkles",
                accent: .moss
            )
        }
    }

    static func activityTone(isRecording: Bool, isProcessing: Bool) -> ActivityStatusTone {
        if isRecording {
            return .recording
        }
        if isProcessing {
            return .processing
        }
        return .ready
    }
}
