import Testing
@testable import LightWhisperNativeApp

@Suite("UI Design System")
struct UIDesignSystemTests {
    @Test
    func settingsSectionsStayCompleteAndOrdered() {
        #expect(
            SettingsSectionID.allCases.map(\.rawValue) == [
                "tooling",
                "speech",
                "polish",
                "assistant",
                "providers",
                "search",
                "updater",
                "validation",
                "oauth",
            ]
        )
        #expect(
            SettingsSectionID.allCases.allSatisfy {
                !$0.title.isEmpty && !$0.summary.isEmpty && !$0.symbolName.isEmpty
            }
        )
    }

    @Test
    func workflowPresentationMapsEachWorkflowToDistinctAccentAndCopy() {
        let presentations = RecordingWorkflow.allCases.map(AppChrome.workflowPresentation(for:))

        #expect(presentations.map(\.title) == ["Dictation", "Translation", "Assistant"])
        #expect(Set(presentations.map(\.accent.rawValue)).count == RecordingWorkflow.allCases.count)
        #expect(presentations.allSatisfy { !$0.eyebrow.isEmpty && !$0.summary.isEmpty && !$0.symbolName.isEmpty })
    }

    @Test(arguments: [
        (false, false, ActivityStatusTone.ready),
        (false, true, ActivityStatusTone.processing),
        (true, false, ActivityStatusTone.recording),
        (true, true, ActivityStatusTone.recording),
    ])
    func activityTonePrioritizesRecordingBeforeProcessing(
        isRecording: Bool,
        isProcessing: Bool,
        expected: ActivityStatusTone
    ) {
        #expect(AppChrome.activityTone(isRecording: isRecording, isProcessing: isProcessing) == expected)
    }
}
