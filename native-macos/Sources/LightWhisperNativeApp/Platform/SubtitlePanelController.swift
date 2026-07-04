import AppKit
import ApplicationServices
import SwiftUI

enum SubtitleOverlayMode: String, Sendable {
    case dictation
    case translation
    case assistant
}

enum SubtitleOverlayPhase: String, Sendable {
    case idle
    case recording
    case processing
    case searching
    case polishing
    case result
    case error
}

@MainActor
final class SubtitlePanelController {
    private final class OverlayState: ObservableObject {
        @Published var text = ""
        @Published var detail = ""
        @Published var mode: SubtitleOverlayMode = .dictation
        @Published var phase: SubtitleOverlayPhase = .idle
        @Published var waveLevel: Float = 0
        @Published var isInteractive = false
        @Published var copyFeedback = false

        var accentRole: InterfaceAccentRole {
            switch mode {
            case .dictation:
                return .rust
            case .translation:
                return .amber
            case .assistant:
                return .moss
            }
        }

        var accentColor: Color {
            AppTheme.accent(accentRole)
        }

        var phaseLabel: String {
            switch phase {
            case .idle:
                return "Idle"
            case .recording:
                return "Recording"
            case .processing:
                return "Processing"
            case .searching:
                return "Searching"
            case .polishing:
                return "Polishing"
            case .result:
                return "Result"
            case .error:
                return "Error"
            }
        }
    }

    private struct OverlayView: View {
        @ObservedObject var state: OverlayState
        let onCopy: () -> Void
        let onClose: () -> Void

        var body: some View {
            content
                .padding(.horizontal, 24)
                .padding(.vertical, 18)
                .frame(maxWidth: .infinity)
                .background(panelBackground)
                .padding(.horizontal, 32)
                .padding(.top, 20)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                .background(Color.clear)
        }

        private var content: some View {
            VStack(spacing: 12) {
                header
                detail
                transcript
                waveform
            }
        }

        private var header: some View {
            HStack(spacing: 10) {
                Circle()
                    .fill(state.accentColor)
                    .frame(width: 10, height: 10)
                Text(state.phaseLabel)
                    .font(.system(size: 12, weight: .semibold, design: .monospaced))
                    .foregroundStyle(.white.opacity(0.85))
                Spacer()
                if state.isInteractive {
                    actionButtons
                }
            }
        }

        private var actionButtons: some View {
            HStack(spacing: 8) {
                Button(state.copyFeedback ? "Copied" : "Copy") {
                    onCopy()
                }
                .buttonStyle(.plain)
                .padding(.horizontal, 10)
                .padding(.vertical, 5)
                .background(state.accentColor.opacity(0.22), in: Capsule())
                .foregroundStyle(.white)

                Button("Close") {
                    onClose()
                }
                .buttonStyle(.plain)
                .padding(.horizontal, 10)
                .padding(.vertical, 5)
                .background(AppTheme.canvas.opacity(0.12), in: Capsule())
                .foregroundStyle(.white)
            }
        }

        @ViewBuilder
        private var detail: some View {
            if !state.detail.isEmpty {
                Text(state.detail)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(.white.opacity(0.72))
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }

        private var transcript: some View {
            Text(state.text)
                .font(.system(size: 28, weight: .semibold, design: .rounded))
                .multilineTextAlignment(.leading)
                .foregroundStyle(.white)
                .frame(maxWidth: .infinity, alignment: .leading)
        }

        private var waveform: some View {
            HStack(alignment: .bottom, spacing: 4) {
                ForEach(0..<18, id: \.self) { index in
                    RoundedRectangle(cornerRadius: 3, style: .continuous)
                        .fill(state.accentColor.opacity(index < activeBarCount ? 1 : 0.18))
                        .frame(width: 8, height: barHeight(for: index))
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }

        private var panelBackground: some View {
            RoundedRectangle(cornerRadius: 26, style: .continuous)
                .fill(AppTheme.ink.opacity(0.92))
                .overlay(
                    RoundedRectangle(cornerRadius: 26, style: .continuous)
                        .stroke(Color.white.opacity(0.06), lineWidth: 1)
                        .padding(1)
                )
                .overlay(
                    RoundedRectangle(cornerRadius: 26, style: .continuous)
                        .stroke(state.accentColor.opacity(0.28), lineWidth: 1)
                )
        }

        private var activeBarCount: Int {
            max(1, min(18, Int((state.waveLevel * 18).rounded(.up))))
        }

        private func barHeight(for index: Int) -> CGFloat {
            let base = CGFloat((index % 6) + 1) * 5
            let dynamic = CGFloat(state.waveLevel) * 26
            return base + dynamic
        }
    }

    private let state = OverlayState()
    private var panel: NSPanel?
    private var hideTask: Task<Void, Never>?
    private var animationTask: Task<Void, Never>?

    func show(
        mode: SubtitleOverlayMode = .dictation,
        phase: SubtitleOverlayPhase = .idle,
        text: String,
        detail: String = "",
        interactive: Bool = false
    ) {
        cancelTransientTasks()
        updateState(mode: mode, phase: phase, text: text, detail: detail, interactive: interactive)
        let panel = panel ?? makePanel()
        panel.ignoresMouseEvents = !interactive
        reinforcePanelWindowing(panel)
        layout(panel)
        panel.orderFrontRegardless()
    }

    func update(
        mode: SubtitleOverlayMode? = nil,
        phase: SubtitleOverlayPhase? = nil,
        text: String? = nil,
        detail: String? = nil,
        interactive: Bool? = nil
    ) {
        state.mode = mode ?? state.mode
        state.phase = phase ?? state.phase
        if let text {
            state.text = text
        }
        if let detail {
            state.detail = detail
        }
        if let interactive {
            state.isInteractive = interactive
        }
        panel?.ignoresMouseEvents = !(interactive ?? state.isInteractive)
        guard let panel else {
            show(
                mode: state.mode,
                phase: state.phase,
                text: state.text,
                detail: state.detail,
                interactive: state.isInteractive
            )
            return
        }
        reinforcePanelWindowing(panel)
        layout(panel)
    }

    func updateWaveLevel(_ value: Float) {
        state.waveLevel = max(0, min(1, value))
    }

    func animateAssistantResult(
        text: String,
        detail: String,
        stepDelayNanoseconds: UInt64 = 18_000_000
    ) {
        animationTask?.cancel()
        state.copyFeedback = false
        let characters = Array(text)
        show(mode: .assistant, phase: .processing, text: "", detail: detail, interactive: false)

        animationTask = Task { @MainActor [weak self] in
            guard let self else { return }
            var rendered = ""
            for character in characters {
                if Task.isCancelled { return }
                rendered.append(character)
                update(phase: .processing, text: rendered, detail: detail, interactive: false)
                try? await Task.sleep(nanoseconds: stepDelayNanoseconds)
            }
            update(phase: .result, text: text, detail: detail, interactive: true)
        }
    }

    func scheduleAutoHide(after seconds: TimeInterval) {
        hideTask?.cancel()
        hideTask = Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: UInt64(seconds * 1_000_000_000))
            guard let self, !Task.isCancelled else { return }
            if !state.isInteractive {
                hide()
            }
        }
    }

    func hide() {
        cancelTransientTasks()
        state.waveLevel = 0
        state.isInteractive = false
        panel?.ignoresMouseEvents = true
        panel?.orderOut(nil)
    }

    private func makePanel() -> NSPanel {
        let panel = NSPanel(
            contentRect: .zero,
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        panel.backgroundColor = .clear
        panel.isOpaque = false
        panel.hasShadow = false
        panel.isMovableByWindowBackground = false
        panel.ignoresMouseEvents = true
        reinforcePanelWindowing(panel)
        panel.contentViewController = NSHostingController(
            rootView: OverlayView(
                state: state,
                onCopy: { [weak self] in self?.copyCurrentText() },
                onClose: { [weak self] in self?.hide() }
            )
        )
        self.panel = panel
        return panel
    }

    private func reinforcePanelWindowing(_ panel: NSPanel) {
        panel.level = .screenSaver
        panel.hidesOnDeactivate = false
        panel.canHide = false
        panel.collectionBehavior = [
            .canJoinAllSpaces,
            .fullScreenAuxiliary,
            .stationary,
            .transient,
            .ignoresCycle
        ]
    }

    private func layout(_ panel: NSPanel) {
        let screen = frontmostScreen() ?? panel.screen ?? fallbackScreen()
        let frame = screen?.frame ?? CGRect(x: 0, y: 0, width: 1280, height: 720)
        panel.setFrame(frame, display: true)
    }

    private func fallbackScreen() -> NSScreen? {
        if let mainScreen = NSScreen.main {
            return mainScreen
        }
        return NSScreen.screens.first
    }

    private func frontmostScreen() -> NSScreen? {
        if let frame = frontmostWindowFrame() {
            return screenIntersecting(frame)
        }

        let mouseLocation = NSEvent.mouseLocation
        return NSScreen.screens.first { NSMouseInRect(mouseLocation, $0.frame, false) }
    }

    private func frontmostWindowFrame() -> CGRect? {
        guard let app = NSWorkspace.shared.frontmostApplication else {
            return nil
        }

        let appElement = AXUIElementCreateApplication(app.processIdentifier)
        var focusedWindowObject: CFTypeRef?
        let focusedWindowStatus = AXUIElementCopyAttributeValue(
            appElement,
            kAXFocusedWindowAttribute as CFString,
            &focusedWindowObject
        )
        guard focusedWindowStatus == .success,
              let focusedWindowObject,
              CFGetTypeID(focusedWindowObject) == AXUIElementGetTypeID()
        else {
            return nil
        }

        let windowElement = unsafeDowncast(focusedWindowObject as AnyObject, to: AXUIElement.self)
        guard let origin = pointAttribute(named: kAXPositionAttribute as CFString, on: windowElement),
              let size = sizeAttribute(named: kAXSizeAttribute as CFString, on: windowElement)
        else {
            return nil
        }

        return CGRect(origin: origin, size: size)
    }

    private func pointAttribute(named name: CFString, on element: AXUIElement) -> CGPoint? {
        var rawValue: CFTypeRef?
        let status = AXUIElementCopyAttributeValue(element, name, &rawValue)
        guard status == .success,
              let rawValue,
              CFGetTypeID(rawValue) == AXValueGetTypeID()
        else {
            return nil
        }

        var point = CGPoint.zero
        guard AXValueGetValue(rawValue as! AXValue, .cgPoint, &point) else {
            return nil
        }
        return point
    }

    private func sizeAttribute(named name: CFString, on element: AXUIElement) -> CGSize? {
        var rawValue: CFTypeRef?
        let status = AXUIElementCopyAttributeValue(element, name, &rawValue)
        guard status == .success,
              let rawValue,
              CFGetTypeID(rawValue) == AXValueGetTypeID()
        else {
            return nil
        }

        var size = CGSize.zero
        guard AXValueGetValue(rawValue as! AXValue, .cgSize, &size) else {
            return nil
        }
        return size
    }

    private func screenIntersecting(_ frame: CGRect) -> NSScreen? {
        NSScreen.screens.max { lhs, rhs in
            intersectionArea(of: lhs.frame, with: frame) < intersectionArea(of: rhs.frame, with: frame)
        }
    }

    private func intersectionArea(of lhs: CGRect, with rhs: CGRect) -> CGFloat {
        let intersection = lhs.intersection(rhs)
        guard !intersection.isNull else {
            return 0
        }
        return intersection.width * intersection.height
    }

    private func copyCurrentText() {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(state.text, forType: .string)
        state.copyFeedback = true
        Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: 1_200_000_000)
            guard let self, !Task.isCancelled else { return }
            state.copyFeedback = false
        }
    }

    private func updateState(
        mode: SubtitleOverlayMode,
        phase: SubtitleOverlayPhase,
        text: String,
        detail: String,
        interactive: Bool
    ) {
        state.mode = mode
        state.phase = phase
        state.text = text
        state.detail = detail
        state.isInteractive = interactive
        state.copyFeedback = false
        if phase != .recording {
            state.waveLevel = 0
        }
    }

    private func cancelTransientTasks() {
        hideTask?.cancel()
        hideTask = nil
        animationTask?.cancel()
        animationTask = nil
    }
}
