import { useRef, useState } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ModelConnectionRequest } from "@/api/tauri";
import type { UserProfile } from "@/types";

const api = vi.hoisted(() => ({
  removeCorrection: vi.fn(),
  setCorrectionValidationConfig: vi.fn(),
  validateCorrections: vi.fn(),
  testModelConnection: vi.fn(),
}));
vi.mock("@/api/tauri", () => api);
vi.mock("sonner", () => ({ toast: { error: vi.fn() } }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

import CorrectionRulesModal from "./CorrectionRulesModal";

const polishConnection: ModelConnectionRequest = {
  target: "polish", provider: "openai", baseUrl: "https://polish.example/v1",
  model: "draft-polish-model", apiKey: "polish-only-key", reasoningMode: "balanced",
  openaiAuthMode: "api_key", xaiAuthMode: "api_key",
};
const profile: UserProfile = {
  hot_words: [], correction_patterns: [], vocab_frequency: {}, total_transcriptions: 0, last_updated: 0,
  llm_provider: {
    active: "openai",
    custom_providers: [{ id: "review-test", name: "Review Test", base_url: "https://review.example/v1", model: "stored-review-model", api_format: "openai_compat" }],
  },
};

function Harness({ separate = false }: { separate?: boolean }) {
  const [provider, setProvider] = useState<string | null>(null);
  const [model, setModel] = useState("");
  const returnFocusRef = useRef<HTMLButtonElement>(null);
  return <CorrectionRulesModal profile={profile} polishConnection={polishConnection}
    allProviderOptions={[{ key: "review-test", label: "Review Test" }]}
    validationEnabled setValidationEnabled={vi.fn()}
    validationUseSeparateModel={separate} setValidationUseSeparateModel={vi.fn()}
    validationProvider={provider} setValidationProvider={setProvider}
    validationModel={model} setValidationModel={setModel}
    validationRunning={false} setValidationRunning={vi.fn()}
    validationResult={null} setValidationResult={vi.fn()}
    returnFocusRef={returnFocusRef} onClose={vi.fn()} onRefreshProfile={vi.fn()} />;
}

beforeEach(() => {
  vi.clearAllMocks();
  api.setCorrectionValidationConfig.mockResolvedValue(undefined);
  api.testModelConnection.mockResolvedValue({ model: "tested-review-model", elapsedMs: 12 });
});

describe("CorrectionRulesModal connection test", () => {
  it("uses the supplied current polish draft without running correction validation", async () => {
    render(<Harness />);
    expect(api.testModelConnection).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "settings.testModelConnection" }));

    await waitFor(() => expect(api.testModelConnection).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
      ...polishConnection, target: "validation",
    })));
    expect(api.validateCorrections).not.toHaveBeenCalled();
  });

  it("uses separate provider and unsaved model drafts without forwarding another provider's key", async () => {
    render(<Harness separate />);
    fireEvent.change(screen.getByRole("combobox", { name: "settings.provider" }), { target: { value: "review-test" } });
    fireEvent.change(screen.getByRole("textbox", { name: "settings.modelLabel" }), { target: { value: "draft-review-model" } });
    expect(api.testModelConnection).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "settings.testModelConnection" }));

    await waitFor(() => expect(api.testModelConnection).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
      target: "validation", provider: "review-test", baseUrl: "https://review.example/v1", model: "draft-review-model",
    })));
    expect(api.testModelConnection.mock.calls[0][0].apiKey).toBeUndefined();
    expect(api.validateCorrections).not.toHaveBeenCalled();
  });
});
