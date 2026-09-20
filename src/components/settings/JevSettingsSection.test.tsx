import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UserProfile } from "@/types";

const tauriMock = vi.hoisted(() => ({
  getJevApiKey: vi.fn(),
  setJevApiKey: vi.fn(),
  setJevConfig: vi.fn(),
}));

const toastMock = vi.hoisted(() => ({
  error: vi.fn(),
}));

vi.mock("@/api/tauri", () => tauriMock);
vi.mock("sonner", () => ({ toast: toastMock }));

const labels: Record<string, string> = {
  "settings.jev": "Jev",
  "settings.jevEnabled": "Enable Jev gate",
  "settings.jevProvider": "Jev provider",
  "settings.jevApiKey": "Jev API key",
  "settings.jevSaveFailed": "Jev settings save failed",
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

type JevProvider = "typesafe" | "openrouter" | "vercel";
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
  return screen.getByRole("switch", { name: /jev/i });
}

function getProviderSelect() {
  return screen.getByRole("combobox", { name: /jev.*provider|provider.*jev/i });
}

function getApiKeyInput() {
  return screen.getByLabelText(/jev.*api|api.*key/i);
}

beforeEach(() => {
  tauriMock.getJevApiKey.mockReset();
  tauriMock.getJevApiKey.mockResolvedValue("");
  tauriMock.setJevApiKey.mockReset();
  tauriMock.setJevApiKey.mockResolvedValue(undefined);
  tauriMock.setJevConfig.mockReset();
  tauriMock.setJevConfig.mockResolvedValue(undefined);
  toastMock.error.mockReset();
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("JevSettingsSection", () => {
  it("defaults off and expands exactly three providers with a password key input", async () => {
    const onSaved = renderSection({ ...baseProfile, jev: undefined });

    expect(getToggle()).toHaveAttribute("aria-checked", "false");
    expect(screen.queryByRole("combobox", { name: /jev.*provider|provider.*jev/i })).not.toBeInTheDocument();

    fireEvent.click(getToggle());

    await waitFor(() => {
      expect(tauriMock.setJevConfig).toHaveBeenCalledWith(true, "typesafe");
    });
    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));

    const providerSelect = getProviderSelect() as HTMLSelectElement;
    expect(Array.from(providerSelect.options).map((option) => option.value)).toEqual([
      "typesafe",
      "openrouter",
      "vercel",
    ]);
    expect(getApiKeyInput()).toHaveAttribute("type", "password");
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
      expect(tauriMock.setJevConfig).toHaveBeenCalledWith(true, "openrouter");
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
    tauriMock.setJevConfig.mockRejectedValueOnce(new Error("settings unavailable"));
    renderSection({ ...baseProfile, jev: undefined }, onSaved);

    fireEvent.click(getToggle());

    await waitFor(() => {
      expect(toastMock.error).toHaveBeenCalledWith(expect.stringMatching(/jev|save/i));
    });
    expect(onSaved).not.toHaveBeenCalled();
  });
});
