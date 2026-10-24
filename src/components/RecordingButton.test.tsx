import { act, fireEvent, render } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
const events = await vi.hoisted(async () => {
  const { createTauriEventController } = await import("@/test/tauriEventMock");
  return createTauriEventController();
});
vi.mock("@tauri-apps/api/event", () => events.module);
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key === "recording.useOriginal" ? "Use original" : key }),
}));
import RecordingButton from "./RecordingButton";

afterEach(() => events.reset());

it("renders live centered waveform bars, ignores older sessions, and disposes its subscription", async () => {
  const view = render(<RecordingButton isStarting={false} isRecording isProcessing={false} isReady onToggle={vi.fn()} />);
  await act(async () => { await Promise.resolve(); });
  act(() => {
    events.emit("waveform", { sessionId: 2, bars: [0.1, 0.2, 0.3, 0.4, 0.5] });
  });
  const heights = () => [...view.container.querySelectorAll<HTMLElement>(".eq-bar")].map((bar) => bar.style.transform);
  expect(new Set(heights()).size).toBeGreaterThan(1);
  const previous = heights();
  act(() => events.emit("waveform", { sessionId: 1, bars: [1, 1, 1, 1, 1] }));
  expect(heights()).toEqual(previous);
  expect(view.container.querySelectorAll(".record-icon")).toHaveLength(3);
  const disposer = vi.fn<() => void>();
  // A second mount covers a subscription that resolves after unmount.
  events.listen.mockResolvedValueOnce(disposer);
  const late = render(<RecordingButton isStarting={false} isRecording isProcessing={false} isReady onToggle={vi.fn()} />);
  late.unmount();
  await act(async () => { await Promise.resolve(); });
  expect(disposer).toHaveBeenCalledOnce();
});

it.each([true, false])("offers use-original cancellation while processing with ASR readiness %s", (isReady) => {
  const onToggle = vi.fn();
  const onCancelProcessing = vi.fn();
  const view = render(
    <RecordingButton isStarting={false} isRecording={false} isProcessing isReady={isReady}
      onToggle={onToggle} onCancelProcessing={onCancelProcessing} />,
  );
  const button = view.getByRole("button", { name: "Use original" });

  expect(button).toBeEnabled();
  fireEvent.click(button);

  expect(onCancelProcessing).toHaveBeenCalledOnce();
  expect(onToggle).not.toHaveBeenCalled();
});

it("keeps processing disabled when no cancellation callback exists", () => {
  const onToggle = vi.fn();
  const view = render(
    <RecordingButton isStarting={false} isRecording={false} isProcessing isReady onToggle={onToggle} />,
  );
  const button = view.getByRole("button", { name: "recording.processing" });

  expect(button).toBeDisabled();
  fireEvent.click(button);

  expect(onToggle).not.toHaveBeenCalled();
});
