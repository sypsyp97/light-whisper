import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import ScreenVisionModelPicker from "./ScreenVisionModelPicker";

const listAiModels = vi.hoisted(() => vi.fn());
const testModelConnection = vi.hoisted(() => vi.fn());
const translate = (key: string) => key;
vi.mock("@/api/tauri", () => ({ listAiModels, testModelConnection }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: translate }) }));
vi.mock("./ScreenVisionAuth", () => ({ default: () => null }));
afterEach(() => { vi.clearAllMocks(); vi.useRealTimers(); });

it("tests the current vision draft props only after a click", async () => {
  vi.useFakeTimers();
  listAiModels.mockResolvedValue({ models: [], sourceUrl: "" });
  testModelConnection.mockResolvedValue({ model: "draft-vision-model", elapsedMs: 12 });
  const props = {
    model: "saved-vision-model", provider: "openai", baseUrl: "https://saved.example/v1", apiKey: "saved-vision-key",
    loggedIn: false, authIdentity: "test", openaiAuthMode: "api_key" as const, xaiAuthMode: "api_key" as const,
    providerOptions: [{ key: "openai", label: "OpenAI" }], onProviderChange: vi.fn(), onApiKeyChange: vi.fn(),
    onChange: vi.fn(), onBlur: vi.fn(), onSelect: vi.fn(),
  };
  const view = render(<ScreenVisionModelPicker {...props} />);
  view.rerender(<ScreenVisionModelPicker {...props} model="draft-vision-model" baseUrl="https://draft.example/v1" apiKey="draft-vision-key" />);
  expect(testModelConnection).not.toHaveBeenCalled();

  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "settings.testModelConnection" })); });

  expect(testModelConnection).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
    target: "vision", provider: "openai", baseUrl: "https://draft.example/v1", model: "draft-vision-model",
    apiKey: "draft-vision-key", openaiAuthMode: "api_key", xaiAuthMode: "api_key",
  }));
});

it("shows a refresh warning while cached vision models remain selectable", async () => {
  vi.useFakeTimers();
  listAiModels.mockResolvedValue({ models: [{ id: "cached-vision", ownedBy: "openai" }], sourceUrl: "https://example.test/models", warning: "Vision catalog refresh unavailable" });
  const onSelect = vi.fn();
  render(<ScreenVisionModelPicker model="cached-vision" provider="openai" baseUrl="" apiKey="synthetic-key" loggedIn={false}
    authIdentity="test" providerOptions={[{ key: "openai", label: "OpenAI" }]}
    onProviderChange={vi.fn()} onApiKeyChange={vi.fn()} onChange={vi.fn()} onBlur={vi.fn()} onSelect={onSelect} />);
  await act(async () => { await vi.advanceTimersByTimeAsync(700); });
  expect(screen.getByText("Vision catalog refresh unavailable")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "settings.openModelList" }));
  fireEvent.click(screen.getByRole("option", { name: /cached-vision/ }));
  expect(onSelect).toHaveBeenCalledWith("cached-vision");
});
