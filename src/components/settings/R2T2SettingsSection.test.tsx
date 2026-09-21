import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import type { UserProfile } from "@/types";
const mocks = vi.hoisted(() => ({ save: vi.fn(), error: vi.fn(), success: vi.fn() }));
vi.mock("@/api/tauri", () => ({ setR2T2Config: mocks.save }));
vi.mock("sonner", () => ({ toast: { error: mocks.error, success: mocks.success } }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));
import R2T2SettingsSection from "./R2T2SettingsSection";

beforeEach(() => { vi.clearAllMocks(); mocks.save.mockResolvedValue(undefined); });

it("loads old profiles as automatic language and saves both settings together", async () => {
  const saved = vi.fn();
  render(<R2T2SettingsSection profile={{} as UserProfile} onSaved={saved} />);
  expect(screen.getByRole("combobox")).toHaveValue("");
  expect(screen.getAllByRole("option")).toHaveLength(31);
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "Rust meeting" } });
  fireEvent.change(screen.getByRole("combobox"), { target: { value: "de" } });
  fireEvent.click(screen.getByRole("button"));
  await waitFor(() => expect(saved).toHaveBeenCalledOnce());
  expect(mocks.save).toHaveBeenCalledWith("Rust meeting", "de");
});

it("retains the draft on failure and sends null when returning to auto", async () => {
  mocks.save.mockRejectedValueOnce(new Error("offline"));
  const saved = vi.fn();
  render(<R2T2SettingsSection profile={{ r2t2: { context: "old", language: "en" } } as UserProfile} onSaved={saved} />);
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "new" } });
  fireEvent.change(screen.getByRole("combobox"), { target: { value: "" } });
  fireEvent.click(screen.getByRole("button"));
  await waitFor(() => expect(mocks.error).toHaveBeenCalledOnce());
  expect(screen.getByRole("textbox")).toHaveValue("new");
  expect(saved).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button"));
  await waitFor(() => expect(saved).toHaveBeenCalledOnce());
  expect(mocks.save).toHaveBeenLastCalledWith("new", null);
});
