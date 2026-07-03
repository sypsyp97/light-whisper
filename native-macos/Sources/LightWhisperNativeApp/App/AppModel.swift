import AppKit
import SwiftUI

@MainActor
final class AppModel: ObservableObject {
    static let mainWindowSceneID = "main"

    @Published var engineSettings: EngineSettings
    @Published var userProfile: UserProfile
    @Published var selectedInputDeviceUID: String?
    @Published var onlineASRAPIKey = ""
    @Published var aiPolishAPIKey = ""
    @Published var assistantAPIKey = ""
    @Published var webSearchAPIKey = ""
    @Published var statusMessage = "Ready"
    @Published var errorMessage: String?
    @Published var updateInfo: AppUpdateInfo?
    @Published var isRecording = false
    @Published var isProcessing = false
    @Published var activeWorkflow: RecordingWorkflow {
        didSet {
            UserDefaults.standard.set(activeWorkflow.rawValue, forKey: Self.activeWorkflowDefaultsKey)
        }
    }

    weak var mainWindowReference: NSWindow?
    var openMainWindowScene: (() -> Void)?

    private static let activeWorkflowDefaultsKey = "activeWorkflow"
    private let keychainStore = KeychainStore()
    private lazy var coordinator = DictationCoordinator(model: self)

    init() {
        engineSettings = (try? JSONFileStore<EngineSettings>(url: AppPaths.engineSettingsURL()).load(
            defaultValue: EngineSettings()
        )) ?? EngineSettings()
        userProfile = (try? ProfileManagementService.load()) ?? UserProfile.defaultValue()
        let persistedWorkflow = UserDefaults.standard.string(forKey: Self.activeWorkflowDefaultsKey)
        activeWorkflow = persistedWorkflow.flatMap(RecordingWorkflow.init(rawValue:)) ?? .dictation
    }

    func bindMainWindow(_ window: NSWindow?) {
        mainWindowReference = window
    }

    func handleStatusAction(_ action: StatusBarController.Action) {
        switch action {
        case .showMainWindow:
            showMainWindow()
        case .toggleDictation:
            toggle(workflow: .dictation)
        case .startTranslation:
            toggle(workflow: .translation)
        case .startAssistant:
            toggle(workflow: .assistant)
        case .stopActiveWorkflow:
            stopActiveWorkflow()
        case .openSettings:
            showSettingsWindow()
        case .checkForUpdates:
            if let updateInfo, updateInfo.available {
                UpdaterService.openReleasePage(updateInfo.releaseURL)
            } else {
                Task { await checkForUpdates() }
            }
        case .quitApplication:
            NSApp.terminate(nil)
        }
    }

    func selectInputDevice(uid: String?) {
        selectedInputDeviceUID = uid?.trimmingCharacters(in: .whitespacesAndNewlines).nilIfEmpty
        statusMessage = selectedInputDeviceUID == nil ? "Following system default input" : "Using selected input device"
    }

    func persistEngineSettings() {
        do {
            let store = try JSONFileStore<EngineSettings>(url: AppPaths.engineSettingsURL())
            try store.save(engineSettings)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func persistUserProfile() {
        do {
            try ProfileManagementService.save(userProfile)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func persistOnlineASRAPIKey() {
        persistAPIKey(onlineASRAPIKey, user: engineSettings.onlineASRKeychainUser())
    }

    func persistAIPolishAPIKey() {
        persistAPIKey(aiPolishAPIKey, user: LLMProviderCatalog.keychainUser(for: userProfile.llmProvider.resolveActiveProvider()))
    }

    func persistAssistantAPIKey() {
        let provider = userProfile.llmProvider.resolveAssistantProvider()
        persistAPIKey(assistantAPIKey, user: LLMProviderCatalog.keychainUser(for: provider))
    }

    func persistWebSearchAPIKey() {
        let user = "web-search-\(userProfile.webSearch.provider.rawValue)-api-key"
        persistAPIKey(webSearchAPIKey, user: user)
    }

    func flushPendingChanges() {
        persistOnlineASRAPIKey()
        persistAIPolishAPIKey()
        persistAssistantAPIKey()
        persistWebSearchAPIKey()
        persistEngineSettings()
        persistUserProfile()
    }

    func addHotWord(_ text: String, weight: UInt8 = 3) {
        ProfileManagementService.addHotWord(profile: &userProfile, text: text, weight: weight)
        persistUserProfile()
    }

    func removeHotWord(_ text: String) {
        ProfileManagementService.removeHotWord(profile: &userProfile, text: text)
        persistUserProfile()
    }

    func removeCorrection(original: String, corrected: String) {
        ProfileManagementService.removeCorrection(profile: &userProfile, original: original, corrected: corrected)
        persistUserProfile()
    }

    func saveEditedHistoryEntry(recordID: String, editedText: String) {
        do {
            let store = try HistoryStore()
            if let context = try store.applyUserEdit(recordID: recordID, editedText: editedText) {
                ProfileManagementService.submitUserCorrection(profile: &userProfile, submission: context)
                persistUserProfile()
            }
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func checkForUpdates() async {
        isProcessing = true
        defer { isProcessing = false }
        do {
            updateInfo = try await UpdaterService.checkForUpdates(currentVersion: AppVersion.current())
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func toggle(workflow: RecordingWorkflow) {
        activeWorkflow = workflow
        if isRecording {
            stopActiveWorkflow()
        } else {
            coordinator.start(workflow: workflow)
        }
    }

    func stopActiveWorkflow() {
        coordinator.stop()
    }

    private func showMainWindow() {
        if let window = mainWindowReference {
            window.identifier = NSUserInterfaceItemIdentifier(Self.mainWindowSceneID)
            window.makeKeyAndOrderFront(nil)
            NSApp.activate(ignoringOtherApps: true)
            return
        }

        openMainWindowScene?()
        NSApp.activate(ignoringOtherApps: true)
    }

    private func showSettingsWindow() {
        NSApp.sendAction(Selector(("showSettingsWindow:")), to: nil, from: nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    private func persistAPIKey(_ value: String, user: String) {
        do {
            if let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines).nilIfEmpty {
                try keychainStore.set(trimmed, for: user)
            } else {
                try? keychainStore.deleteValue(for: user)
            }
        } catch {
            errorMessage = error.localizedDescription
        }
    }
}

private extension String {
    var nilIfEmpty: String? {
        isEmpty ? nil : self
    }
}
