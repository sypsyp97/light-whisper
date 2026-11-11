import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AiModelListPayload, UserProfile } from "@/types";

const api = vi.hoisted(() => ({
  getSelectionApiKey: vi.fn(), listAiModels: vi.fn(), setSelectionApiKey: vi.fn(),
  setSelectionAssistantConfig: vi.fn(), testModelConnection: vi.fn(),
}));
vi.mock("@/api/tauri", () => api);
vi.mock("sonner", () => ({ toast: { error: vi.fn() } }));
vi.mock("react-i18next", () => {
  const t = (key: string) => key;
  return { useTranslation: () => ({ t }) };
});
import SelectionAssistantSettingsSection from "./SelectionAssistantSettingsSection";

const profile: UserProfile = {
  hot_words: [], correction_patterns: [], vocab_frequency: {}, total_transcriptions: 0, last_updated: 0,
  llm_provider: {
    active: "openai", custom_providers: [], selection_use_separate_model: true,
    selection_provider: "openai", selection_model: "selection-model",
  },
  selection_assistant: {
    enabled: true, auto_screenshot: false, translation_target: "English", excluded_apps: [],
  },
};
function deferred() {
  let resolve!: (value: AiModelListPayload) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<AiModelListPayload>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
async function renderSection() {
  await act(async () => {
    render(<SelectionAssistantSettingsSection profile={profile} openaiAuthMode="api_key"
      openaiOauthLoggedIn={false} openaiControls={null} onConfigureJev={vi.fn()} onSaved={vi.fn()} />);
  });
  fireEvent.click(screen.getByRole("button", { name: "settings.openAssistantModelList" }));
  await act(async () => { await vi.advanceTimersByTimeAsync(350); });
}
async function settle(request: ReturnType<typeof deferred>, outcome: "resolve" | "reject", id: string) {
  await act(async () => {
    if (outcome === "resolve") request.resolve({ models: [{ id }], sourceUrl: `https://${id}.example/models` });
    else request.reject(new Error(`${id} failed`));
  });
}
beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  api.getSelectionApiKey.mockResolvedValue("synthetic-selection-key");
  api.setSelectionApiKey.mockResolvedValue(undefined);
  api.setSelectionAssistantConfig.mockResolvedValue(undefined);
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: () => ({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() }),
  });
});
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); });

describe("selection catalog loading retirement", () => {
  it.each(["resolve", "reject"] as const)("settles after key deletion before retired catalog %s", async (outcome) => {
    const old = deferred();
    api.listAiModels.mockReturnValueOnce(old.promise);
    await renderSection();
    expect(api.listAiModels).toHaveBeenCalledOnce();
    expect(screen.getByRole("button", { name: "settings.fetching" })).toBeDisabled();

    fireEvent.change(screen.getByLabelText("settings.selectionApiKey"), { target: { value: "" } });

    expect.soft(screen.queryByRole("button", { name: "common.refresh" })).toBeEnabled();
    await settle(old, outcome, "retired-selection");
    expect.soft(screen.queryByRole("button", { name: "common.refresh" })).toBeEnabled();
    expect(screen.queryByText("retired-selection")).not.toBeInTheDocument();
    expect(screen.queryByText("retired-selection failed")).not.toBeInTheDocument();
    expect(api.listAiModels).toHaveBeenCalledOnce();
  });

  it.each(["resolve", "reject"] as const)("settles when separate mode closes before retired %s", async (outcome) => {
    const old = deferred();
    api.listAiModels.mockReturnValueOnce(old.promise);
    api.getSelectionApiKey.mockResolvedValueOnce("synthetic-selection-key").mockResolvedValue("");
    await renderSection();
    expect(screen.getByRole("button", { name: "settings.fetching" })).toBeDisabled();
    const separate = screen.getByRole("switch", { name: "settings.selectionSeparateConfig" });

    fireEvent.click(separate);
    expect(separate).toHaveAttribute("aria-checked", "false");
    await act(async () => { fireEvent.click(separate); });
    expect(separate).toHaveAttribute("aria-checked", "true");
    expect(screen.getByLabelText("settings.selectionApiKey")).toHaveValue("");

    expect.soft(screen.queryByRole("button", { name: "common.refresh" })).toBeEnabled();
    await settle(old, outcome, "retired-selection");
    expect.soft(screen.queryByRole("button", { name: "common.refresh" })).toBeEnabled();
    expect(screen.queryByText("retired-selection")).not.toBeInTheDocument();
    expect(api.listAiModels).toHaveBeenCalledOnce();
  });

  it.each([
    ["resolve", "resolve"], ["resolve", "reject"], ["reject", "resolve"], ["reject", "reject"],
  ] as const)("old %s cannot clear newer selection loading, whose %s settles", async (oldOutcome, currentOutcome) => {
    const old = deferred();
    const current = deferred();
    api.listAiModels.mockReturnValueOnce(old.promise).mockReturnValueOnce(current.promise);
    await renderSection();
    fireEvent.change(screen.getByLabelText("settings.selectionApiKey"), { target: { value: "new-selection-key" } });
    await act(async () => { await vi.advanceTimersByTimeAsync(350); });
    expect(api.listAiModels).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("button", { name: "settings.fetching" })).toBeDisabled();

    await settle(old, oldOutcome, "retired-selection");

    expect(screen.getByRole("button", { name: "settings.fetching" })).toBeDisabled();
    expect(screen.queryByText("retired-selection")).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "settings.searchAssistantModel" }), { target: { value: "current-selection" } });
    await settle(current, currentOutcome, "current-selection");
    expect(screen.getByRole("button", { name: "common.refresh" })).toBeEnabled();
    if (currentOutcome === "resolve") expect(screen.getByRole("option", { name: /current-selection/ })).toBeVisible();
    else expect(screen.getByText("current-selection failed")).toBeVisible();
  });
});
