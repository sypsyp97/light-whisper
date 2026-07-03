import SwiftUI

struct SettingsView: View {
    @ObservedObject var model: AppModel
    private static let followActiveAssistantProviderTag = "__follow_active_provider__"

    private var activeProviderSupportsBaseURL: Bool {
        let activeProvider = model.userProfile.llmProvider.resolveActiveProvider()
        return activeProvider == "custom"
            || model.userProfile.llmProvider.customProviders.contains { $0.id == activeProvider }
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
            Picker("Engine", selection: Binding(
                get: { model.engineSettings.engine },
                set: { nextEngine in
                    model.persistOnlineASRAPIKey()
                    model.engineSettings.engine = nextEngine
                    model.persistEngineSettings()
                    model.loadOnlineASRAPIKey()
                }
            )) {
                Text("Alibaba DashScope").tag(EngineKind.alibabaAsr)
                Text("GLM-ASR").tag(EngineKind.glmAsr)
            }

            if model.engineSettings.engine == .alibabaAsr {
                Picker("Alibaba Region", selection: Binding(
                    get: { model.engineSettings.alibabaRegion },
                    set: { nextRegion in
                        model.persistOnlineASRAPIKey()
                        model.engineSettings.alibabaRegion = nextRegion
                        model.persistEngineSettings()
                        model.loadOnlineASRAPIKey()
                    }
                )) {
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
            Picker("Provider", selection: Binding(
                get: { model.userProfile.llmProvider.active },
                set: { nextProvider in
                    model.persistAIPolishAPIKey()
                    model.persistAssistantAPIKey()
                    model.userProfile.llmProvider.active = nextProvider
                    model.persistUserProfile()
                    model.loadAIPolishAPIKey()
                    model.loadAssistantAPIKey()
                }
            )) {
                Text("Cerebras").tag("cerebras")
                Text("OpenAI").tag("openai")
                Text("DeepSeek").tag("deepseek")
                Text("SiliconFlow").tag("siliconflow")
                Text("Custom").tag("custom")
                ForEach(model.userProfile.llmProvider.customProviders) { provider in
                    Text(provider.name).tag(provider.id)
                }
            }

            if activeProviderSupportsBaseURL {
                TextField("Base URL", text: Binding(
                    get: { activeProviderBaseURL() },
                    set: { setActiveProviderBaseURL($0) }
                ))
            }

            TextField("Model", text: Binding(
                get: { activeProviderModel() },
                set: { setActiveProviderModel($0) }
            ))
        }
    }

    private var assistantSection: some View {
        Section(SettingsSectionID.assistant.title) {
            Picker("Assistant Provider", selection: Binding(
                get: {
                    model.userProfile.llmProvider.assistantUseSeparateModel
                        ? (model.userProfile.llmProvider.assistantProvider ?? model.userProfile.llmProvider.active)
                        : Self.followActiveAssistantProviderTag
                },
                set: { nextProvider in
                    model.persistAssistantAPIKey()
                    if nextProvider == Self.followActiveAssistantProviderTag {
                        model.userProfile.llmProvider.assistantUseSeparateModel = false
                        model.userProfile.llmProvider.assistantProvider = nil
                    } else {
                        model.userProfile.llmProvider.assistantUseSeparateModel = true
                        model.userProfile.llmProvider.assistantProvider = nextProvider
                    }
                    model.persistUserProfile()
                    model.loadAssistantAPIKey()
                }
            )) {
                Text("Use Active Provider").tag(Self.followActiveAssistantProviderTag)
                Text("Cerebras").tag("cerebras")
                Text("OpenAI").tag("openai")
                Text("DeepSeek").tag("deepseek")
                Text("SiliconFlow").tag("siliconflow")
                Text("Custom").tag("custom")
                ForEach(model.userProfile.llmProvider.customProviders) { provider in
                    Text(provider.name).tag(provider.id)
                }
            }

            if model.userProfile.llmProvider.assistantUseSeparateModel {
                TextField(
                    "Assistant Model",
                    text: Binding(
                        get: { model.userProfile.llmProvider.assistantModel ?? "" },
                        set: {
                            model.userProfile.llmProvider.assistantModel = normalizedOptional($0)
                            model.persistUserProfile()
                        }
                    )
                )
            }

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

    private func activeProviderBaseURL() -> String {
        let activeProvider = model.userProfile.llmProvider.resolveActiveProvider()
        if activeProvider == "custom" {
            return model.userProfile.llmProvider.customBaseURL ?? ""
        }
        guard let index = model.userProfile.llmProvider.customProviders.firstIndex(where: { $0.id == activeProvider }) else {
            return ""
        }
        return model.userProfile.llmProvider.customProviders[index].baseURL
    }

    private func setActiveProviderBaseURL(_ value: String) {
        let activeProvider = model.userProfile.llmProvider.resolveActiveProvider()
        if activeProvider == "custom" {
            model.userProfile.llmProvider.customBaseURL = normalizedOptional(value)
        } else if let index = model.userProfile.llmProvider.customProviders.firstIndex(where: { $0.id == activeProvider }) {
            model.userProfile.llmProvider.customProviders[index].baseURL = value
        }
        model.persistUserProfile()
    }

    private func activeProviderModel() -> String {
        let activeProvider = model.userProfile.llmProvider.resolveActiveProvider()
        guard let index = model.userProfile.llmProvider.customProviders.firstIndex(where: { $0.id == activeProvider }) else {
            return model.userProfile.llmProvider.customModel ?? ""
        }
        return model.userProfile.llmProvider.customProviders[index].model
    }

    private func setActiveProviderModel(_ value: String) {
        let activeProvider = model.userProfile.llmProvider.resolveActiveProvider()
        if let index = model.userProfile.llmProvider.customProviders.firstIndex(where: { $0.id == activeProvider }) {
            model.userProfile.llmProvider.customProviders[index].model = value
        } else {
            model.userProfile.llmProvider.customModel = normalizedOptional(value)
        }
        model.persistUserProfile()
    }

    private func normalizedOptional(_ value: String) -> String? {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}
