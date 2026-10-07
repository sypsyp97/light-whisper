import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import zh from "@/i18n/zh";

const api = vi.hoisted(() => ({
  getGpuIdleSeconds: vi.fn(),
  setGpuIdleSeconds: vi.fn(),
}));

vi.mock("@/api/tauri", () => api);
vi.mock("sonner", () => ({ toast: { error: vi.fn() } }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => {
      const [, name] = key.split(".");
      return zh.settings[name as keyof typeof zh.settings] as string;
    },
  }),
}));

import GpuIdleUnloadControl from "./GpuIdleUnloadControl";

describe("GpuIdleUnloadControl", () => {
  beforeEach(() => {
    api.getGpuIdleSeconds.mockReset();
    api.setGpuIdleSeconds.mockReset();
    api.getGpuIdleSeconds.mockResolvedValue(0);
    api.setGpuIdleSeconds.mockImplementation(async (seconds: number) => seconds);
  });

  it("starts off with the shared settings switch and no reload estimates", async () => {
    render(<GpuIdleUnloadControl />);
    const toggle = await screen.findByRole("switch", { name: zh.settings.gpuIdleTitle });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(toggle).toHaveClass("toggle-switch");
    expect(screen.getByText(zh.settings.gpuIdleDesc)).toBeInTheDocument();
    expect(screen.queryByRole("spinbutton")).not.toBeInTheDocument();
    expect(screen.queryByLabelText(zh.settings.gpuIdleSeconds)).not.toBeInTheDocument();
    expect(screen.queryByText(/冷启动|预热|4\.8|5\.5|RTX 4070/)).not.toBeInTheDocument();
  });

  it("turns on at the suggested 180 seconds with a plain text field and can be switched back off", async () => {
    render(<GpuIdleUnloadControl />);
    fireEvent.click(await screen.findByRole("switch", { name: zh.settings.gpuIdleTitle }));
    await waitFor(() => expect(api.setGpuIdleSeconds).toHaveBeenCalledWith(180));
    const input = await screen.findByLabelText(zh.settings.gpuIdleSeconds);
    expect(input).toHaveAttribute("type", "text");
    expect(input).toHaveValue("180");
    expect(screen.queryByRole("spinbutton")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("switch", { name: zh.settings.gpuIdleTitle }));
    await waitFor(() => expect(api.setGpuIdleSeconds).toHaveBeenLastCalledWith(0, 180));
    await waitFor(() => expect(screen.queryByRole("textbox", { name: zh.settings.gpuIdleSeconds })).not.toBeInTheDocument());
    // Keep the field in the document while its containing row collapses.
    expect(input).toBeInTheDocument();
    expect(input.closest("[aria-hidden]")).toHaveAttribute("aria-hidden", "true");
    expect(input.closest("[inert]")).not.toBeNull();
    await waitFor(() => expect(input).not.toBeInTheDocument());
  });

  it("turns off with one click while the timeout has unsaved edits", async () => {
    api.getGpuIdleSeconds.mockResolvedValue(180);
    const user = userEvent.setup();
    render(<GpuIdleUnloadControl />);
    const input = await screen.findByLabelText(zh.settings.gpuIdleSeconds);
    await user.clear(input);
    await user.type(input, "60");
    await user.click(screen.getByRole("switch"));
    await waitFor(() => expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "false"));
    expect(api.setGpuIdleSeconds).toHaveBeenCalledExactlyOnceWith(0, 60);
  });

  it("keeps an edited timeout when switched off and back on", async () => {
    api.getGpuIdleSeconds.mockResolvedValue(180);
    const user = userEvent.setup();
    render(<GpuIdleUnloadControl />);
    const input = await screen.findByLabelText(zh.settings.gpuIdleSeconds);
    await user.clear(input);
    await user.type(input, "60");
    await user.click(screen.getByRole("switch"));
    await waitFor(() => expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "false"));
    await user.click(screen.getByRole("switch"));
    await waitFor(() => expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "true"));
    expect(screen.getByLabelText(zh.settings.gpuIdleSeconds)).toHaveValue("60");
    expect(api.setGpuIdleSeconds).toHaveBeenLastCalledWith(60);
  });

  it("restores the saved timeout after reopening settings while disabled", async () => {
    api.getGpuIdleSeconds.mockImplementation(async (includeDisabled?: boolean) => includeDisabled ? 75 : 0);
    const user = userEvent.setup();
    render(<GpuIdleUnloadControl />);
    await waitFor(() => expect(api.getGpuIdleSeconds).toHaveBeenCalled());
    await user.click(screen.getByRole("switch"));
    await waitFor(() => expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "true"));
    expect(screen.getByLabelText(zh.settings.gpuIdleSeconds)).toHaveValue("75");
    expect(api.setGpuIdleSeconds).toHaveBeenLastCalledWith(75);
  });

  it("saves edited seconds without requiring blur or Enter", async () => {
    api.getGpuIdleSeconds.mockResolvedValue(180);
    const user = userEvent.setup();
    render(<GpuIdleUnloadControl />);
    const input = await screen.findByLabelText(zh.settings.gpuIdleSeconds);
    await user.clear(input);
    await user.type(input, "45");
    await waitFor(() => expect(api.setGpuIdleSeconds).toHaveBeenLastCalledWith(45));
    expect(input).toHaveValue("45");
  });

  it("keeps the last positive timeout when zero disables the timer", async () => {
    api.getGpuIdleSeconds.mockResolvedValue(60);
    const user = userEvent.setup();
    render(<GpuIdleUnloadControl />);
    const input = await screen.findByLabelText(zh.settings.gpuIdleSeconds);
    await user.clear(input);
    await user.type(input, "0{Enter}");
    await waitFor(() => expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "false"));
    expect(api.setGpuIdleSeconds).toHaveBeenLastCalledWith(0, 60);
    await user.click(screen.getByRole("switch"));
    await waitFor(() => expect(api.setGpuIdleSeconds).toHaveBeenLastCalledWith(60));
  });

  it("ignores an initial read that completes after a saved change", async () => {
    let finishRead!: (value: number) => void;
    api.getGpuIdleSeconds.mockReturnValue(new Promise<number>((resolve) => { finishRead = resolve; }));
    const user = userEvent.setup();
    render(<GpuIdleUnloadControl />);
    await user.click(screen.getByRole("switch"));
    await waitFor(() => expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "true"));
    await act(async () => { finishRead(0); });
    expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "true");
  });

  it("still saves a timeout on Enter and on blur to another control", async () => {
    api.getGpuIdleSeconds.mockResolvedValue(180);
    const user = userEvent.setup();
    render(<><GpuIdleUnloadControl /><button>Outside</button></>);
    const input = await screen.findByLabelText(zh.settings.gpuIdleSeconds);
    await user.clear(input);
    await user.type(input, "60{Enter}");
    await waitFor(() => expect(api.setGpuIdleSeconds).toHaveBeenLastCalledWith(60));
    await user.clear(input);
    await user.type(input, "90");
    await user.click(screen.getByRole("button", { name: "Outside" }));
    await waitFor(() => expect(api.setGpuIdleSeconds).toHaveBeenLastCalledWith(90));
  });

  it("saves a timeout when keyboard focus moves back to the switch", async () => {
    api.getGpuIdleSeconds.mockResolvedValue(180);
    const user = userEvent.setup();
    render(<GpuIdleUnloadControl />);
    const input = await screen.findByLabelText(zh.settings.gpuIdleSeconds);
    await user.clear(input);
    await user.type(input, "60");
    await user.tab({ shift: true });
    await waitFor(() => expect(api.setGpuIdleSeconds).toHaveBeenCalledExactlyOnceWith(60));
  });
});
