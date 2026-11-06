import { StrictMode, act } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { UserProfile } from "@/types";
import type { ModelConnectionRequest } from "@/api/tauri";

const api = vi.hoisted(() => ({
  getSelectionApiKey: vi.fn(),
  listAiModels: vi.fn(),
  setSelectionApiKey: vi.fn(),
  setSelectionAssistantConfig: vi.fn(),
  testModelConnection: vi.fn(),
}));
const i18n = vi.hoisted(() => ({
  t: (key: string) => key,
}));

vi.mock("@/api/tauri", () => api);
vi.mock("sonner", () => ({ toast: { error: vi.fn() } }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: i18n.t,
  }),
}));

import SelectionAssistantSettingsSection from "@/components/settings/SelectionAssistantSettingsSection";

const profile: UserProfile = {
  hot_words: [],
  correction_patterns: [],
  vocab_frequency: {},
  total_transcriptions: 0,
  last_updated: 0,
  llm_provider: {
    active: "openai",
    custom_providers: [],
  },
  selection_assistant: {
    enabled: true,
    auto_screenshot: true,
    translation_target: "English",
    excluded_apps: ["old.exe"],
  },
};

function section(profileValue: UserProfile, polishConnection?: ModelConnectionRequest) {
  return (
    <SelectionAssistantSettingsSection
      profile={profileValue}
      polishConnection={polishConnection}
      openaiAuthMode="api_key"
      openaiOauthLoggedIn={false}
      openaiControls={null}
      onConfigureJev={vi.fn()}
      onSaved={vi.fn()}
    />
  );
}

function selectCustomTarget(value: string) {
  fireEvent.click(screen.getByRole("button", {
    name: "settings.selectionTranslationTarget",
  }));
  fireEvent.click(screen.getByText("settings.customLang"));
  const input = screen.getByRole("textbox", { name: "settings.customLangLabel" });
  fireEvent.change(input, { target: { value } });
  fireEvent.keyDown(input, { key: "Enter" });
}

async function renderSelectionWithPendingProviderKey() {
  let resolve!: (key: string) => void;
  const pendingKey = new Promise<string>((next) => { resolve = next; });
  api.getSelectionApiKey.mockImplementation((provider: string) => (
    provider === "deepseek" ? pendingKey : Promise.resolve("provider-a-key")
  ));
  await act(async () => {
    render(section({ ...profile, llm_provider: {
      ...profile.llm_provider, selection_use_separate_model: true,
      selection_provider: "openai", selection_model: "provider-a-model",
    } }));
  });
  expect(screen.getByLabelText("settings.selectionApiKey")).toHaveValue("provider-a-key");
  return { pendingKey, resolve };
}

async function selectDeepSeek() {
  fireEvent.click(screen.getByRole("button", { name: "settings.selectionProvider" }));
  await act(async () => { fireEvent.click(screen.getByRole("option", { name: /^DeepSeek/ })); });
  expect(api.getSelectionApiKey).toHaveBeenLastCalledWith("deepseek");
}

beforeEach(() => {
  vi.clearAllMocks();
  api.getSelectionApiKey.mockResolvedValue("");
  api.listAiModels.mockResolvedValue({ models: [], sourceUrl: "" });
  api.setSelectionApiKey.mockResolvedValue(undefined);
  api.setSelectionAssistantConfig.mockResolvedValue(undefined);
  api.testModelConnection.mockResolvedValue({ model: "tested-model", elapsedMs: 12 });
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("SelectionAssistantSettingsSection", () => {
  describe("connection credential isolation", () => {
    it("invalidates the old success and never tests a new provider with the previous provider key", async () => {
      const read = await renderSelectionWithPendingProviderKey();
      try {
        await act(async () => { fireEvent.click(screen.getByRole("button", { name: "settings.testModelConnection" })); });
        expect(screen.getByText("settings.modelConnectionSuccess")).toBeVisible();
        await selectDeepSeek();
        expect(screen.queryByText("settings.modelConnectionSuccess")).not.toBeInTheDocument();

        await act(async () => { fireEvent.click(screen.getByRole("button", { name: "settings.testModelConnection" })); });

        expect(api.testModelConnection).not.toHaveBeenCalledWith(expect.objectContaining({
          provider: "deepseek", apiKey: "provider-a-key",
        }));
      } finally {
        await act(async () => { read.resolve("stored-provider-b-key"); await read.pendingKey; });
      }
    });

    it("tests a new provider's local key draft and preserves it when its stored key arrives late", async () => {
      const read = await renderSelectionWithPendingProviderKey();
      await selectDeepSeek();
      fireEvent.change(screen.getByLabelText("settings.selectionApiKey"), { target: { value: "provider-b-draft-key" } });
      const button = screen.getByRole("button", { name: "settings.testModelConnection" });
      expect(button).not.toBeDisabled();
      await act(async () => { fireEvent.click(button); });
      expect(api.testModelConnection).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
        target: "selection", provider: "deepseek", apiKey: "provider-b-draft-key",
      }));

      await act(async () => { read.resolve("stored-provider-b-key"); await read.pendingKey; });

      expect(screen.getByLabelText("settings.selectionApiKey")).toHaveValue("provider-b-draft-key");
      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "settings.testModelConnection" })); });
      expect(api.testModelConnection).toHaveBeenLastCalledWith(expect.objectContaining({
        provider: "deepseek", apiKey: "provider-b-draft-key",
      }));
    });
  });

  it("tests the supplied polish draft when selection follows polish", async () => {
    const polishConnection: ModelConnectionRequest = {
      target: "polish", provider: "custom-polish", baseUrl: "https://polish.example/v1",
      model: "draft-polish-model", apiKey: "draft-polish-key", reasoningMode: "deep",
      openaiAuthMode: "api_key", xaiAuthMode: "api_key",
    };
    await act(async () => { render(section(profile, polishConnection)); });
    expect(api.testModelConnection).not.toHaveBeenCalled();

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "settings.testModelConnection" })); });

    expect(api.testModelConnection).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
      ...polishConnection, target: "selection",
    }));
  });

  it("tests the independent selection model with its current model and key drafts", async () => {
    api.getSelectionApiKey.mockResolvedValue("saved-selection-key");
    await act(async () => {
      render(section({
        ...profile,
        llm_provider: {
          ...profile.llm_provider,
          custom_providers: [{ id: "selection-test", name: "Selection Test", base_url: "https://selection.example/v1", model: "provider-default", api_format: "openai_compat" }],
          selection_use_separate_model: true, selection_provider: "selection-test",
          selection_model: "saved-selection-model", selection_reasoning_mode: "balanced",
        },
      }, { target: "polish", provider: "openai", model: "other-model", apiKey: "polish-only-key" }));
    });
    expect(screen.getByLabelText("settings.selectionApiKey")).toHaveValue("saved-selection-key");
    fireEvent.change(screen.getByRole("textbox", { name: "settings.assistantModelLabel" }), { target: { value: "draft-selection-model" } });
    fireEvent.change(screen.getByLabelText("settings.selectionApiKey"), { target: { value: "draft-selection-key" } });
    expect(api.testModelConnection).not.toHaveBeenCalled();

    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "settings.testModelConnection" })); });

    expect(api.testModelConnection).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
      target: "selection", provider: "selection-test", baseUrl: "https://selection.example/v1",
      model: "draft-selection-model", apiKey: "draft-selection-key", reasoningMode: "balanced", openaiAuthMode: "api_key",
    }));
  });

  it("saves xAI config and key, restores the separate model, and can disable it", async () => {
    const rendered = render(section(profile));
    await act(async () => {
      fireEvent.click(screen.getByRole("switch", { name: "settings.selectionSeparateConfig" }));
    });
    fireEvent.click(screen.getByRole("button", { name: "settings.selectionProvider" }));
    fireEvent.click(screen.getByRole("option", { name: /^xAI Grok/ }));
    await act(async () => { await vi.advanceTimersByTimeAsync(350); });

    expect(api.setSelectionAssistantConfig).toHaveBeenLastCalledWith(
      expect.objectContaining({ useSeparateModel: true, provider: "xai", model: "grok-4.7" }),
    );
    expect(api.getSelectionApiKey).toHaveBeenLastCalledWith("xai");
    fireEvent.change(screen.getByPlaceholderText("xAI Grok API Key"), {
      target: { value: "test-xai-key" },
    });
    await act(async () => { await vi.advanceTimersByTimeAsync(400); });
    expect(api.setSelectionApiKey).toHaveBeenLastCalledWith("xai", "test-xai-key");

    rendered.unmount();
    const savedProfile: UserProfile = JSON.parse(JSON.stringify({
      ...profile,
      llm_provider: {
        ...profile.llm_provider,
        selection_use_separate_model: true,
        selection_provider: "xai",
        selection_model: "grok-4.6",
      },
    }));
    await act(async () => { render(section(savedProfile)); });
    expect(screen.getByRole("switch", { name: "settings.selectionSeparateConfig" }))
      .toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("button", { name: "settings.selectionProvider" }))
      .toHaveTextContent("xAI Grok");
    expect(screen.getByDisplayValue("grok-4.6")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("switch", { name: "settings.selectionAssistantEnabled" }));
    await act(async () => { await vi.advanceTimersByTimeAsync(350); });
    expect(api.setSelectionAssistantConfig).toHaveBeenLastCalledWith(
      expect.objectContaining({ enabled: false, useSeparateModel: true, provider: "xai" }),
    );
  });

  it("does not persist programmatic hydration in StrictMode", () => {
    const rendered = render(
      <StrictMode>{section(profile)}</StrictMode>,
    );

    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(api.setSelectionAssistantConfig).not.toHaveBeenCalled();

    rendered.unmount();
    expect(api.setSelectionAssistantConfig).not.toHaveBeenCalled();
  });

  it("keeps a dirty draft and pending save across an unrelated profile refresh", () => {
    const rendered = render(section(profile));

    fireEvent.click(screen.getByRole("switch", { name: "settings.selectionAssistantEnabled" }));
    selectCustomTarget("German");
    fireEvent.change(screen.getByDisplayValue("old.exe"), {
      target: { value: "alpha.exe\nbeta.exe" },
    });

    const refreshedProfile: UserProfile = {
      ...profile,
      hot_words: [{
        text: "unrelated",
        weight: 1,
        source: "user",
        use_count: 0,
        last_used: 0,
      }],
      history_settings: { enabled: true, save_audio: false, retention_days: 30 },
      llm_provider: { ...profile.llm_provider, custom_providers: [] },
      selection_assistant: { ...profile.selection_assistant! },
    };
    rendered.rerender(section(refreshedProfile));

    expect(screen.getByRole("switch", { name: "settings.selectionAssistantEnabled" }))
      .toHaveAttribute("aria-checked", "false");
    expect(screen.getByRole("button", { name: "settings.selectionTranslationTarget" }))
      .toHaveTextContent("German");
    expect(rendered.container.querySelector("textarea"))
      .toHaveValue("alpha.exe\nbeta.exe");

    act(() => {
      vi.advanceTimersByTime(349);
    });
    expect(api.setSelectionAssistantConfig).not.toHaveBeenCalled();
    act(() => {
      vi.advanceTimersByTime(1);
    });

    expect(api.setSelectionAssistantConfig).toHaveBeenCalledTimes(1);
    expect(api.setSelectionAssistantConfig).toHaveBeenCalledWith({
      enabled: false,
      autoScreenshot: true,
      screenshotRouting: false,
      customPrompt: null,
      translationTarget: "German",
      excludedApps: ["alpha.exe", "beta.exe"],
      useSeparateModel: false,
      provider: null,
      model: null,
      reasoningMode: "provider_default",
    });

    rendered.unmount();
    expect(api.setSelectionAssistantConfig).toHaveBeenCalledTimes(1);
  });

  it("flushes the latest complete config exactly once on a real unmount", () => {
    const rendered = render(
      <StrictMode>{section(profile)}</StrictMode>,
    );

    fireEvent.click(screen.getByRole("switch", { name: "settings.selectionAssistantEnabled" }));
    selectCustomTarget("German");
    fireEvent.change(screen.getByDisplayValue("old.exe"), {
      target: { value: "alpha.exe\nbeta.exe" },
    });

    expect(api.setSelectionAssistantConfig).not.toHaveBeenCalled();
    rendered.unmount();

    expect(api.setSelectionAssistantConfig).toHaveBeenCalledTimes(1);
    expect(api.setSelectionAssistantConfig).toHaveBeenCalledWith({
      enabled: false,
      autoScreenshot: true,
      screenshotRouting: false,
      customPrompt: null,
      translationTarget: "German",
      excludedApps: ["alpha.exe", "beta.exe"],
      useSeparateModel: false,
      provider: null,
      model: null,
      reasoningMode: "provider_default",
    });
  });

  it("hydrates a relevant backend update without writing it back", () => {
    const rendered = render(section(profile));
    const updatedProfile: UserProfile = {
      ...profile,
      llm_provider: {
        ...profile.llm_provider,
        selection_use_separate_model: true,
        selection_provider: "deepseek",
        selection_model: "deepseek-chat",
        selection_reasoning_mode: "deep",
      },
      selection_assistant: {
        enabled: false,
        auto_screenshot: false,
        translation_target: "Japanese",
        excluded_apps: ["new.exe"],
      },
    };

    rendered.rerender(section(updatedProfile));

    expect(screen.getByRole("switch", { name: "settings.selectionAssistantEnabled" }))
      .toHaveAttribute("aria-checked", "false");
    expect(screen.getByRole("button", { name: "settings.processingModeOff" }))
      .toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("switch", { name: "settings.selectionSeparateConfig" }))
      .toHaveAttribute("aria-checked", "true");
    expect(screen.getByDisplayValue("deepseek-chat")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "settings.selectionTranslationTarget" }))
      .toHaveTextContent("Japanese");
    expect(screen.getByDisplayValue("new.exe")).toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(1000);
    });
    rendered.unmount();
    expect(api.setSelectionAssistantConfig).not.toHaveBeenCalled();
  });

  it("does not expose selection length controls", () => {
    render(section(profile));

    expect(screen.queryByText("settings.selectionLengthRange")).not.toBeInTheDocument();
    expect(screen.queryByText("settings.selectionMinChars")).not.toBeInTheDocument();
    expect(screen.queryByText("settings.selectionMaxChars")).not.toBeInTheDocument();
  });

  it("defaults older profiles to Off and saves On, Auto, and selection-specific instructions", async () => {
    render(section({ ...profile, selection_assistant: undefined }));

    expect(screen.getByRole("button", { name: "settings.processingModeOff" }))
      .toHaveAttribute("aria-pressed", "true");
    fireEvent.click(screen.getByRole("button", { name: "settings.processingModeOn" }));
    await act(async () => { await vi.advanceTimersByTimeAsync(350); });
    expect(api.setSelectionAssistantConfig).toHaveBeenLastCalledWith(expect.objectContaining({
      autoScreenshot: true,
      screenshotRouting: false,
    }));

    fireEvent.click(screen.getByRole("button", { name: "settings.processingModeAuto" }));
    fireEvent.click(screen.getByRole("button", { name: "settings.selectionCustomPrompt" }));
    fireEvent.change(screen.getByRole("textbox", { name: "settings.selectionCustomPromptLabel" }), {
      target: { value: "Keep acronyms" },
    });
    await act(async () => { await vi.advanceTimersByTimeAsync(350); });

    expect(api.setSelectionAssistantConfig).toHaveBeenLastCalledWith(expect.objectContaining({
      autoScreenshot: true,
      screenshotRouting: true,
      customPrompt: "Keep acronyms",
    }));

    fireEvent.click(screen.getByRole("button", { name: "settings.processingModeOff" }));
    await act(async () => { await vi.advanceTimersByTimeAsync(350); });
    expect(api.setSelectionAssistantConfig).toHaveBeenLastCalledWith(expect.objectContaining({
      autoScreenshot: false,
      screenshotRouting: false,
      customPrompt: "Keep acronyms",
    }));
  });

  it("uses the shared picker styles and offers common language presets", () => {
    render(section(profile));

    const trigger = screen.getByRole("button", {
      name: "settings.selectionTranslationTarget",
    });
    expect(trigger).toHaveClass("picker-trigger");
    fireEvent.click(trigger);

    const simplifiedChinese = screen.getByRole("option", { name: "简体中文" });
    expect(simplifiedChinese).toHaveClass("picker-option");
    fireEvent.click(simplifiedChinese);

    act(() => {
      vi.advanceTimersByTime(350);
    });
    expect(api.setSelectionAssistantConfig).toHaveBeenLastCalledWith(
      expect.objectContaining({ translationTarget: "简体中文" }),
    );
  });
});
