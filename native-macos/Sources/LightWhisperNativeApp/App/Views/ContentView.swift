import SwiftUI

struct ContentView: View {
    @ObservedObject var model: AppModel

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            header
            workflowPicker
            statusLine
        }
        .padding(24)
        .frame(minWidth: 640, minHeight: 420, alignment: .topLeading)
        .background(WindowAccessor { model.bindMainWindow($0) })
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Light Whisper")
                .font(.system(size: 28, weight: .semibold))
            Text("macOS online ASR utility")
                .foregroundStyle(AppTheme.mutedText)
        }
    }

    private var workflowPicker: some View {
        Picker("Workflow", selection: $model.activeWorkflow) {
            ForEach(RecordingWorkflow.allCases) { workflow in
                Text(AppChrome.workflowPresentation(for: workflow).title).tag(workflow)
            }
        }
        .pickerStyle(.segmented)
    }

    private var statusLine: some View {
        let tone = AppChrome.activityTone(isRecording: model.isRecording, isProcessing: model.isProcessing)
        return Label(model.errorMessage ?? model.statusMessage, systemImage: symbolName(for: tone))
            .foregroundStyle(model.errorMessage == nil ? AppTheme.ink : .red)
    }

    private func symbolName(for tone: ActivityStatusTone) -> String {
        switch tone {
        case .ready:
            return "checkmark.circle"
        case .processing:
            return "clock"
        case .recording:
            return "record.circle"
        }
    }
}
