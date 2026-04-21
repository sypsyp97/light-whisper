import AppKit
import Foundation

@MainActor
final class StatusBarController: NSObject {
    enum Action: String, CaseIterable, Identifiable {
        case showMainWindow
        case toggleDictation
        case startTranslation
        case startAssistant
        case stopActiveWorkflow
        case openSettings
        case checkForUpdates
        case quitApplication

        var id: String { rawValue }
    }

    struct Snapshot: Equatable {
        var isRecording = false
        var isProcessing = false
        var activeWorkflowTitle: String?
        var statusMessage = "Ready"
        var errorMessage: String?
        var microphoneLevel: Float = 0
        var updateAvailable = false
    }

    private enum ItemKey: Hashable {
        case status
        case level
        case showMainWindow
        case toggleDictation
        case startTranslation
        case startAssistant
        case stopActiveWorkflow
        case openSettings
        case checkForUpdates
        case quitApplication
    }

    private let statusBar: NSStatusBar
    private var statusItem: NSStatusItem?
    private var menu: NSMenu?
    private var actionHandler: ((Action) -> Void)?
    private var items: [ItemKey: NSMenuItem] = [:]
    private var snapshot = Snapshot()

    init(statusBar: NSStatusBar = .system) {
        self.statusBar = statusBar
    }

    func install(actionHandler: @escaping (Action) -> Void) {
        self.actionHandler = actionHandler
        if statusItem == nil {
            buildStatusItem()
        }
        update(snapshot: snapshot)
    }

    func update(snapshot: Snapshot) {
        self.snapshot = snapshot
        guard statusItem != nil else {
            return
        }

        let titlePrefix: String
        if snapshot.isRecording {
            titlePrefix = "REC"
        } else if snapshot.isProcessing {
            titlePrefix = "..."
        } else if snapshot.updateAvailable {
            titlePrefix = "UPD"
        } else {
            titlePrefix = "LW"
        }

        statusItem?.button?.title = titlePrefix
        statusItem?.button?.toolTip = snapshot.errorMessage ?? snapshot.statusMessage

        items[.status]?.title = statusLine(for: snapshot)
        items[.level]?.title = "Mic Level: \(Int(max(0, min(1, snapshot.microphoneLevel)) * 100))%"
        items[.toggleDictation]?.title = snapshot.isRecording ? "Stop Dictation" : "Start Dictation"
        items[.toggleDictation]?.state = snapshot.isRecording && snapshot.activeWorkflowTitle == "Dictation" ? .on : .off
        items[.startTranslation]?.state = snapshot.isRecording && snapshot.activeWorkflowTitle == "Translation" ? .on : .off
        items[.startAssistant]?.state = snapshot.isRecording && snapshot.activeWorkflowTitle == "Assistant" ? .on : .off
        items[.stopActiveWorkflow]?.isEnabled = snapshot.isRecording || snapshot.isProcessing
        items[.checkForUpdates]?.title = snapshot.updateAvailable ? "Install Available Update" : "Check for Updates"
    }

    func remove() {
        if let statusItem {
            statusBar.removeStatusItem(statusItem)
        }
        statusItem = nil
        menu = nil
        items.removeAll()
        actionHandler = nil
    }

    private func buildStatusItem() {
        let item = statusBar.statusItem(withLength: NSStatusItem.variableLength)
        item.isVisible = true
        item.button?.title = "LW"
        item.button?.toolTip = "Light Whisper"
        item.button?.setAccessibilityLabel("Light Whisper")

        let menu = NSMenu()
        menu.autoenablesItems = false

        let statusItemView = NSMenuItem(title: snapshot.statusMessage, action: nil, keyEquivalent: "")
        statusItemView.isEnabled = false
        let levelItem = NSMenuItem(title: "Mic Level: 0%", action: nil, keyEquivalent: "")
        levelItem.isEnabled = false

        items[.status] = statusItemView
        items[.level] = levelItem
        menu.addItem(statusItemView)
        menu.addItem(levelItem)
        menu.addItem(.separator())

        let showMainWindow = makeItem(
            key: .showMainWindow,
            title: "Show Main Window",
            action: #selector(handleShowMainWindow)
        )
        let toggleDictation = makeItem(
            key: .toggleDictation,
            title: "Start Dictation",
            action: #selector(handleToggleDictation)
        )
        let startTranslation = makeItem(
            key: .startTranslation,
            title: "Start Translation",
            action: #selector(handleStartTranslation)
        )
        let startAssistant = makeItem(
            key: .startAssistant,
            title: "Start Assistant",
            action: #selector(handleStartAssistant)
        )
        let stopActiveWorkflow = makeItem(
            key: .stopActiveWorkflow,
            title: "Stop Active Workflow",
            action: #selector(handleStopActiveWorkflow)
        )

        menu.addItem(showMainWindow)
        menu.addItem(toggleDictation)
        menu.addItem(startTranslation)
        menu.addItem(startAssistant)
        menu.addItem(stopActiveWorkflow)
        menu.addItem(.separator())

        let openSettings = makeItem(
            key: .openSettings,
            title: "Open Settings",
            action: #selector(handleOpenSettings)
        )
        let checkForUpdates = makeItem(
            key: .checkForUpdates,
            title: "Check for Updates",
            action: #selector(handleCheckForUpdates)
        )
        let quitApplication = makeItem(
            key: .quitApplication,
            title: "Quit Light Whisper",
            action: #selector(handleQuitApplication)
        )

        menu.addItem(openSettings)
        menu.addItem(checkForUpdates)
        menu.addItem(.separator())
        menu.addItem(quitApplication)

        item.menu = menu
        self.menu = menu
        self.statusItem = item
    }

    private func makeItem(key: ItemKey, title: String, action: Selector) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: "")
        item.target = self
        items[key] = item
        return item
    }

    private func statusLine(for snapshot: Snapshot) -> String {
        if let errorMessage = snapshot.errorMessage?.trimmingCharacters(in: .whitespacesAndNewlines), !errorMessage.isEmpty {
            return "Error: \(errorMessage)"
        }
        if snapshot.isRecording {
            return "Recording \(snapshot.activeWorkflowTitle ?? "workflow")"
        }
        if snapshot.isProcessing {
            return "Processing \(snapshot.activeWorkflowTitle ?? "workflow")"
        }
        return snapshot.statusMessage
    }

    private func send(_ action: Action) {
        actionHandler?(action)
    }

    @objc
    private func handleShowMainWindow() {
        send(.showMainWindow)
    }

    @objc
    private func handleToggleDictation() {
        send(.toggleDictation)
    }

    @objc
    private func handleStartTranslation() {
        send(.startTranslation)
    }

    @objc
    private func handleStartAssistant() {
        send(.startAssistant)
    }

    @objc
    private func handleStopActiveWorkflow() {
        send(.stopActiveWorkflow)
    }

    @objc
    private func handleOpenSettings() {
        send(.openSettings)
    }

    @objc
    private func handleCheckForUpdates() {
        send(.checkForUpdates)
    }

    @objc
    private func handleQuitApplication() {
        send(.quitApplication)
    }
}
