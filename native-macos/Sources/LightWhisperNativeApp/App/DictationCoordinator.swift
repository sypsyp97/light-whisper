import Foundation

@MainActor
final class DictationCoordinator {
    private weak var model: AppModel?
    private let audioCapture = AudioCaptureService()
    private let clipboard = ClipboardService()
    private let subtitlePanel = SubtitlePanelController()

    init(model: AppModel) {
        self.model = model
    }

    func start(workflow: RecordingWorkflow) {
        guard let model else {
            return
        }

        model.activeWorkflow = workflow
        model.isRecording = true
        model.statusMessage = "Recording \(AppChrome.workflowPresentation(for: workflow).title)"
        subtitlePanel.show(
            mode: overlayMode(for: workflow),
            phase: .recording,
            text: model.statusMessage,
            detail: "Listening",
            interactive: false
        )

        Task { @MainActor [weak self] in
            guard let self else { return }
            do {
                try await audioCapture.start(preferredDeviceUID: model.selectedInputDeviceUID) { [weak self] level in
                    Task { @MainActor in
                        self?.subtitlePanel.updateWaveLevel(level)
                    }
                }
            } catch {
                model.errorMessage = error.localizedDescription
                model.isRecording = false
                subtitlePanel.update(phase: .error, text: error.localizedDescription, interactive: true)
            }
        }
    }

    func stop() {
        guard let model else {
            return
        }

        guard audioCapture.isRecording else {
            model.isRecording = false
            return
        }

        do {
            let audio = try audioCapture.stop()
            model.isRecording = false
            model.isProcessing = true
            Task { @MainActor [weak self] in
                await self?.finalize(audio: audio, workflow: model.activeWorkflow)
            }
        } catch {
            model.errorMessage = error.localizedDescription
            model.isRecording = false
            model.isProcessing = false
        }
    }

    private func finalize(audio: CapturedAudio, workflow: RecordingWorkflow) async {
        guard let model else {
            return
        }

        defer { model.isProcessing = false }
        do {
            let apiKey = model.onlineASRAPIKey
            let transcript = try await OnlineASRService.transcribe(
                audioWAV: audio.wavData,
                settings: model.engineSettings,
                apiKey: apiKey,
                hotWords: model.userProfile.hotWordTexts(limit: 20)
            )
            let text = try await finalizedText(from: transcript, workflow: workflow, model: model)
            clipboard.copy(text)
            subtitlePanel.update(phase: .result, text: text, detail: "Copied", interactive: true)
        } catch {
            model.errorMessage = error.localizedDescription
            subtitlePanel.update(phase: .error, text: error.localizedDescription, interactive: true)
        }
    }

    private func finalizedText(
        from transcript: String,
        workflow: RecordingWorkflow,
        model: AppModel
    ) async throws -> String {
        if workflow == .assistant {
            let response = try await AssistantService.generate(
                request: AssistantRequest(
                    request: transcript,
                    profile: model.userProfile,
                    selectedText: try? clipboard.selectedText(),
                    includeScreenContext: model.userProfile.assistantScreenContextEnabled,
                    manualAPIKey: model.assistantAPIKey,
                    sharedFallbackAPIKey: model.aiPolishAPIKey
                )
            )
            return response.content
        }

        let translationTarget = workflow == .translation ? model.userProfile.translationTarget : nil
        let decision = AIPolishService.processingDecision(
            transcript: transcript,
            profile: model.userProfile,
            apiKey: model.aiPolishAPIKey,
            translationTargetOverride: translationTarget
        )
        guard decision.shouldPolish else {
            return transcript
        }

        let result = try await AIPolishService.polish(
            request: AIPolishRequest(
                text: transcript,
                profile: model.userProfile,
                includeScreenContext: model.userProfile.aiPolishScreenContextEnabled,
                translationBehavior: decision.translationTarget.map(AIPolishTranslationBehavior.target) ?? .inheritProfile,
                manualAPIKey: model.aiPolishAPIKey
            )
        )
        return result.polishedText
    }

    private func overlayMode(for workflow: RecordingWorkflow) -> SubtitleOverlayMode {
        switch workflow {
        case .dictation:
            return .dictation
        case .translation:
            return .translation
        case .assistant:
            return .assistant
        }
    }
}
