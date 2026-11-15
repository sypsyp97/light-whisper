import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useMicrophoneSettings } from "../useMicrophoneSettings";

const mocks = vi.hoisted(() => ({
  listen: vi.fn(), start: vi.fn(), stop: vi.fn(),
  devices: vi.fn(), select: vi.fn(), test: vi.fn(),
}));
const translate = (key: string) => key;
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: translate }) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@/api/tauri", () => ({
  listInputDevices: mocks.devices, setInputDevice: mocks.select,
  startMicrophoneLevelMonitor: mocks.start, stopMicrophoneLevelMonitor: mocks.stop,
  testMicrophone: mocks.test,
}));
vi.mock("@/lib/storage", () => ({ readLocalStorage: () => "true", writeLocalStorage: vi.fn() }));

beforeEach(() => {
  vi.resetAllMocks();
  mocks.devices.mockResolvedValue({ devices: [], selectedDeviceName: null });
  mocks.select.mockResolvedValue(undefined);
  mocks.stop.mockResolvedValue(undefined);
  mocks.start.mockResolvedValue("test microphone");
  mocks.listen.mockResolvedValue(vi.fn());
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

const closePicker = vi.fn();

async function renderMonitoringHook() {
  const hook = renderHook(({ active, isRecording }) => useMicrophoneSettings({
    active, isRecording, closePicker,
  }), { initialProps: { active: true, isRecording: false } });
  await act(async () => {});
  expect(hook.result.current.micMonitorReady).toBe(true);
  return hook;
}

describe("microphone monitor effect ownership", () => {
  it("does not restart monitoring when event registration completes after unmount", async () => {
    let resolve!: (unlisten: () => void) => void;
    mocks.listen.mockReturnValue(new Promise<() => void>((done) => { resolve = done; }));
    const unlisten = vi.fn();
    const hook = renderHook(() => useMicrophoneSettings({
      active: true, isRecording: false, closePicker: vi.fn(),
    }));
    hook.unmount();
    await act(async () => { resolve(unlisten); });
    expect(unlisten).toHaveBeenCalledOnce();
    expect(mocks.start).not.toHaveBeenCalled();
  });
});

describe("microphone asynchronous intent", () => {
  it.each(["disabled", "inactive", "recording", "unmounted"] as const)(
    "does not resume a completed microphone test after becoming %s",
    async (intent) => {
      const request = deferred<string>();
      mocks.test.mockReturnValue(request.promise);
      const hook = await renderMonitoringHook();
      let completion!: Promise<void>;
      await act(async () => { completion = hook.result.current.handleTestMicrophone(); });
      expect(mocks.test).toHaveBeenCalledOnce();

      await act(async () => {
        if (intent === "disabled") hook.result.current.handleMicLevelMonitorToggle(false);
        else if (intent === "inactive") hook.rerender({ active: false, isRecording: false });
        else if (intent === "recording") hook.rerender({ active: true, isRecording: true });
        else hook.unmount();
      });
      const starts = mocks.start.mock.calls.length;
      await act(async () => { request.resolve("test finished"); await completion; });

      expect(mocks.start).toHaveBeenCalledTimes(starts);
      if (intent !== "unmounted") expect(hook.result.current.micMonitorReady).toBe(false);
    },
  );

  it.each(["disabled", "inactive", "recording", "unmounted"] as const)(
    "stops an effect start that resolves after becoming %s",
    async (intent) => {
      const start = deferred<string>();
      let monitoring = false;
      mocks.start.mockImplementationOnce(async () => {
        const message = await start.promise;
        monitoring = true;
        return message;
      });
      mocks.stop.mockImplementation(async () => { monitoring = false; });
      const hook = renderHook(({ active, isRecording }) => useMicrophoneSettings({
        active, isRecording, closePicker,
      }), { initialProps: { active: true, isRecording: false } });
      await act(async () => {});
      expect(mocks.start).toHaveBeenCalledOnce();

      await act(async () => {
        if (intent === "disabled") hook.result.current.handleMicLevelMonitorToggle(false);
        else if (intent === "inactive") hook.rerender({ active: false, isRecording: false });
        else if (intent === "recording") hook.rerender({ active: true, isRecording: true });
        else hook.unmount();
      });
      await act(async () => { start.resolve("late start"); await start.promise; });

      expect(monitoring).toBe(false);
      if (intent !== "unmounted") expect(hook.result.current.micMonitorReady).toBe(false);
    },
  );

  it("allows only the newest overlapping microphone test to resume monitoring", async () => {
    const older = deferred<string>();
    const newer = deferred<string>();
    mocks.test.mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise);
    const hook = await renderMonitoringHook();
    let olderCompletion!: Promise<void>;
    let newerCompletion!: Promise<void>;
    await act(async () => {
      olderCompletion = hook.result.current.handleTestMicrophone();
      newerCompletion = hook.result.current.handleTestMicrophone();
    });
    expect(mocks.test).toHaveBeenCalledTimes(2);
    const starts = mocks.start.mock.calls.length;

    await act(async () => { older.resolve("older finished"); await olderCompletion; });
    expect(mocks.start).toHaveBeenCalledTimes(starts);
    expect(hook.result.current.micMonitorReady).toBe(false);

    await act(async () => { newer.resolve("newer finished"); await newerCompletion; });
    expect(mocks.start).toHaveBeenCalledTimes(starts + 1);
    expect(hook.result.current.micMonitorReady).toBe(true);
  });

  it.each(["success", "failure"] as const)("ignores an older test finishing with %s after the newest test", async (outcome) => {
    const older = deferred<string>();
    const newer = deferred<string>();
    mocks.test.mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise);
    const hook = await renderMonitoringHook();
    let olderCompletion!: Promise<void>;
    let newerCompletion!: Promise<void>;
    await act(async () => {
      olderCompletion = hook.result.current.handleTestMicrophone();
      newerCompletion = hook.result.current.handleTestMicrophone();
    });
    await act(async () => { newer.resolve("newer finished"); await newerCompletion; });
    expect(hook.result.current.micMonitorReady).toBe(true);
    const starts = mocks.start.mock.calls.length;

    await act(async () => {
      if (outcome === "success") older.resolve("older finished");
      else older.reject(new Error("older test failed"));
      await olderCompletion;
    });

    expect(mocks.start).toHaveBeenCalledTimes(starts);
    expect(hook.result.current.micMonitorReady).toBe(true);
  });

  it.each([true, false])("restores allowed monitor intent after a failed test (enabled: %s)", async (enabled) => {
    const request = deferred<string>();
    mocks.test.mockReturnValue(request.promise);
    const hook = await renderMonitoringHook();
    let completion!: Promise<void>;
    await act(async () => { completion = hook.result.current.handleTestMicrophone(); });
    if (!enabled) {
      await act(async () => { hook.result.current.handleMicLevelMonitorToggle(false); });
    }
    const starts = mocks.start.mock.calls.length;

    await act(async () => { request.reject(new Error("test failed")); await completion; });

    expect(mocks.start).toHaveBeenCalledTimes(starts + (enabled ? 1 : 0));
    expect(hook.result.current.micMonitorReady).toBe(enabled);
  });

  it("stops a test restart that resolves after monitoring is disabled", async () => {
    const start = deferred<string>();
    let monitoring = true;
    const hook = await renderMonitoringHook();
    mocks.test.mockResolvedValue("test finished");
    mocks.stop.mockImplementation(async () => { monitoring = false; });
    mocks.start.mockImplementationOnce(async () => {
      const message = await start.promise;
      monitoring = true;
      return message;
    });
    let completion!: Promise<void>;
    await act(async () => { completion = hook.result.current.handleTestMicrophone(); });
    expect(mocks.start).toHaveBeenCalledTimes(2);
    await act(async () => { hook.result.current.handleMicLevelMonitorToggle(false); });

    await act(async () => { start.resolve("late restart"); await completion; });

    expect(monitoring).toBe(false);
    expect(hook.result.current.micMonitorReady).toBe(false);
  });
});



describe("TLC microphone intent replay", () => {
  it.each(["microphone", "microphone-test-owner"])("replays %s through the real hook", async (name) => {
    const { ownershipTrace } = await import("@/test/asyncOwnershipTrace");
    const events = ownershipTrace(name);
    const listener = deferred<() => void>();
    mocks.listen.mockReturnValue(listener.promise);
    const start = deferred<string>();
    const tests = [deferred<string>(), deferred<string>()];
    mocks.test.mockReturnValueOnce(tests[0].promise).mockReturnValueOnce(tests[1].promise);
    let running = false;
    mocks.start.mockImplementation(async () => { await start.promise; running = true; return "started"; });
    mocks.stop.mockImplementation(async () => { running = false; });
    const hook = renderHook(({ active, isRecording }) => useMicrophoneSettings({ active, isRecording, closePicker }),
      { initialProps: { active: true, isRecording: false } });
    await act(async () => {});
    const completions: Promise<void>[] = [];
    let currentTest = 0;
    let invalidated = false;
    const handled: string[] = [];
    for (const event of events) {
      const parts = event.split(":");
      if (event === "mic:reconcile") {
        await act(async () => { listener.resolve(vi.fn()); await listener.promise; });
      } else if (parts[1] === "test" && parts[2] === "begin") {
        currentTest = Number(parts[3]);
        await act(async () => { completions[currentTest - 1] = hook.result.current.handleTestMicrophone(); });
      } else if (parts[1] === "test" && ["success", "failure"].includes(parts[2])) {
        const id = Number(parts[3]); const starts = mocks.start.mock.calls.length;
        await act(async () => {
          if (parts[2] === "success") tests[id - 1].resolve("test complete");
          else tests[id - 1].reject(new Error("microphone test failed"));
          await tests[id - 1].promise.catch(() => undefined);
        });
        if (id !== currentTest) expect(mocks.start).toHaveBeenCalledTimes(starts);
        else currentTest = 0;
      } else if (event === "mic:monitor:stop") {
        expect(mocks.stop).toHaveBeenCalled();
      } else if (parts[1] === "intent") {
        invalidated = true;
        await act(async () => {
          if (parts[2] === "recording") hook.rerender({ active: true, isRecording: true });
          else if (parts[2] === "inactive") hook.rerender({ active: false, isRecording: false });
          else if (parts[2] === "disabled") hook.result.current.handleMicLevelMonitorToggle(false);
          else if (parts[2] === "unmounted") hook.unmount();
          else throw new Error(`Unmapped TLC microphone intent: ${event}`);
        });
      } else if (event === "mic:monitor:start-complete") {
        await act(async () => { start.resolve("late start"); await start.promise; });
        if (invalidated || currentTest !== 0) expect(running).toBe(false);
      } else if (event === "mic:monitor:compensate") {
        expect(mocks.stop).toHaveBeenCalled();
      } else { throw new Error(`Unmapped TLC microphone event: ${event}`); }
      handled.push(event);
    }
    expect(handled).toEqual(events);
    if (currentTest !== 0 || invalidated) expect(hook.result.current.micMonitorReady).toBe(false);
    hook.unmount();
  });
});

describe("microphone monitor replacement ownership", () => {
  it.each(["success", "failure"] as const)("keeps the replacement monitor running after an old start finishes with %s", async (outcome) => {
    const oldStart = deferred<string>();
    let monitoring = false;
    mocks.stop.mockImplementation(async () => { monitoring = false; });
    mocks.start.mockImplementation(async () => {
      monitoring = true;
      return "replacement started";
    }).mockImplementationOnce(async () => {
      const message = await oldStart.promise;
      monitoring = true;
      return message;
    });
    const retired = renderHook(() => useMicrophoneSettings({
      active: true, isRecording: false, closePicker,
    }));
    await act(async () => {});
    expect(mocks.start).toHaveBeenCalledOnce();
    await act(async () => { retired.unmount(); });
    const replacement = renderHook(() => useMicrophoneSettings({
      active: true, isRecording: false, closePicker,
    }));
    await act(async () => {});


    await act(async () => {
      if (outcome === "success") oldStart.resolve("retired start completed");
      else oldStart.reject(new Error("retired start failed"));
      await oldStart.promise.catch(() => undefined);
    });
    await act(async () => {});

    expect(monitoring).toBe(true);
    expect(replacement.result.current.micMonitorReady).toBe(true);
  });

  it.each(["success", "failure"] as const)("keeps the replacement monitor running after a retired test finishes with %s", async (outcome) => {
    const oldTest = deferred<string>();
    let monitoring = false;
    mocks.stop.mockImplementation(async () => { monitoring = false; });
    mocks.start.mockImplementation(async () => {
      monitoring = true;
      return "monitor started";
    });
    mocks.test.mockReturnValue(oldTest.promise);
    const retired = await renderMonitoringHook();
    let completion!: Promise<void>;
    await act(async () => { completion = retired.result.current.handleTestMicrophone(); });
    expect(mocks.test).toHaveBeenCalledOnce();
    await act(async () => { retired.unmount(); });
    const replacement = await renderMonitoringHook();
    expect(monitoring).toBe(true);

    await act(async () => {
      if (outcome === "success") oldTest.resolve("retired test completed");
      else oldTest.reject(new Error("retired test failed"));
      await completion;
    });

    expect(monitoring).toBe(true);
    expect(replacement.result.current.micMonitorReady).toBe(true);
  });
});



describe("TLC microphone effect replacement replay", () => {
  it.each(["microphone-effect-owner", "microphone-retired-test"])("replays %s through two hook instances", async (name) => {
    const { ownershipTrace } = await import("@/test/asyncOwnershipTrace");
    const events = ownershipTrace(name);
    const first = deferred<string>(); const second = deferred<string>();
    let nativeOwner = "none";
    mocks.stop.mockImplementation(async () => { nativeOwner = "none"; });
    mocks.test.mockReturnValue(first.promise);
    let kind = "";
    let retired!: ReturnType<typeof renderMonitoringHook> extends Promise<infer T> ? T : never;
    let replacement!: typeof retired;
    let completion: Promise<void> | undefined;
    let handled = 0;
    for (const event of events) {
      if (event.startsWith("effect:first:begin:")) {
        kind = event.split(":")[3];
        mocks.start.mockImplementation(async () => { await second.promise; nativeOwner = "replacement"; return "new monitor"; })
          .mockImplementationOnce(async () => {
            if (kind === "start") await first.promise;
            nativeOwner = "retired"; return "old monitor";
          });
        retired = renderHook(() => useMicrophoneSettings({ active: true, isRecording: false, closePicker }));
        await act(async () => {});
        if (kind === "test") {
          await act(async () => { completion = retired.result.current.handleTestMicrophone(); });
        }
      } else if (event === "effect:first:retire") {
        await act(async () => { retired.unmount(); });
      } else if (event === "effect:second:mount") {
        replacement = renderHook(() => useMicrophoneSettings({ active: true, isRecording: false, closePicker }));
        await act(async () => {});
      } else if (event === "effect:second:start-complete") {


        await act(async () => { second.resolve("new monitor"); await second.promise; });
      } else if (event === "effect:first:complete") {
        await act(async () => { first.resolve("late retired completion"); await first.promise; if (completion) await completion; });
      } else if (event === "effect:first:cleanup") {
        await act(async () => {});
        expect(nativeOwner).toBe("replacement");
        expect(replacement.result.current.micMonitorReady).toBe(true);
      } else { throw new Error(`Unmapped TLC microphone effect event: ${event}`); }
      handled += 1;
    }
    expect(handled).toBe(events.length);
    replacement!.unmount();
    await act(async () => {});
  });
});
