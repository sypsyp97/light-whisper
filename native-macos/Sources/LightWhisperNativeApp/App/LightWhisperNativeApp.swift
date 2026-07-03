import SwiftUI

@main
struct LightWhisperNativeApp: App {
    @StateObject private var model = AppModel()

    @Environment(\.openWindow) private var openWindow

    var body: some Scene {
        WindowGroup("Light Whisper", id: AppModel.mainWindowSceneID) {
            ContentView(model: model)
                .onAppear {
                    model.openMainWindowScene = {
                        openWindow(id: AppModel.mainWindowSceneID)
                    }
                }
        }

        Settings {
            SettingsView(model: model)
        }

        MenuBarExtra("Light Whisper", systemImage: "waveform") {
            Button("Show Main Window") {
                model.handleStatusAction(.showMainWindow)
            }
            Button("Start Dictation") {
                model.handleStatusAction(.toggleDictation)
            }
            Button("Start Translation") {
                model.handleStatusAction(.startTranslation)
            }
            Button("Start Assistant") {
                model.handleStatusAction(.startAssistant)
            }
            Divider()
            Button("Check for Updates") {
                model.handleStatusAction(.checkForUpdates)
            }
            Button("Quit Light Whisper") {
                model.handleStatusAction(.quitApplication)
            }
        }
    }
}
