import { StrictMode, act } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { UserProfile } from "@/types";

const api = vi.hoisted(() => ({
  getSelectionApiKey: vi.fn(),
  listAiModels: vi.fn(),
  setSelectionApiKey: vi.fn(),
  setSelectionAssistantConfig: vi.fn(),
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
    min_chars: 4,
    max_chars: 900,
    translation_target: "English",
    excluded_apps: ["old.exe"],
  },
};

function section(profileValue: UserProfile) {
  return (
    <SelectionAssistantSettingsSection
      profile={profileValue}
      openaiAuthMode="api_key"
      openaiOauthLoggedIn={false}
      openaiControls={null}
    />
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  api.getSelectionApiKey.mockResolvedValue("");
  api.listAiModels.mockResolvedValue({ models: [], sourceUrl: "" });
  api.setSelectionApiKey.mockResolvedValue(undefined);
  api.setSelectionAssistantConfig.mockResolvedValue(undefined);
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("SelectionAssistantSettingsSection", () => {
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
    fireEvent.change(screen.getByDisplayValue("English"), {
      target: { value: "German" },
    });
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
    expect(screen.getByDisplayValue("German")).toBeInTheDocument();
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
      minChars: 4,
      maxChars: 900,
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
    fireEvent.change(screen.getByDisplayValue("English"), {
      target: { value: "German" },
    });
    fireEvent.change(screen.getByDisplayValue("old.exe"), {
      target: { value: "alpha.exe\nbeta.exe" },
    });

    expect(api.setSelectionAssistantConfig).not.toHaveBeenCalled();
    rendered.unmount();

    expect(api.setSelectionAssistantConfig).toHaveBeenCalledTimes(1);
    expect(api.setSelectionAssistantConfig).toHaveBeenCalledWith({
      enabled: false,
      autoScreenshot: true,
      minChars: 4,
      maxChars: 900,
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
        min_chars: 5,
        max_chars: 1200,
        translation_target: "Japanese",
        excluded_apps: ["new.exe"],
      },
    };

    rendered.rerender(section(updatedProfile));

    expect(screen.getByRole("switch", { name: "settings.selectionAssistantEnabled" }))
      .toHaveAttribute("aria-checked", "false");
    expect(screen.getByRole("switch", { name: "settings.selectionAutoScreenshot" }))
      .toHaveAttribute("aria-checked", "false");
    expect(screen.getByRole("switch", { name: "settings.selectionSeparateConfig" }))
      .toHaveAttribute("aria-checked", "true");
    expect(screen.getByDisplayValue("deepseek-chat")).toBeInTheDocument();
    expect(screen.getByDisplayValue("Japanese")).toBeInTheDocument();
    expect(screen.getByDisplayValue("5")).toBeInTheDocument();
    expect(screen.getByDisplayValue("1200")).toBeInTheDocument();
    expect(screen.getByDisplayValue("new.exe")).toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(1000);
    });
    rendered.unmount();
    expect(api.setSelectionAssistantConfig).not.toHaveBeenCalled();
  });
});
