import SwiftUI

struct SettingsView: View {
    @ObservedObject var model: AppModel

    private var activeProviderSupportsBaseURL: Bool {
        !LLMProviderConfig.isBuiltinProvider(model.userProfile.llmProvider.resolveActiveProvider())
    }

    var body: some View {
        Form {
            speechSection
            providersSection
            assistantSection
            keysSection
        }
        .padding(24)
        .frame(minWidth: 720, minHeight: 560)
        .onDisappear {
            model.flushPendingChanges()
        }
        .onChange(of: model.onlineASRAPIKey) {
            model.persistOnlineASRAPIKey()
        }
        .onChange(of: model.aiPolishAPIKey) {
            model.persistAIPolishAPIKey()
        }
        .onChange(of: model.assistantAPIKey) {
            model.persistAssistantAPIKey()
        }
        .onChange(of: model.webSearchAPIKey) {
            model.persistWebSearchAPIKey()
        }
    }

    private var speechSection: some View {
        Section(SettingsSectionID.speech.title) {
            Picker("Engine", selection: $model.engineSettings.engine) {
                Text("Alibaba DashScope").tag(EngineKind.alibabaAsr)
                Text("GLM-ASR").tag(EngineKind.glmAsr)
            }
            .onChange(of: model.engineSettings.engine) {
                model.flushPendingChanges()
            }

            if model.engineSettings.engine == .alibabaAsr {
                Picker("Alibaba Region", selection: $model.engineSettings.alibabaRegion) {
                    Text("International").tag(OnlineRegion.international)
                    Text("China").tag(OnlineRegion.domestic)
                }
            }

            Picker("Input Device", selection: $model.selectedInputDeviceUID) {
                Text("System Default").tag(nil as String?)
            }
            .onChange(of: model.selectedInputDeviceUID) {
                model.selectInputDevice(uid: model.selectedInputDeviceUID)
            }
        }
    }

    private var providersSection: some View {
        Section(SettingsSectionID.providers.title) {
            Picker("Provider", selection: $model.userProfile.llmProvider.active) {
                Text("Cerebras").tag("cerebras")
                Text("OpenAI").tag("openai")
                Text("DeepSeek").tag("deepseek")
                Text("SiliconFlow").tag("siliconflow")
            }
            .onChange(of: model.userProfile.llmProvider.active) {
                model.flushPendingChanges()
            }

            if activeProviderSupportsBaseURL {
                TextField("Base URL", text: Binding(
                    get: { model.userProfile.llmProvider.customBaseURL ?? "" },
                    set: { model.userProfile.llmProvider.customBaseURL = $0 }
                ))
            }
        }
    }

    private var assistantSection: some View {
        Section(SettingsSectionID.assistant.title) {
            Picker("Assistant Provider", selection: Binding(
                get: { model.userProfile.llmProvider.assistantProvider ?? model.userProfile.llmProvider.active },
                set: { model.userProfile.llmProvider.assistantProvider = $0 }
            )) {
                Text("Use Active Provider").tag(model.userProfile.llmProvider.active)
                Text("OpenAI").tag("openai")
                Text("DeepSeek").tag("deepseek")
            }
            .onChange(of: model.userProfile.llmProvider.assistantProvider) {
                model.flushPendingChanges()
            }

            TextField(
                        "Assistant Model",
                        text: Binding(
                            get: { model.userProfile.llmProvider.assistantModel ?? "" },
                            set: { model.userProfile.llmProvider.assistantModel = $0 }
                        )
            )

            Toggle("Screen Context", isOn: $model.userProfile.assistantScreenContextEnabled)
        }
    }

    private var keysSection: some View {
        Section("API Keys") {
            SecureField("Online ASR API Key", text: $model.onlineASRAPIKey)
            SecureField("AI Polish API Key", text: $model.aiPolishAPIKey)
            SecureField("Assistant API Key", text: $model.assistantAPIKey)
            SecureField("Web Search API Key", text: $model.webSearchAPIKey)
        }
    }
}
