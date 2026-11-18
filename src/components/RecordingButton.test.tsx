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

  events.listen.mockResolvedValueOnce(disposer);
  const late = render(<RecordingButton isStarting={false} isRecording isProcessing={false} isReady onToggle={vi.fn()} />);
  late.unmount();
  await act(async () => { await Promise.resolve(); });
  expect(disposer).toHaveBeenCalledOnce();
});

it.each([true, false])("keeps the processing circle disabled at ASR readiness %s", (isReady) => {
  const onToggle = vi.fn();
  const view = render(
    <RecordingButton isStarting={false} isRecording={false} isProcessing isReady={isReady} onToggle={onToggle} />,
  );
  const button = view.getByRole("button", { name: "recording.processing" });

  expect(button).toBeDisabled();
  fireEvent.click(button);

  expect(onToggle).not.toHaveBeenCalled();
});

it("keeps one start ring through waveform updates and removes it when recording stops", async () => {
  const props = { isStarting: false, isRecording: false, isProcessing: false, isReady: true, onToggle: vi.fn() };
  const view = render(<RecordingButton {...props} />);
  const rings = () => [...view.container.querySelectorAll(".recording-pulse-ring, .recording-pulse-ring-outer")];
  expect(rings()).toHaveLength(0);
  view.rerender(<RecordingButton {...props} isRecording />);
  const started = rings();
  expect.soft(started).toHaveLength(1);
  await act(async () => events.emit("waveform", { sessionId: 4, bars: [1, 0, 1, 0, 1] }));
  expect(rings()).toEqual(started);

  view.rerender(<RecordingButton {...props} isProcessing />);
  expect(rings()).toHaveLength(0);
  view.rerender(<RecordingButton {...props} isRecording />);
  expect.soft(rings()).toHaveLength(1);
  expect(rings()[0]).not.toBe(started[0]);
});
