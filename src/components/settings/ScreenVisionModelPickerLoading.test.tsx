import { useState } from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AiModelListPayload } from "@/types";

const api = vi.hoisted(() => ({ listAiModels: vi.fn(), testModelConnection: vi.fn() }));
vi.mock("@/api/tauri", () => api);
vi.mock("react-i18next", () => {
  const t = (key: string) => key;
  return { useTranslation: () => ({ t }) };
});
import ScreenVisionModelPicker from "./ScreenVisionModelPicker";

function deferred() {
  let resolve!: (value: AiModelListPayload) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<AiModelListPayload>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
function VisionPicker({ oauth = false }: { oauth?: boolean }) {
  const [key, setKey] = useState(oauth ? "" : "synthetic-key");
  const [loggedIn, setLoggedIn] = useState(oauth);
  return <>
    <button onClick={() => setLoggedIn(false)}>Sign out test account</button>
    <ScreenVisionModelPicker model="vision-model" provider="openai" baseUrl="" apiKey={key}
      loggedIn={loggedIn} authIdentity={loggedIn ? "signed-in" : "signed-out"}
      openaiAuthMode={oauth ? "oauth" : "api_key"}
      providerOptions={[{ key: "openai", label: "OpenAI" }]}
      onProviderChange={vi.fn()} onApiKeyChange={setKey}
      onChange={vi.fn()} onBlur={vi.fn()} onSelect={vi.fn()} />
  </>;
}
async function settle(request: ReturnType<typeof deferred>, outcome: "resolve" | "reject") {
  await act(async () => {
    if (outcome === "resolve") request.resolve({ models: [{ id: "retired-vision" }], sourceUrl: "https://old.example/models" });
    else request.reject(new Error("Retired vision request failed"));
  });
}
beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: () => ({ matches: true, addEventListener: vi.fn(), removeEventListener: vi.fn() }),
  });
});
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); });

describe("vision catalog loading retirement", () => {
  it.each([
    ["key deletion", "resolve"], ["key deletion", "reject"],
    ["OAuth sign out", "resolve"], ["OAuth sign out", "reject"],
  ] as const)("settles a pending catalog on %s before retired %s", async (retirement, outcome) => {
    const old = deferred();
    api.listAiModels.mockReturnValueOnce(old.promise);
    render(<VisionPicker oauth={retirement === "OAuth sign out"} />);
    fireEvent.click(screen.getByRole("button", { name: "settings.openModelList" }));
    await act(async () => { await vi.advanceTimersByTimeAsync(700); });
    expect(api.listAiModels).toHaveBeenCalledOnce();
    expect(screen.getByRole("button", { name: "settings.fetching" })).toBeDisabled();

    if (retirement === "key deletion") {
      fireEvent.change(screen.getByLabelText("settings.screenVisionApiKey"), { target: { value: "" } });
    } else {
      fireEvent.click(screen.getByRole("button", { name: "Sign out test account" }));
    }
    await act(async () => { await vi.advanceTimersByTimeAsync(700); });

    expect.soft(screen.queryByRole("button", { name: "common.refresh" })).toBeEnabled();
    expect(api.listAiModels).toHaveBeenCalledOnce();
    await settle(old, outcome);
    expect.soft(screen.queryByRole("button", { name: "common.refresh" })).toBeEnabled();
    expect(screen.queryByText("retired-vision")).not.toBeInTheDocument();
    expect(screen.queryByText("Retired vision request failed")).not.toBeInTheDocument();
  });

  it.each(["resolve", "reject"] as const)("finishes the current vision catalog on %s", async (outcome) => {
    const current = deferred();
    api.listAiModels.mockReturnValueOnce(current.promise);
    render(<VisionPicker />);
    fireEvent.click(screen.getByRole("button", { name: "settings.openModelList" }));
    await act(async () => { await vi.advanceTimersByTimeAsync(700); });
    expect(screen.getByRole("button", { name: "settings.fetching" })).toBeDisabled();
    await settle(current, outcome);
    expect(screen.getByRole("button", { name: "common.refresh" })).toBeEnabled();
    if (outcome === "resolve") expect(screen.getByRole("option", { name: /retired-vision/ })).toBeVisible();
    else expect(screen.getAllByText("Retired vision request failed").length).toBeGreaterThan(0);
  });
});
