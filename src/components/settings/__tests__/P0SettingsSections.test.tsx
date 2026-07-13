import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { UserProfile } from "@/types";

const api = vi.hoisted(() => ({
  setAppProfileRules: vi.fn(),
  setHistorySettings: vi.fn(),
}));

vi.mock("@/api/tauri", () => api);
vi.mock("sonner", () => ({ toast: { error: vi.fn() } }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

import AppProfileRulesSettingsSection from "@/components/settings/AppProfileRulesSettingsSection";
import HistorySettingsSection from "@/components/settings/HistorySettingsSection";

beforeEach(() => {
  vi.clearAllMocks();
  api.setAppProfileRules.mockResolvedValue(undefined);
  api.setHistorySettings.mockResolvedValue(undefined);
});

describe("P0 settings sections", () => {
  it("keeps history opt-in when no saved setting exists", () => {
    render(<HistorySettingsSection profile={null} onSaved={vi.fn()} />);

    expect(screen.getByRole("switch", { name: "settings.historyEnabled" }))
      .toHaveAttribute("aria-checked", "false");
    expect(screen.getByRole("switch", { name: "settings.historySaveAudio" }))
      .toBeDisabled();
  });

  it("disabling history also disables audio retention", async () => {
    const profile = {
      history_settings: { enabled: true, save_audio: true, retention_days: 90 },
    } as UserProfile;
    render(<HistorySettingsSection profile={profile} onSaved={vi.fn()} />);

    fireEvent.click(screen.getByRole("switch", { name: "settings.historyEnabled" }));

    await waitFor(() => {
      expect(api.setHistorySettings).toHaveBeenCalledWith(false, false, 90);
    });
  });

  it("creates an ordered per-app rule with explicit overrides", async () => {
    vi.spyOn(crypto, "randomUUID").mockReturnValue("00000000-0000-4000-8000-000000000001");
    const profile = { app_profile_rules: [] } as unknown as UserProfile;
    render(<AppProfileRulesSettingsSection profile={profile} onSaved={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "settings.appRuleAdd" }));
    fireEvent.change(screen.getByLabelText("settings.appRuleProcess"), {
      target: { value: "Code.exe" },
    });
    fireEvent.change(screen.getByLabelText("settings.appRuleName"), {
      target: { value: "Editor" },
    });
    fireEvent.click(screen.getByRole("button", { name: "settings.appRuleSave" }));

    await waitFor(() => {
      expect(api.setAppProfileRules).toHaveBeenCalledWith([
        expect.objectContaining({
          id: "00000000-0000-4000-8000-000000000001",
          name: "Editor",
          process_name: "Code.exe",
          enabled: true,
          ai_polish: "inherit",
          translation: "inherit",
          screen_context: "inherit",
          history: "inherit",
        }),
      ]);
    });
  });
});
