import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useAsrEngineSettings } from "../useAsrEngineSettings";

const api = vi.hoisted(() => ({ getEngine: vi.fn(), getOnlineAsrApiKey: vi.fn(), getOnlineAsrEndpoint: vi.fn(), getAlibabaAsrConfig: vi.fn(), listAlibabaAsrModels: vi.fn(), setAlibabaAsrModel: vi.fn(), setEngine: vi.fn(), setOnlineAsrApiKey: vi.fn(), setOnlineAsrEndpoint: vi.fn() }));
const errorToast = vi.hoisted(() => vi.fn());
vi.mock("@/api/tauri", () => api);
vi.mock("sonner", () => ({ toast: { error: errorToast, success: vi.fn() } }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
async function setup(engine = "glm-asr") {
  api.getEngine.mockResolvedValue(engine);
  const hook = renderHook(() => useAsrEngineSettings({ engineLabel: (value) => value, retryModel: vi.fn() }));
  await waitFor(() => expect(hook.result.current.engineLoading).toBe(false));
  return hook;
}
beforeEach(() => {
  vi.resetAllMocks();
  api.getOnlineAsrApiKey.mockResolvedValue("FAKE_OLD_KEY");
  api.getOnlineAsrEndpoint.mockResolvedValue({ region: "international", url: "https://old.example" });
  api.getAlibabaAsrConfig.mockResolvedValue({ model: "qwen3-asr-flash", models: [] });
  api.listAlibabaAsrModels.mockResolvedValue({ models: [], source: "fallback" });
  api.setEngine.mockResolvedValue(undefined);
  api.setOnlineAsrEndpoint.mockResolvedValue({ region: "domestic", url: "https://new.example" });
});
describe("online ASR credential ownership", () => {
  it("clears the previous provider key and reports a failed read after switching", async () => {
    const { result } = await setup();
    api.getOnlineAsrApiKey.mockRejectedValueOnce(new Error("keyring unavailable"));
    await act(async () => { await result.current.handleEngineSwitch("alibaba-asr"); });
    expect(api.getOnlineAsrApiKey).toHaveBeenCalledTimes(2);
    expect(result.current.engine).toBe("alibaba-asr");
    expect(result.current.onlineAsrApiKey).toBe("");
    expect(errorToast).toHaveBeenCalled();
    expect(api.setOnlineAsrApiKey).not.toHaveBeenCalled();
  });
  it("clears the previous region key if the new region read fails", async () => {
    const { result } = await setup("alibaba-asr");
    api.getOnlineAsrApiKey.mockRejectedValueOnce(new Error("keyring unavailable"));
    await act(async () => { await result.current.handleOnlineAsrRegionChange("domestic"); });
    expect(result.current.onlineAsrRegion).toBe("domestic");
    expect(result.current.onlineAsrApiKey).toBe("");
    expect(api.setOnlineAsrEndpoint).toHaveBeenCalledWith("domestic");
    expect(api.getOnlineAsrApiKey).toHaveBeenCalledTimes(2);
    expect(errorToast).toHaveBeenCalled();
  });
  it("ignores an old initial key read arriving after provider selection", async () => {
    const old = deferred<string>();
    api.getOnlineAsrApiKey.mockReturnValueOnce(old.promise);
    const { result } = await setup();
    api.getOnlineAsrApiKey.mockResolvedValueOnce("FAKE_NEW_KEY");
    await act(async () => { await result.current.handleEngineSwitch("alibaba-asr"); });
    await act(async () => { old.resolve("FAKE_OLD_KEY"); });
    expect(result.current.onlineAsrApiKey).toBe("FAKE_NEW_KEY");
  });
  it("keeps the existing key when the engine switch itself fails", async () => {
    const { result } = await setup();
    api.setEngine.mockRejectedValueOnce(new Error("switch failed"));
    await act(async () => { await result.current.handleEngineSwitch("alibaba-asr"); });
    expect(result.current.engine).toBe("glm-asr");
    expect(result.current.onlineAsrApiKey).toBe("FAKE_OLD_KEY");
  });
  it("does not let a late read replace a newer user key edit", async () => {
    const old = deferred<string>();
    api.getOnlineAsrApiKey.mockReturnValueOnce(old.promise);
    const { result, unmount } = await setup();
    act(() => { result.current.handleOnlineAsrApiKeyChange("FAKE_USER_EDIT"); });
    await act(async () => { old.resolve("FAKE_OLD_KEY"); });
    expect(result.current.onlineAsrApiKey).toBe("FAKE_USER_EDIT");
    unmount();
  });
  it("blocks region changes and key edits during an engine transition", async () => {
    const { result } = await setup("alibaba-asr");
    const switching = deferred<void>();
    api.setEngine.mockReturnValueOnce(switching.promise);
    let pending!: Promise<void>;
    act(() => { pending = result.current.handleEngineSwitch("glm-asr"); });
    await act(async () => { await Promise.resolve(); });
    await act(async () => { await result.current.handleOnlineAsrRegionChange("domestic"); });
    act(() => { result.current.handleOnlineAsrApiKeyChange("FAKE_DURING_SWITCH"); });
    expect(api.setOnlineAsrEndpoint).not.toHaveBeenCalled();
    expect(result.current.onlineAsrApiKey).toBe("FAKE_OLD_KEY");
    await act(async () => { switching.resolve(); await pending; });
    expect(api.setOnlineAsrApiKey).not.toHaveBeenCalled();
  });

  it("preserves the old region and key when the region mutation fails", async () => {
    const { result } = await setup("alibaba-asr");
    api.setOnlineAsrEndpoint.mockRejectedValueOnce(new Error("region unavailable"));
    await act(async () => { await result.current.handleOnlineAsrRegionChange("domestic"); });
    expect(api.setOnlineAsrEndpoint).toHaveBeenCalledWith("domestic");
    expect(api.getOnlineAsrApiKey).toHaveBeenCalledTimes(1);
    expect(result.current.onlineAsrRegion).toBe("international");
    expect(result.current.onlineAsrApiKey).toBe("FAKE_OLD_KEY");
    expect(errorToast).toHaveBeenCalledWith("toast.onlineAsrRegionSwitchFailed");
  });
  it.each(["engine", "region"])("flushes the original key before a %s mutation", async (kind) => {
    const { result } = await setup("alibaba-asr");
    const save = deferred<void>();
    api.setOnlineAsrApiKey.mockReturnValueOnce(save.promise);
    act(() => { result.current.handleOnlineAsrApiKeyChange("FAKE_EDIT"); });
    let pending!: Promise<void>;
    act(() => { pending = kind === "engine"
      ? result.current.handleEngineSwitch("glm-asr")
      : result.current.handleOnlineAsrRegionChange("domestic"); });
    expect(api.setOnlineAsrApiKey).toHaveBeenCalledWith("FAKE_EDIT", "alibaba-asr-intl-api-key");
    expect(api.setEngine).not.toHaveBeenCalled();
    expect(api.setOnlineAsrEndpoint).not.toHaveBeenCalled();
    await act(async () => { save.resolve(); await pending; });
    if (kind === "engine") expect(api.setEngine).toHaveBeenCalledWith("glm-asr");
    else expect(api.setOnlineAsrEndpoint).toHaveBeenCalledWith("domestic");
  });

  it("blocks engine changes and key edits during a region transition", async () => {
    const { result } = await setup("alibaba-asr");
    const switching = deferred<{region: string; url: string}>();
    api.setOnlineAsrEndpoint.mockReturnValueOnce(switching.promise);
    let pending!: Promise<void>;
    act(() => { pending = result.current.handleOnlineAsrRegionChange("domestic"); });
    await act(async () => { await Promise.resolve(); });
    await act(async () => { await result.current.handleEngineSwitch("glm-asr"); });
    act(() => { result.current.handleOnlineAsrApiKeyChange("FAKE_DURING_SWITCH"); });
    expect(api.setEngine).not.toHaveBeenCalled();
    expect(result.current.onlineAsrApiKey).toBe("FAKE_OLD_KEY");
    await act(async () => { switching.resolve({region: "domestic", url: "https://new.example"}); await pending; });
    expect(api.setOnlineAsrApiKey).not.toHaveBeenCalled();
  });

  it("still applies initial endpoint data when only the key draft changes", async () => {
    const endpoint = deferred<{region: string; url: string}>();
    api.getOnlineAsrEndpoint.mockReturnValueOnce(endpoint.promise);
    const { result, unmount } = await setup("alibaba-asr");
    act(() => { result.current.handleOnlineAsrApiKeyChange("FAKE_USER_EDIT"); });
    await act(async () => { endpoint.resolve({region: "domestic", url: "https://current.example"}); });
    expect(result.current.onlineAsrRegion).toBe("domestic");
    expect(result.current.onlineAsrApiKey).toBe("FAKE_USER_EDIT");
    unmount();
  });

});
