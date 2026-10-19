import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { JevConnection, JevProvider, UserProfile } from "@/types";

const tauriMock = vi.hoisted(() => ({
  getJevApiKey: vi.fn(),
  setJevApiKey: vi.fn(),
  setJevProvider: vi.fn(),
  setJevFeatures: vi.fn(),
  getJevConnection: vi.fn(),
  setJevConnection: vi.fn(),
  validateCorrections: vi.fn(),
}));

const toastMock = vi.hoisted(() => ({
  error: vi.fn(),
  success: vi.fn(),
}));

vi.mock("@/api/tauri", () => tauriMock);
vi.mock("sonner", () => ({ toast: toastMock }));

const labels: Record<string, string> = {
  "settings.jev": "Jev",
  "settings.jevEnabled": "Enable Jev gate",
  "settings.jevModel": "Decision model",
  "settings.jevProvider": "Decision provider",
  "settings.jevApiKey": "Decision API key",
  "settings.jevApiUrl": "Decision API URL",
  "settings.jevReviewNow": "Review now",
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

type JevProfile = UserProfile & {
  jev?: {
    enabled: boolean;
    provider: JevProvider;
    correction_review?: boolean;
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

function getModelInput() {
  return screen.getByRole("textbox", { name: "Decision model" });
}

function getUrlInput() {
  return screen.getByRole("textbox", { name: "Decision API URL" });
}

function getApiKeyInput() {
  return screen.getByLabelText("Decision API key");
}

const connections: Record<JevProvider, JevConnection> = {
  typesafe: { base_url: "https://api.typesafe.ai/v1/systemone", model: "jev-1.13.0" },
  openrouter: { base_url: "https://openrouter.ai/api/alpha/decisions", model: "~typesafe/jev-latest" },
  vercel: { base_url: "https://ai-gateway.vercel.sh/v4/ai/evaluation-model", model: "typesafe-ai/jev" },
  liquid: { base_url: "https://api.liquid.ai/decisions/v1/systemone", model: "d1:free" },
  custom: { base_url: "", model: "" },
};

async function expectConnection(provider: JevProvider) {
  await waitFor(() => {
    expect(getUrlInput()).toHaveValue(connections[provider].base_url);
    expect(getModelInput()).toHaveValue(connections[provider].model);
  });
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
  tauriMock.getJevConnection.mockReset();
  tauriMock.getJevConnection.mockImplementation((provider: JevProvider) => Promise.resolve({ ...connections[provider] }));
  tauriMock.setJevConnection.mockReset();
  tauriMock.setJevConnection.mockResolvedValue(undefined);
  tauriMock.validateCorrections.mockReset();
  tauriMock.validateCorrections.mockResolvedValue(0);
  toastMock.error.mockReset();
  toastMock.success.mockReset();
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

  it("defaults off and offers all five providers with editable backend connection defaults", async () => {
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
      "liquid",
      "custom",
    ]);
    await expectConnection("typesafe");
    expect(getModelInput()).not.toBeDisabled();
    expect(getUrlInput()).not.toBeDisabled();
    expect(getApiKeyInput()).toHaveAttribute("type", "password");
  });

  it("switches to Liquid and loads its isolated connection and credential without stale-key overwrite", async () => {
    const oldOpenrouterKey = deferred<string>();
    tauriMock.getJevApiKey.mockImplementation((provider: JevProvider) => (
      provider === "openrouter" ? oldOpenrouterKey.promise : Promise.resolve("liquid-key")
    ));
    renderSection({
      ...baseProfile,
      jev: { enabled: true, provider: "openrouter" },
    });

    await waitFor(() => expect(tauriMock.getJevApiKey).toHaveBeenCalledWith("openrouter"));
    fireEvent.change(getProviderSelect(), { target: { value: "liquid" } });

    await waitFor(() => expect(tauriMock.setJevProvider).toHaveBeenCalledWith("liquid"));
    await expectConnection("liquid");
    expect(getProviderSelect()).toHaveValue("liquid");
    expect(Array.from((getProviderSelect() as HTMLSelectElement).options).map((option) => option.value)).toEqual([
      "typesafe", "openrouter", "vercel", "liquid", "custom",
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

  it("restores persisted Liquid and changes provider independently of the free model input", async () => {
    tauriMock.getJevApiKey.mockImplementation((provider: JevProvider) => Promise.resolve(
      provider === "liquid" ? "liquid-key" : "typesafe-key",
    ));
    renderSection({
      ...baseProfile,
      jev: { enabled: true, provider: "liquid" },
    });

    await waitFor(() => expect(tauriMock.getJevApiKey).toHaveBeenCalledWith("liquid"));
    await expectConnection("liquid");
    expect(getProviderSelect()).toHaveValue("liquid");
    expect(Array.from((getProviderSelect() as HTMLSelectElement).options).map((option) => option.value)).toEqual([
      "typesafe", "openrouter", "vercel", "liquid", "custom",
    ]);
    await waitFor(() => expect(getApiKeyInput()).toHaveValue("liquid-key"));

    fireEvent.change(getModelInput(), { target: { value: "d1:future-preview" } });
    expect(getProviderSelect()).toHaveValue("liquid");
    expect(tauriMock.setJevProvider).not.toHaveBeenCalled();
    fireEvent.change(getProviderSelect(), { target: { value: "typesafe" } });

    await waitFor(() => expect(tauriMock.setJevProvider).toHaveBeenCalledWith("typesafe"));
    await expectConnection("typesafe");
    expect(getProviderSelect()).toHaveValue("typesafe");
    expect(Array.from((getProviderSelect() as HTMLSelectElement).options).map((option) => option.value)).toEqual([
      "typesafe",
      "openrouter",
      "vercel",
      "liquid",
      "custom",
    ]);
    await waitFor(() => expect(getApiKeyInput()).toHaveValue("typesafe-key"));
  });

  it("rolls back the connection and provider when switching to Liquid fails", async () => {
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
    await expectConnection("openrouter");
    fireEvent.change(getProviderSelect(), { target: { value: "liquid" } });

    await waitFor(() => expect(toastMock.error).toHaveBeenCalledWith(expect.stringMatching(/decision|save/i)));
    await expectConnection("openrouter");
    expect(getProviderSelect()).toHaveValue("openrouter");
    expect(Array.from((getProviderSelect() as HTMLSelectElement).options).map((option) => option.value)).toEqual([
      "typesafe",
      "openrouter",
      "vercel",
      "liquid",
      "custom",
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

  it("saves arbitrary model and URL together under the captured provider", async () => {
    renderSection({ ...baseProfile, jev: { enabled: true, provider: "vercel" } });
    await expectConnection("vercel");

    fireEvent.change(getModelInput(), { target: { value: "vendor/future-decision-preview" } });
    fireEvent.change(getUrlInput(), { target: { value: "https://decision.example/Case/Endpoint" } });

    await waitFor(() => expect(tauriMock.setJevConnection).toHaveBeenCalledWith(
      "vercel", "https://decision.example/Case/Endpoint", "vendor/future-decision-preview",
    ));
    expect(tauriMock.setJevProvider).not.toHaveBeenCalled();
    expect(tauriMock.setJevApiKey).not.toHaveBeenCalled();
  });

  it("offers Custom as an independent provider with an empty editable connection", async () => {
    renderSection({ ...baseProfile, jev: { enabled: true, provider: "typesafe" } });
    await expectConnection("typesafe");
    fireEvent.change(getProviderSelect(), { target: { value: "custom" } });
    await waitFor(() => expect(tauriMock.setJevProvider).toHaveBeenCalledWith("custom"));
    await expectConnection("custom");

    fireEvent.change(getModelInput(), { target: { value: "local-decider" } });
    fireEvent.change(getUrlInput(), { target: { value: "http://127.0.0.1:8123/v1" } });
    fireEvent.change(getApiKeyInput(), { target: { value: "custom-test-key" } });
    await waitFor(() => {
      expect(tauriMock.setJevConnection).toHaveBeenCalledWith("custom", "http://127.0.0.1:8123/v1", "local-decider");
      expect(tauriMock.setJevApiKey).toHaveBeenCalledWith("custom", "custom-test-key");
    });
    expect(tauriMock.setJevApiKey).not.toHaveBeenCalledWith("typesafe", "custom-test-key");
  });

  it("ignores a connection read from the previous provider", async () => {
    const stale = deferred<JevConnection>();
    tauriMock.getJevConnection.mockImplementation((provider: JevProvider) => (
      provider === "typesafe" ? stale.promise : Promise.resolve(connections[provider])
    ));
    renderSection({ ...baseProfile, jev: { enabled: true, provider: "typesafe" } });
    await waitFor(() => expect(tauriMock.getJevConnection).toHaveBeenCalledWith("typesafe"));
    fireEvent.change(getProviderSelect(), { target: { value: "openrouter" } });
    await expectConnection("openrouter");
    await act(async () => {
      stale.resolve({ base_url: "https://stale.example/v1", model: "stale-model" });
      await stale.promise;
    });
    await expectConnection("openrouter");
    expect(tauriMock.setJevConnection).not.toHaveBeenCalled();
  });

  it("does not replace an edited connection with a late read for the same provider", async () => {
    const stale = deferred<JevConnection>();
    tauriMock.getJevConnection.mockReturnValue(stale.promise);
    renderSection({ ...baseProfile, jev: { enabled: true, provider: "typesafe" } });
    await waitFor(() => expect(tauriMock.getJevConnection).toHaveBeenCalled());
    fireEvent.change(getUrlInput(), { target: { value: "https://draft.example/v1" } });
    fireEvent.change(getModelInput(), { target: { value: "draft-model" } });
    await act(async () => {
      stale.resolve(connections.typesafe);
      await stale.promise;
    });

    expect(getUrlInput()).toHaveValue("https://draft.example/v1");
    expect(getModelInput()).toHaveValue("draft-model");
    await waitFor(() => expect(tauriMock.setJevConnection).toHaveBeenCalledWith(
      "typesafe", "https://draft.example/v1", "draft-model",
    ));
  });

  it("preserves an edited connection and key when a stale profile is refreshed", async () => {
    const profile = { ...baseProfile, jev: { enabled: true, provider: "typesafe" as const } };
    tauriMock.getJevApiKey.mockResolvedValue("saved-key");
    const view = render(<JevSettingsSection profile={profile} onSaved={vi.fn()} />);
    await expectConnection("typesafe");
    await waitFor(() => expect(getApiKeyInput()).toHaveValue("saved-key"));
    fireEvent.change(getModelInput(), { target: { value: "unsaved-model" } });
    fireEvent.change(getUrlInput(), { target: { value: "https://unsaved.example/decision" } });
    fireEvent.change(getApiKeyInput(), { target: { value: "unsaved-key" } });

    view.rerender(<JevSettingsSection profile={{ ...profile, last_updated: 99,
      jev: { ...profile.jev, connections: { typesafe: connections.typesafe } },
    }} onSaved={vi.fn()} />);

    expect(getModelInput()).toHaveValue("unsaved-model");
    expect(getUrlInput()).toHaveValue("https://unsaved.example/decision");
    expect(getApiKeyInput()).toHaveValue("unsaved-key");
  });

  it("keeps edits when a stale profile names the previous provider after a local switch", async () => {
    const original = { ...baseProfile, jev: { enabled: true, provider: "typesafe" as const } };
    const view = render(<JevSettingsSection profile={original} onSaved={vi.fn()} />);
    await expectConnection("typesafe");
    fireEvent.change(getProviderSelect(), { target: { value: "openrouter" } });
    await expectConnection("openrouter");
    fireEvent.change(getModelInput(), { target: { value: "openrouter-draft" } });
    view.rerender(<JevSettingsSection profile={{ ...original, last_updated: 1 }} onSaved={vi.fn()} />);
    expect(getProviderSelect()).toHaveValue("openrouter");
    expect(getModelInput()).toHaveValue("openrouter-draft");
  });

  it("keeps the latest provider and Liquid drafts when an intermediate provider profile refresh arrives late", async () => {
    const initial = { ...baseProfile, jev: { enabled: true, provider: "typesafe" as const } };
    const staleRefresh = deferred<JevProfile>();
    let refreshFinished: Promise<void> | undefined;
    const onSaved = vi.fn(() => {
      refreshFinished ??= staleRefresh.promise.then((profile) => {
        view.rerender(<JevSettingsSection profile={profile} onSaved={onSaved} />);
      });
    });
    tauriMock.getJevApiKey.mockImplementation((provider: JevProvider) => Promise.resolve(`${provider}-saved-key`));
    const view = render(<JevSettingsSection profile={initial} onSaved={onSaved} />);
    await expectConnection("typesafe");

    fireEvent.change(getProviderSelect(), { target: { value: "openrouter" } });
    await expectConnection("openrouter");
    await waitFor(() => expect(getProviderSelect()).not.toBeDisabled());
    expect(onSaved).toHaveBeenCalledTimes(1);
    fireEvent.change(getProviderSelect(), { target: { value: "liquid" } });
    await expectConnection("liquid");
    await waitFor(() => {
      expect(getProviderSelect()).not.toBeDisabled();
      expect(getApiKeyInput()).toHaveValue("liquid-saved-key");
    });
    fireEvent.change(getModelInput(), { target: { value: "liquid-latest-model-draft" } });
    fireEvent.change(getUrlInput(), { target: { value: "https://liquid-draft.example/Decision" } });
    fireEvent.change(getApiKeyInput(), { target: { value: "liquid-latest-key-draft" } });

    await act(async () => {
      staleRefresh.resolve({ ...initial, last_updated: 1, jev: { enabled: true, provider: "openrouter" } });
      await refreshFinished;
    });

    expect(getProviderSelect()).toHaveValue("liquid");
    expect(getModelInput()).toHaveValue("liquid-latest-model-draft");
    expect(getUrlInput()).toHaveValue("https://liquid-draft.example/Decision");
    expect(getApiKeyInput()).toHaveValue("liquid-latest-key-draft");
    await waitFor(() => {
      expect(tauriMock.setJevConnection).toHaveBeenCalledWith("liquid", "https://liquid-draft.example/Decision", "liquid-latest-model-draft");
      expect(tauriMock.setJevApiKey).toHaveBeenCalledWith("liquid", "liquid-latest-key-draft");
    });
    expect(tauriMock.setJevConnection).not.toHaveBeenCalledWith("openrouter", "https://liquid-draft.example/Decision", "liquid-latest-model-draft");
    expect(tauriMock.setJevApiKey).not.toHaveBeenCalledWith("openrouter", "liquid-latest-key-draft");
  });

  it("retains each provider's edited connection across switches while reads are stale", async () => {
    renderSection({ ...baseProfile, jev: { enabled: true, provider: "typesafe" } });
    await expectConnection("typesafe");
    fireEvent.change(getModelInput(), { target: { value: "typesafe-local-draft" } });
    fireEvent.change(getProviderSelect(), { target: { value: "liquid" } });
    await expectConnection("liquid");
    fireEvent.change(getModelInput(), { target: { value: "liquid-local-draft" } });
    fireEvent.change(getProviderSelect(), { target: { value: "typesafe" } });
    await waitFor(() => {
      expect(getProviderSelect()).toHaveValue("typesafe");
      expect(getModelInput()).toHaveValue("typesafe-local-draft");
    });
    fireEvent.change(getProviderSelect(), { target: { value: "liquid" } });
    await waitFor(() => expect(getModelInput()).toHaveValue("liquid-local-draft"));
    expect(tauriMock.setJevConnection).toHaveBeenCalledWith("typesafe", connections.typesafe.base_url, "typesafe-local-draft");
    expect(tauriMock.setJevConnection).toHaveBeenCalledWith("liquid", connections.liquid.base_url, "liquid-local-draft");
  });

  it.each(["key", "connection"] as const)("shows a failed %s read while independently loading the other fields", async (kind) => {
    if (kind === "key") tauriMock.getJevApiKey.mockRejectedValue(new Error("read denied"));
    else tauriMock.getJevConnection.mockRejectedValue(new Error("read denied"));
    tauriMock.getJevApiKey.mockImplementation(() => kind === "key"
      ? Promise.reject(new Error("read denied")) : Promise.resolve("available-key"));
    renderSection({ ...baseProfile, jev: { enabled: true, provider: "typesafe" } });
    await waitFor(() => expect(screen.getByRole("alert")).toBeInTheDocument());
    if (kind === "key") await expectConnection("typesafe");
    else await waitFor(() => expect(getApiKeyInput()).toHaveValue("available-key"));
    expect(tauriMock.setJevConnection).not.toHaveBeenCalled();
    expect(tauriMock.setJevApiKey).not.toHaveBeenCalled();
  });

  it("waits for both captured connection and key saves before switching provider", async () => {
    const keySave = deferred<void>();
    const connectionSave = deferred<void>();
    tauriMock.setJevApiKey.mockReturnValue(keySave.promise);
    tauriMock.setJevConnection.mockReturnValue(connectionSave.promise);
    renderSection({ ...baseProfile, jev: { enabled: true, provider: "typesafe" } });
    await expectConnection("typesafe");
    fireEvent.change(getApiKeyInput(), { target: { value: "draft-key" } });
    fireEvent.change(getModelInput(), { target: { value: "draft-model" } });
    fireEvent.change(getProviderSelect(), { target: { value: "openrouter" } });
    await waitFor(() => expect(tauriMock.setJevApiKey).toHaveBeenCalledWith("typesafe", "draft-key"));
    expect(tauriMock.setJevProvider).not.toHaveBeenCalled();
    await act(async () => keySave.resolve());
    await waitFor(() => expect(tauriMock.setJevConnection).toHaveBeenCalledWith(
      "typesafe", connections.typesafe.base_url, "draft-model",
    ));
    expect(tauriMock.setJevProvider).not.toHaveBeenCalled();
    await act(async () => connectionSave.resolve());
    await waitFor(() => expect(tauriMock.setJevProvider).toHaveBeenCalledWith("openrouter"));
    expect(tauriMock.setJevConnection.mock.calls.every(([provider]) => provider === "typesafe")).toBe(true);
  });

  it("flushes connection and key before a manual correction review", async () => {
    const keySave = deferred<void>();
    const connectionSave = deferred<void>();
    tauriMock.setJevApiKey.mockReturnValue(keySave.promise);
    tauriMock.setJevConnection.mockReturnValue(connectionSave.promise);
    renderSection({ ...baseProfile, jev: { enabled: false, provider: "liquid", correction_review: true } });
    await expectConnection("liquid");
    fireEvent.change(getApiKeyInput(), { target: { value: "review-key" } });
    fireEvent.change(getModelInput(), { target: { value: "review-model" } });
    fireEvent.click(screen.getByRole("button", { name: "Review now" }));
    await waitFor(() => expect(tauriMock.setJevApiKey).toHaveBeenCalledWith("liquid", "review-key"));
    expect(tauriMock.validateCorrections).not.toHaveBeenCalled();
    await act(async () => keySave.resolve());
    await waitFor(() => expect(tauriMock.setJevConnection).toHaveBeenCalledWith(
      "liquid", connections.liquid.base_url, "review-model",
    ));
    expect(tauriMock.validateCorrections).not.toHaveBeenCalled();
    await act(async () => connectionSave.resolve());
    await waitFor(() => expect(tauriMock.validateCorrections).toHaveBeenCalledTimes(1));
  });

  it.each(["disable", "unmount"] as const)("flushes connection and key on %s", async (action) => {
    const view = render(<JevSettingsSection profile={{ ...baseProfile,
      jev: { enabled: false, provider: "typesafe", correction_review: true },
    }} onSaved={vi.fn()} />);
    await expectConnection("typesafe");
    fireEvent.change(getApiKeyInput(), { target: { value: "final-key" } });
    fireEvent.change(getModelInput(), { target: { value: "final-model" } });
    if (action === "disable") fireEvent.click(getToggle());
    else view.unmount();
    await waitFor(() => {
      expect(tauriMock.setJevApiKey).toHaveBeenCalledWith("typesafe", "final-key");
      expect(tauriMock.setJevConnection).toHaveBeenCalledWith(
        "typesafe", connections.typesafe.base_url, "final-model",
      );
    });
  });

  it.each(["key", "connection"] as const)("shows a failed %s save and prevents review using unsaved settings", async (kind) => {
    const onSaved = renderSection({ ...baseProfile,
      jev: { enabled: false, provider: "typesafe", correction_review: true },
    });
    await expectConnection("typesafe");
    if (kind === "key") {
      tauriMock.setJevApiKey.mockRejectedValue(new Error("save denied"));
      fireEvent.change(getApiKeyInput(), { target: { value: "failed-key" } });
    } else {
      tauriMock.setJevConnection.mockRejectedValue(new Error("save denied"));
      fireEvent.change(getModelInput(), { target: { value: "failed-model" } });
    }
    fireEvent.click(screen.getByRole("button", { name: "Review now" }));
    await waitFor(() => expect(toastMock.error).toHaveBeenCalled());
    expect(tauriMock.validateCorrections).not.toHaveBeenCalled();
    expect(onSaved).not.toHaveBeenCalled();
  });

  it("keeps the selected provider and draft when flushing a connection save fails", async () => {
    const onSaved = renderSection({ ...baseProfile, jev: { enabled: true, provider: "typesafe" } });
    await expectConnection("typesafe");
    tauriMock.setJevConnection.mockRejectedValue(new Error("save denied"));
    fireEvent.change(getModelInput(), { target: { value: "retry-this-model" } });
    fireEvent.change(getProviderSelect(), { target: { value: "custom" } });
    await waitFor(() => expect(toastMock.error).toHaveBeenCalled());
    expect(tauriMock.setJevProvider).not.toHaveBeenCalled();
    expect(getProviderSelect()).toHaveValue("typesafe");
    expect(getModelInput()).toHaveValue("retry-this-model");
    expect(onSaved).not.toHaveBeenCalled();
  });

  describe("decision connection draft races", () => {
    const retryCases = (["key", "connection"] as const).flatMap((kind) => (
      (["completed", "in-flight"] as const).flatMap((failure) => (
        (["switch", "review"] as const).map((action) => ({ kind, failure, action }))
      ))
    ));

    it.each(retryCases)("keeps newest $kind draft after $failure failure before $action", async ({ kind, failure, action }) => {
      vi.useFakeTimers();
      let view: ReturnType<typeof render> | undefined;
      let rejectOlder!: (reason: Error) => void;
      const olderSave = new Promise<void>((_resolve, reject) => { rejectOlder = reject; });
      const older = `older-${kind}-A`;
      const latest = `latest-${kind}-B`;
      let storedValue = kind === "key" ? "initial-key" : connections.typesafe.model;
      let reviewedValue: string | undefined;
      let firstAttempt = true;
      const save = kind === "key" ? tauriMock.setJevApiKey : tauriMock.setJevConnection;
      save.mockImplementation((...args: string[]) => {
        const value = args[args.length - 1];
        if (value === older) {
          if (firstAttempt) { firstAttempt = false; return olderSave; }
          return Promise.reject(new Error("older draft rejected"));
        }
        storedValue = value;
        return Promise.resolve(undefined);
      });
      tauriMock.getJevApiKey.mockImplementation((provider: JevProvider) => Promise.resolve(
        kind === "key" && provider === "typesafe" ? storedValue : "",
      ));
      tauriMock.validateCorrections.mockImplementation(() => {
        reviewedValue = storedValue;
        return Promise.resolve(0);
      });
      const edit = (value: string) => fireEvent.change(
        kind === "key" ? getApiKeyInput() : getModelInput(), { target: { value } },
      );

      try {
        await act(async () => {
          view = render(<JevSettingsSection profile={{ ...baseProfile,
            jev: { enabled: false, provider: "typesafe", correction_review: true },
          }} onSaved={vi.fn()} />);
        });
        expect(getModelInput()).toHaveValue(connections.typesafe.model);
        edit(older);
        await act(async () => { await vi.advanceTimersByTimeAsync(600); });
        expect(save.mock.lastCall?.[kind === "key" ? 1 : 2]).toBe(older);
        if (failure === "in-flight") edit(latest);
        await act(async () => {
          rejectOlder(new Error("older draft rejected"));
          await olderSave.catch(() => undefined);
        });
        expect(toastMock.error).toHaveBeenCalled();
        if (failure === "completed") edit(latest);
        await act(async () => {
          if (action === "switch") fireEvent.change(getProviderSelect(), { target: { value: "openrouter" } });
          else fireEvent.click(screen.getByRole("button", { name: "Review now" }));
        });

        expect(save.mock.lastCall?.[kind === "key" ? 1 : 2]).toBe(latest);
        expect(storedValue).toBe(latest);
        const latestArgs = kind === "key"
          ? ["typesafe", latest] : ["typesafe", connections.typesafe.base_url, latest];
        expect(save).toHaveBeenLastCalledWith(...latestArgs);
        expect(save.mock.calls.filter((args) => args[args.length - 1] === older)).toHaveLength(1);
        if (action === "switch") {
          expect(tauriMock.setJevProvider).toHaveBeenCalledWith("openrouter");
          expect(getProviderSelect()).toHaveValue("openrouter");
        } else {
          expect(tauriMock.validateCorrections).toHaveBeenCalledTimes(1);
          expect(reviewedValue).toBe(latest);
          expect(kind === "key" ? getApiKeyInput() : getModelInput()).toHaveValue(latest);
        }
      } finally {
        await act(async () => { view?.unmount(); });
        vi.clearAllTimers();
        vi.useRealTimers();
      }
    });

    it.each(["model", "base_url"] as const)("merges a single edited %s with the unread stored connection before saving", async (field) => {
      vi.useFakeTimers();
      const read = deferred<JevConnection>();
      const stored = { base_url: "https://stored.example/Decision", model: "stored-future-model" };
      const value = field === "model" ? "latest-model-draft" : "https://latest.example/Decision";
      const merged = { ...stored, [field]: value };
      tauriMock.getJevConnection.mockReturnValue(read.promise);
      let view: ReturnType<typeof render> | undefined;
      try {
        await act(async () => {
          view = render(<JevSettingsSection profile={{ ...baseProfile,
            jev: { enabled: true, provider: "typesafe" },
          }} onSaved={vi.fn()} />);
        });
        expect(tauriMock.getJevConnection).toHaveBeenCalledWith("typesafe");
        fireEvent.change(field === "model" ? getModelInput() : getUrlInput(), { target: { value } });
        await act(async () => { await vi.advanceTimersByTimeAsync(600); });
        expect(tauriMock.setJevConnection).not.toHaveBeenCalled();

        await act(async () => { read.resolve(stored); await read.promise; });
        expect(getModelInput()).toHaveValue(merged.model);
        expect(getUrlInput()).toHaveValue(merged.base_url);
        await act(async () => { await vi.runOnlyPendingTimersAsync(); });
        expect(tauriMock.setJevConnection).toHaveBeenCalledWith("typesafe", merged.base_url, merged.model);
        expect(tauriMock.setJevConnection.mock.calls.every(([, baseUrl, model]) => Boolean(baseUrl && model))).toBe(true);
      } finally {
        read.resolve(stored);
        await act(async () => { await read.promise; view?.unmount(); });
        vi.clearAllTimers();
        vi.useRealTimers();
      }
    });

    it("waits for the unread connection to merge and save before manual review", async () => {
      const read = deferred<JevConnection>();
      const save = deferred<void>();
      const stored = { base_url: "https://stored.example/Decision", model: "stored-model" };
      tauriMock.getJevConnection.mockReturnValue(read.promise);
      tauriMock.setJevConnection.mockReturnValue(save.promise);
      const view = render(<JevSettingsSection profile={{ ...baseProfile,
        jev: { enabled: false, provider: "typesafe", correction_review: true },
      }} onSaved={vi.fn()} />);
      try {
        await waitFor(() => expect(tauriMock.getJevConnection).toHaveBeenCalledWith("typesafe"));
        fireEvent.change(getModelInput(), { target: { value: "review-latest-model" } });
        await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Review now" })); });
        expect(tauriMock.validateCorrections).not.toHaveBeenCalled();
        expect(tauriMock.setJevConnection).not.toHaveBeenCalled();

        await act(async () => { read.resolve(stored); await read.promise; });
        await waitFor(() => expect(tauriMock.setJevConnection).toHaveBeenCalledWith(
          "typesafe", stored.base_url, "review-latest-model",
        ));
        expect(getUrlInput()).toHaveValue(stored.base_url);
        expect(getModelInput()).toHaveValue("review-latest-model");
        expect(tauriMock.validateCorrections).not.toHaveBeenCalled();
        await act(async () => { save.resolve(); await save.promise; });
        if (tauriMock.validateCorrections.mock.calls.length === 0) {
          fireEvent.click(screen.getByRole("button", { name: "Review now" }));
        }
        await waitFor(() => expect(tauriMock.validateCorrections).toHaveBeenCalledTimes(1));
      } finally {
        read.resolve(stored);
        save.resolve();
        await act(async () => { await Promise.all([read.promise, save.promise]); view.unmount(); });
      }
    });
  });

  it("preserves a hidden failed key retry across disable and re-enable without deleting the credential", async () => {
    vi.useFakeTimers();
    let view: ReturnType<typeof render> | undefined;
    let keyringAvailable = false;
    let storedKey = "previous-saved-key";
    const draft = "retry-key-A";
    tauriMock.getJevApiKey.mockImplementation(() => Promise.resolve(storedKey));
    tauriMock.setJevApiKey.mockImplementation((_provider: JevProvider, value: string) => {
      if (!keyringAvailable) return Promise.reject(new Error("keyring temporarily unavailable"));
      storedKey = value;
      return Promise.resolve(undefined);
    });
    try {
      await act(async () => {
        view = render(<JevSettingsSection profile={{ ...baseProfile,
          jev: { enabled: false, provider: "typesafe", correction_review: true },
        }} onSaved={vi.fn()} />);
      });
      expect(getApiKeyInput()).toHaveValue("previous-saved-key");
      fireEvent.change(getApiKeyInput(), { target: { value: draft } });
      await act(async () => { await vi.advanceTimersByTimeAsync(600); });
      expect(tauriMock.setJevApiKey).toHaveBeenCalledTimes(1);
      expect(toastMock.error).toHaveBeenCalled();

      await act(async () => { fireEvent.click(getToggle()); });
      expect(getToggle()).toHaveAttribute("aria-checked", "false");
      expect(tauriMock.setJevApiKey).toHaveBeenCalledTimes(2);
      expect(tauriMock.setJevApiKey.mock.calls.every(([, key]) => key === draft)).toBe(true);

      keyringAvailable = true;
      await act(async () => { fireEvent.click(getToggle()); });
      expect(getToggle()).toHaveAttribute("aria-checked", "true");
      expect(tauriMock.setJevApiKey).toHaveBeenLastCalledWith("typesafe", draft);
      expect(tauriMock.setJevApiKey).not.toHaveBeenCalledWith("typesafe", "");
      expect(storedKey).toBe(draft);
      expect(tauriMock.getJevApiKey).toHaveBeenCalledTimes(2);
      expect(getApiKeyInput()).toHaveValue(draft);
    } finally {
      await act(async () => { view?.unmount(); });
      vi.clearAllTimers();
      vi.useRealTimers();
    }
  });
});
