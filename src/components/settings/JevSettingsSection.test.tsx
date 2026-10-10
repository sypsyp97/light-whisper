import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UserProfile } from "@/types";

const tauriMock = vi.hoisted(() => ({
  getJevApiKey: vi.fn(),
  setJevApiKey: vi.fn(),
  setJevProvider: vi.fn(),
  setJevFeatures: vi.fn(),
}));

const toastMock = vi.hoisted(() => ({
  error: vi.fn(),
}));

vi.mock("@/api/tauri", () => tauriMock);
vi.mock("sonner", () => ({ toast: toastMock }));

const labels: Record<string, string> = {
  "settings.jev": "Jev",
  "settings.jevEnabled": "Enable Jev gate",
  "settings.jevModel": "Decision model",
  "settings.jevProvider": "Decision provider",
  "settings.jevApiKey": "Decision API key",
  "settings.jevSaveFailed": "Decision settings save failed",
  "settings.jevScreenRouting": "Use screen context only when needed",
  "settings.jevCorrectionReview": "Review learned corrections",
  "settings.jevSearchRouting": "Decide when to search",
  "settings.jevPolishAudit": "Check for changed meaning",
  "settings.showApiKey": "Show API Key",
  "settings.hideApiKey": "Hide API Key",
};

function translate(key: string) {
  return labels[key] ?? key;
}

vi.mock("@/i18n", () => ({
  default: {
    changeLanguage: vi.fn(),
    language: "en",
    t: translate,
  },
}));
vi.mock("react-i18next", () => ({
  initReactI18next: { init: vi.fn(), type: "3rdParty" },
  useTranslation: () => ({
    i18n: { changeLanguage: vi.fn(), language: "en" },
    t: translate,
  }),
}));

import JevSettingsSection from "@/components/settings/JevSettingsSection";

type JevProvider = "typesafe" | "openrouter" | "vercel" | "liquid";
type JevProfile = UserProfile & {
  jev?: {
    enabled: boolean;
    provider: JevProvider;
  };
};

const baseProfile = {
  blocked_hot_words: [],
  correction_patterns: [],
  correction_validation_enabled: false,
  custom_prompt: null,
  hot_words: [],
  history_settings: { enabled: false, save_audio: false, retention_days: 90 },
  last_correction_validation: 0,
  last_updated: 0,
  llm_provider: { active: "cerebras", custom_providers: [] },
  polish_structure_level: "off",
  selection_assistant: {
    enabled: false,
    auto_screenshot: false,
    translation_target: "English",
    excluded_apps: [],
  },
  total_transcriptions: 0,
  translation_hotkey: null,
  translation_target: null,
  vocab_frequency: {},
  web_search: { enabled: false, max_results: 5, provider: "model_native" },
} as JevProfile;

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((nextResolve) => {
    resolve = nextResolve;
  });
  return { promise, resolve };
}

function renderSection(profile: JevProfile, onSaved = vi.fn()) {
  render(<JevSettingsSection profile={profile} onSaved={onSaved} />);
  return onSaved;
}

function getToggle() {
  return screen.getByRole("switch", { name: "Review learned corrections" });
}

function getProviderSelect() {
  return screen.getByRole("combobox", { name: /decision provider/i });
}

function getModelSelect() {
  return screen.getByRole("combobox", { name: "Decision model" });
}

function getApiKeyInput() {
  return screen.getByLabelText(/decision.*api|api.*key/i);
}

beforeEach(() => {
  tauriMock.getJevApiKey.mockReset();
  tauriMock.getJevApiKey.mockResolvedValue("");
  tauriMock.setJevApiKey.mockReset();
  tauriMock.setJevApiKey.mockResolvedValue(undefined);
  tauriMock.setJevProvider.mockReset();
  tauriMock.setJevProvider.mockResolvedValue(undefined);
  tauriMock.setJevFeatures.mockReset();
  tauriMock.setJevFeatures.mockResolvedValue(undefined);
  toastMock.error.mockReset();
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("JevSettingsSection", () => {
  it("shows the shared JEV connection when selection screenshots use Auto", async () => {
    await act(async () => {
      render(<JevSettingsSection profile={{
        ...baseProfile,
        selection_assistant: {
          ...baseProfile.selection_assistant!,
          auto_screenshot: true,
          screenshot_routing: true,
        },
      }} polishEnabled={false} onSaved={vi.fn()} />);
    });

    expect(getProviderSelect()).toBeInTheDocument();
    expect(getApiKeyInput()).toBeInTheDocument();
    expect(tauriMock.getJevApiKey).toHaveBeenCalledWith("typesafe");
  });

  it("defaults off and expands exactly three providers with a password key input", async () => {
    const onSaved = renderSection({ ...baseProfile, jev: undefined });

    expect(getToggle()).toHaveAttribute("aria-checked", "false");
    expect(screen.queryByRole("combobox", { name: "Decision provider" })).not.toBeInTheDocument();

    fireEvent.click(getToggle());

    await waitFor(() => {
      expect(tauriMock.setJevFeatures).toHaveBeenCalledWith({ correction_review: true, polish_audit: false });
    });
    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));

    const providerSelect = getProviderSelect() as HTMLSelectElement;
    expect(Array.from(providerSelect.options).map((option) => option.value)).toEqual([
      "typesafe",
      "openrouter",
      "vercel",
    ]);
    expect(getModelSelect()).toHaveValue("jev");
    expect(Array.from((getModelSelect() as HTMLSelectElement).options).map((option) => option.value)).toEqual([
      "jev",
      "d1",
    ]);
    expect(getApiKeyInput()).toHaveAttribute("type", "password");
  });

  it("switches to d1 with Liquid AI and loads its isolated credential without stale-key overwrite", async () => {
    const oldOpenrouterKey = deferred<string>();
    tauriMock.getJevApiKey.mockImplementation((provider: JevProvider) => (
      provider === "openrouter" ? oldOpenrouterKey.promise : Promise.resolve("liquid-key")
    ));
    renderSection({
      ...baseProfile,
      jev: { enabled: true, provider: "openrouter" },
    });

    await waitFor(() => expect(tauriMock.getJevApiKey).toHaveBeenCalledWith("openrouter"));
    fireEvent.change(getModelSelect(), { target: { value: "d1" } });

    await waitFor(() => expect(tauriMock.setJevProvider).toHaveBeenCalledWith("liquid"));
    expect(getModelSelect()).toHaveValue("d1");
    expect(getProviderSelect()).toHaveValue("liquid");
    expect(Array.from((getProviderSelect() as HTMLSelectElement).options).map((option) => option.value)).toEqual([
      "liquid",
    ]);
    await waitFor(() => {
      expect(tauriMock.getJevApiKey).toHaveBeenCalledWith("liquid");
      expect(getApiKeyInput()).toHaveValue("liquid-key");
    });

    await act(async () => {
      oldOpenrouterKey.resolve("stale-openrouter-key");
      await oldOpenrouterKey.promise;
    });
    expect(getApiKeyInput()).toHaveValue("liquid-key");

    fireEvent.change(getApiKeyInput(), { target: { value: "liquid-new-key" } });
    await waitFor(() => {
      expect(tauriMock.setJevApiKey).toHaveBeenCalledWith("liquid", "liquid-new-key");
    });
    expect(tauriMock.setJevApiKey).not.toHaveBeenCalledWith("openrouter", "liquid-new-key");
    expect(tauriMock.setJevApiKey).not.toHaveBeenCalledWith("liquid", "stale-openrouter-key");
  });

  it("restores persisted Liquid d1 and returns to TypeSafe for the Jev model", async () => {
    tauriMock.getJevApiKey.mockImplementation((provider: JevProvider) => Promise.resolve(
      provider === "liquid" ? "liquid-key" : "typesafe-key",
    ));
    renderSection({
      ...baseProfile,
      jev: { enabled: true, provider: "liquid" },
    });

    await waitFor(() => expect(tauriMock.getJevApiKey).toHaveBeenCalledWith("liquid"));
    expect(getModelSelect()).toHaveValue("d1");
    expect(getProviderSelect()).toHaveValue("liquid");
    expect(Array.from((getProviderSelect() as HTMLSelectElement).options).map((option) => option.value)).toEqual([
      "liquid",
    ]);
    await waitFor(() => expect(getApiKeyInput()).toHaveValue("liquid-key"));

    fireEvent.change(getModelSelect(), { target: { value: "jev" } });

    await waitFor(() => expect(tauriMock.setJevProvider).toHaveBeenCalledWith("typesafe"));
    expect(getModelSelect()).toHaveValue("jev");
    expect(getProviderSelect()).toHaveValue("typesafe");
    expect(Array.from((getProviderSelect() as HTMLSelectElement).options).map((option) => option.value)).toEqual([
      "typesafe",
      "openrouter",
      "vercel",
    ]);
    await waitFor(() => expect(getApiKeyInput()).toHaveValue("typesafe-key"));
  });

  it("rolls back the model and provider when switching to Liquid fails", async () => {
    const onSaved = vi.fn();
    tauriMock.getJevApiKey.mockImplementation((provider: JevProvider) => Promise.resolve(
      provider === "openrouter" ? "openrouter-key" : "",
    ));
    tauriMock.setJevProvider.mockRejectedValueOnce(new Error("settings unavailable"));
    renderSection({
      ...baseProfile,
      jev: { enabled: true, provider: "openrouter" },
    }, onSaved);

    await waitFor(() => expect(getApiKeyInput()).toHaveValue("openrouter-key"));
    fireEvent.change(getModelSelect(), { target: { value: "d1" } });

    await waitFor(() => expect(toastMock.error).toHaveBeenCalledWith(expect.stringMatching(/decision|save/i)));
    expect(getModelSelect()).toHaveValue("jev");
    expect(getProviderSelect()).toHaveValue("openrouter");
    expect(Array.from((getProviderSelect() as HTMLSelectElement).options).map((option) => option.value)).toEqual([
      "typesafe",
      "openrouter",
      "vercel",
    ]);
    expect(onSaved).not.toHaveBeenCalled();
  });

  it("saves the selected provider and its key independently", async () => {
    const profile: JevProfile = {
      ...baseProfile,
      jev: { enabled: true, provider: "typesafe" },
    };
    tauriMock.getJevApiKey.mockImplementation((provider: JevProvider) => (
      provider === "typesafe" ? Promise.resolve("typesafe-key") : Promise.resolve("")
    ));
    renderSection(profile);

    await waitFor(() => expect(getApiKeyInput()).toHaveValue("typesafe-key"));
    fireEvent.change(getProviderSelect(), { target: { value: "openrouter" } });

    await waitFor(() => {
      expect(tauriMock.setJevProvider).toHaveBeenCalledWith("openrouter");
      expect(tauriMock.getJevApiKey).toHaveBeenCalledWith("openrouter");
    });
    await waitFor(() => expect(getApiKeyInput()).toHaveValue(""));

    fireEvent.change(getApiKeyInput(), { target: { value: "openrouter-key" } });
    await waitFor(() => {
      expect(tauriMock.setJevApiKey).toHaveBeenCalledWith("openrouter", "openrouter-key");
    });
    expect(tauriMock.setJevApiKey).not.toHaveBeenCalledWith("typesafe", "openrouter-key");
  });

  it("does not let a stale provider key response overwrite the current provider", async () => {
    const typesafe = deferred<string>();
    const openrouter = deferred<string>();
    tauriMock.getJevApiKey.mockImplementation((provider: JevProvider) => (
      provider === "typesafe" ? typesafe.promise : openrouter.promise
    ));
    renderSection({
      ...baseProfile,
      jev: { enabled: true, provider: "typesafe" },
    });

    await waitFor(() => expect(tauriMock.getJevApiKey).toHaveBeenCalledWith("typesafe"));
    fireEvent.change(getProviderSelect(), { target: { value: "openrouter" } });

    openrouter.resolve("openrouter-key");
    await waitFor(() => expect(getApiKeyInput()).toHaveValue("openrouter-key"));
    await act(async () => {
      typesafe.resolve("stale-typesafe-key");
      await typesafe.promise;
    });
    await waitFor(() => expect(getApiKeyInput()).toHaveValue("openrouter-key"));
  });

  it("does not let a late same-provider key read overwrite a user draft", async () => {
    const typesafe = deferred<string>();
    tauriMock.getJevApiKey.mockReturnValue(typesafe.promise);
    renderSection({
      ...baseProfile,
      jev: { enabled: true, provider: "typesafe" },
    });

    await waitFor(() => expect(tauriMock.getJevApiKey).toHaveBeenCalledWith("typesafe"));
    fireEvent.change(getApiKeyInput(), { target: { value: "new-typesafe-key" } });
    await act(async () => {
      typesafe.resolve("old-typesafe-key");
      await typesafe.promise;
    });

    expect(getApiKeyInput()).toHaveValue("new-typesafe-key");
    await waitFor(() => {
      expect(tauriMock.setJevApiKey).toHaveBeenCalledWith("typesafe", "new-typesafe-key");
    });
    expect(tauriMock.setJevApiKey).not.toHaveBeenCalledWith("typesafe", "old-typesafe-key");
  });

  it("flushes a pending key save under the provider that owned the draft", async () => {
    tauriMock.getJevApiKey.mockImplementation((provider: JevProvider) => (
      provider === "typesafe" ? Promise.resolve("typesafe-key") : Promise.resolve("")
    ));
    renderSection({
      ...baseProfile,
      jev: { enabled: true, provider: "typesafe" },
    });

    await waitFor(() => expect(getApiKeyInput()).toHaveValue("typesafe-key"));
    fireEvent.change(getApiKeyInput(), { target: { value: "draft-typesafe-key" } });
    fireEvent.change(getProviderSelect(), { target: { value: "openrouter" } });

    await waitFor(() => {
      expect(tauriMock.setJevApiKey).toHaveBeenCalledWith("typesafe", "draft-typesafe-key");
    });
    expect(tauriMock.setJevApiKey).not.toHaveBeenCalledWith("openrouter", "draft-typesafe-key");
  });

  it("shows a visible save error and does not report success when config saving fails", async () => {
    const onSaved = vi.fn();
    tauriMock.setJevFeatures.mockRejectedValueOnce(new Error("settings unavailable"));
    renderSection({ ...baseProfile, jev: undefined }, onSaved);

    fireEvent.click(getToggle());

    await waitFor(() => {
      expect(toastMock.error).toHaveBeenCalledWith(expect.stringMatching(/decision|save/i));
    });
    expect(onSaved).not.toHaveBeenCalled();
  });
});
