import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import ScreenVisionModelPicker from "./ScreenVisionModelPicker";

const listAiModels = vi.hoisted(() => vi.fn());
const translate = (key: string) => key;
vi.mock("@/api/tauri", () => ({ listAiModels }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: translate }) }));
vi.mock("./ScreenVisionAuth", () => ({ default: () => null }));
afterEach(() => { vi.useRealTimers(); });

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
