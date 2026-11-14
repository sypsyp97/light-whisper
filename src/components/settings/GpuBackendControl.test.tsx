import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  getGpuBackend: vi.fn<() => Promise<string>>(),
  setGpuBackend: vi.fn<(backend: string) => Promise<string>>(),
  listGpuBackends: vi.fn<() => Promise<string[]>>(),
}));
const notifications = vi.hoisted(() => ({ error: vi.fn() }));

const messages: Record<string, string> = {
  "settings.gpuBackendTitle": "GPU backend",
  "settings.gpuBackendDesc": "Pick the backend",
  "settings.gpuBackendAuto": "Automatic",
  "settings.gpuBackendAutoDesc": "auto description",
  "settings.gpuBackendCuda": "NVIDIA (CUDA)",
  "settings.gpuBackendCudaDesc": "cuda description",
  "settings.gpuBackendVulkan": "AMD (Vulkan)",
  "settings.gpuBackendVulkanDesc": "vulkan description",
  "settings.gpuBackendCpu": "CPU",
  "settings.gpuBackendCpuDesc": "cpu description",
  "settings.gpuBackendUnavailable": "not included in this build",
  "settings.gpuBackendSaveFailed": "save failed",
  "settings.gpuBackendRestart": "applies on next start",
};

vi.mock("@/api/tauri", () => api);
vi.mock("sonner", () => ({ toast: notifications }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => messages[key] ?? key }),
}));

import GpuBackendControl from "./GpuBackendControl";

const option = (label: string) => screen.getByRole("button", { name: new RegExp(label) });

describe("GpuBackendControl", () => {
  beforeEach(() => {
    api.getGpuBackend.mockReset().mockResolvedValue("auto");
    api.listGpuBackends.mockReset().mockResolvedValue(["cpu", "vulkan"]);
    api.setGpuBackend.mockReset().mockImplementation(async (value) => value);
    notifications.error.mockReset();
  });

  it("loads the stored backend and disables packages this build does not include", async () => {
    render(<GpuBackendControl />);
    await waitFor(() => expect(option("Automatic")).toHaveAttribute("aria-pressed", "true"));
    expect(option("NVIDIA")).toBeDisabled();
    expect(option("AMD")).not.toBeDisabled();
    expect(option("CPU")).not.toBeDisabled();
  });

  it("saves the selected backend", async () => {
    render(<GpuBackendControl />);
    await waitFor(() => expect(api.listGpuBackends).toHaveBeenCalled());
    fireEvent.click(option("AMD"));
    await waitFor(() => expect(api.setGpuBackend).toHaveBeenCalledWith("vulkan"));
    expect(option("AMD")).toHaveAttribute("aria-pressed", "true");
  });

  it("keeps the previous value and reports a failed save", async () => {
    api.setGpuBackend.mockRejectedValueOnce(new Error("nope"));
    render(<GpuBackendControl />);
    await waitFor(() => expect(api.getGpuBackend).toHaveBeenCalled());
    fireEvent.click(option("CPU"));
    await waitFor(() => expect(notifications.error).toHaveBeenCalled());
    expect(option("Automatic")).toHaveAttribute("aria-pressed", "true");
  });
});
