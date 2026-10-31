import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import zh from "@/i18n/zh";

const api = vi.hoisted(() => ({
  getAudioProcessingEnabled: vi.fn<() => Promise<boolean>>(),
  setAudioProcessingEnabled: vi.fn<(enabled: boolean) => Promise<boolean>>(),
}));
const notifications = vi.hoisted(() => ({ error: vi.fn() }));

const messages: Record<string, string> = {
  "settings.audioProcessingTitle": "音频处理（RE-USE）",
  "settings.audioProcessingDesc": "Test audio enhancement description",
  "settings.audioProcessingLoadFailed": "Test audio enhancement read failure",
  "settings.audioProcessingSaveFailed": "Test audio enhancement save failure",
  "common.retry": zh.common.retry,
};

vi.mock("@/api/tauri", () => api);
vi.mock("sonner", () => ({ toast: notifications }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => messages[key] ?? key }),
}));

import AudioProcessingControl from "./AudioProcessingControl";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function getToggle() {
  return screen.getByRole("switch", { name: messages["settings.audioProcessingTitle"] });
}

describe("AudioProcessingControl", () => {
  beforeEach(() => {
    api.getAudioProcessingEnabled.mockReset().mockResolvedValue(false);
    api.setAudioProcessingEnabled.mockReset().mockImplementation(async (enabled) => enabled);
    notifications.error.mockReset();
  });

  it("starts off and disabled until its initial read completes without saving a default", async () => {
    const read = deferred<boolean>();
    api.getAudioProcessingEnabled.mockReturnValueOnce(read.promise);
    render(<AudioProcessingControl />);

    const toggle = getToggle();
    expect(toggle).toHaveClass("toggle-switch");
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(toggle).toBeDisabled();
    expect(screen.getByText(messages["settings.audioProcessingDesc"])).toBeInTheDocument();
    fireEvent.click(toggle);
    expect(api.setAudioProcessingEnabled).not.toHaveBeenCalled();

    await act(async () => { read.resolve(false); await read.promise; });

    expect(toggle).toBeEnabled();
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(api.setAudioProcessingEnabled).not.toHaveBeenCalled();
  });

  it("reflects an enabled setting returned by the initial read", async () => {
    api.getAudioProcessingEnabled.mockResolvedValue(true);
    render(<AudioProcessingControl />);

    await waitFor(() => expect(getToggle()).toBeEnabled());

    expect(getToggle()).toHaveAttribute("aria-checked", "true");
    expect(api.setAudioProcessingEnabled).not.toHaveBeenCalled();
  });

  it("keeps a failed read disabled and retries the read before accepting changes", async () => {
    const retryRead = deferred<boolean>();
    api.getAudioProcessingEnabled
      .mockRejectedValueOnce(new Error("initial read failed"))
      .mockReturnValueOnce(retryRead.promise);
    render(<AudioProcessingControl />);

    const retry = await screen.findByRole("button", { name: zh.common.retry });
    expect(screen.getByText(messages["settings.audioProcessingLoadFailed"])).toBeInTheDocument();
    expect(getToggle()).toBeDisabled();
    fireEvent.click(getToggle());
    expect(api.setAudioProcessingEnabled).not.toHaveBeenCalled();

    fireEvent.click(retry);
    await waitFor(() => expect(api.getAudioProcessingEnabled).toHaveBeenCalledTimes(2));
    expect(getToggle()).toBeDisabled();
    await act(async () => { retryRead.resolve(true); await retryRead.promise; });

    expect(getToggle()).toBeEnabled();
    expect(getToggle()).toHaveAttribute("aria-checked", "true");
    expect(api.setAudioProcessingEnabled).not.toHaveBeenCalled();
  });

  it("waits for each save, prevents duplicate clicks, and uses the backend's returned boolean", async () => {
    const declined = deferred<boolean>();
    const enabled = deferred<boolean>();
    const disabled = deferred<boolean>();
    api.setAudioProcessingEnabled
      .mockReturnValueOnce(declined.promise)
      .mockReturnValueOnce(enabled.promise)
      .mockReturnValueOnce(disabled.promise);
    render(<AudioProcessingControl />);
    await waitFor(() => expect(getToggle()).toBeEnabled());

    fireEvent.click(getToggle());
    expect(api.setAudioProcessingEnabled).toHaveBeenCalledExactlyOnceWith(true);
    expect(getToggle()).toBeDisabled();
    expect(getToggle()).toHaveAttribute("aria-checked", "false");
    fireEvent.click(getToggle());
    expect(api.setAudioProcessingEnabled).toHaveBeenCalledTimes(1);
    await act(async () => { declined.resolve(false); await declined.promise; });
    expect(getToggle()).toBeEnabled();
    expect(getToggle()).toHaveAttribute("aria-checked", "false");

    fireEvent.click(getToggle());
    expect(getToggle()).toHaveAttribute("aria-checked", "false");
    await act(async () => { enabled.resolve(true); await enabled.promise; });
    expect(getToggle()).toBeEnabled();
    expect(getToggle()).toHaveAttribute("aria-checked", "true");

    fireEvent.click(getToggle());
    expect(getToggle()).toBeDisabled();
    expect(getToggle()).toHaveAttribute("aria-checked", "true");
    await act(async () => { disabled.resolve(false); await disabled.promise; });
    expect(getToggle()).toBeEnabled();
    expect(getToggle()).toHaveAttribute("aria-checked", "false");
    expect(api.setAudioProcessingEnabled.mock.calls).toEqual([[true], [true], [false]]);
  });

  it.each([false, true])("preserves %s after a failed save, reports the error, and permits retry", async (enabled) => {
    api.getAudioProcessingEnabled.mockResolvedValue(enabled);
    api.setAudioProcessingEnabled.mockRejectedValueOnce(new Error("save failed"));
    render(<AudioProcessingControl />);
    await waitFor(() => expect(getToggle()).toBeEnabled());

    fireEvent.click(getToggle());
    await waitFor(() => expect(notifications.error).toHaveBeenCalledExactlyOnceWith(
      messages["settings.audioProcessingSaveFailed"],
    ));

    expect(getToggle()).toBeEnabled();
    expect(getToggle()).toHaveAttribute("aria-checked", String(enabled));
    expect(api.setAudioProcessingEnabled).toHaveBeenCalledExactlyOnceWith(!enabled);
    fireEvent.click(getToggle());
    await waitFor(() => expect(getToggle()).toHaveAttribute("aria-checked", String(!enabled)));
    expect(api.setAudioProcessingEnabled).toHaveBeenCalledTimes(2);
  });

  it("leaves a new mount untouched by a late read and handles an in-flight save after unmount", async () => {
    const lateRead = deferred<boolean>();
    const lateSave = deferred<boolean>();
    api.getAudioProcessingEnabled.mockReturnValueOnce(lateRead.promise);
    api.setAudioProcessingEnabled.mockReturnValueOnce(lateSave.promise);
    const first = render(<AudioProcessingControl />);
    first.unmount();

    const current = render(<AudioProcessingControl />);
    await waitFor(() => expect(getToggle()).toBeEnabled());
    await act(async () => { lateRead.resolve(true); await lateRead.promise; });
    expect(getToggle()).toHaveAttribute("aria-checked", "false");
    expect(api.setAudioProcessingEnabled).not.toHaveBeenCalled();

    fireEvent.click(getToggle());
    expect(api.setAudioProcessingEnabled).toHaveBeenCalledExactlyOnceWith(true);
    current.unmount();
    await act(async () => {
      lateSave.reject(new Error("save finished after unmount"));
      await lateSave.promise.catch(() => undefined);
    });

    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    expect(api.setAudioProcessingEnabled).toHaveBeenCalledTimes(1);
  });
});
