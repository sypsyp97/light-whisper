import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const events = vi.hoisted(() => {
  type Handler = (event: { payload: unknown }) => void;
  const handlers = new Map<string, Set<Handler>>();
  return {
    handlers,
    listen: async (name: string, handler: Handler) => {
      let set = handlers.get(name);
      if (!set) { set = new Set(); handlers.set(name, set); }
      set.add(handler);
      return () => set.delete(handler);
    },
    emit: (name: string, payload: unknown) => {
      for (const handler of handlers.get(name) ?? []) handler({ payload });
    },
  };
});
const api = vi.hoisted(() => ({
  cancelAssistantConversation: vi.fn(), continueAssistantConversation: vi.fn(),
  copyToClipboard: vi.fn(), getRecordingSnapshot: vi.fn(), hideSubtitleWindow: vi.fn(),
  openAssistantSource: vi.fn(), retryAssistantRequest: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: events.listen }));
vi.mock("@/api/tauri", () => api);
vi.mock("react-i18next", () => {
  const t = (key: string) => key;
  return { useTranslation: () => ({ t, i18n: { changeLanguage: vi.fn() } }) };
});
vi.mock("@/i18n", () => ({ default: { t: (key: string) => key, changeLanguage: vi.fn() } }));
vi.mock("@/components/MarkdownContent", () => ({
  default: ({ content }: { content: string }) => <div>{content}</div>,
}));
vi.mock("@/hooks/useSmoothText", () => ({
  useSmoothText: (source: string) => source,
  segmentGraphemes: (value: string) => Array.from(value),
}));

import SubtitleOverlay from "@/pages/SubtitleOverlay";

function deferred() {
  let resolve!: (value: string) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<string>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

async function renderOverlay() {
  const rendered = render(<SubtitleOverlay />);
  await act(async () => { await vi.dynamicImportSettled(); });
  return rendered;
}

async function showRetryableResult(sessionId: number, request = "Initial assistant request") {
  await act(async () => {
    events.emit("recording-state", {
      sessionId, revision: 1, phase: "processing", isStarting: false,
      isRecording: false, isProcessing: true, mode: "assistant",
    });
    events.emit("assistant-stream", { sessionId, status: "started", request });
    events.emit("assistant-stream", { sessionId, status: "search_error", message: "SEARCH_RATE_LIMITED" });
    events.emit("transcription-result", { sessionId, text: "Initial answer", interim: false, mode: "assistant" });
  });
}

async function settle(request: ReturnType<typeof deferred>, outcome: "resolve" | "reject", text = "Retired assistant answer") {
  await act(async () => {
    if (outcome === "resolve") request.resolve(text);
    else request.reject(new Error("Retired request failed"));
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  events.handlers.clear();
  api.getRecordingSnapshot.mockResolvedValue(null);
  api.cancelAssistantConversation.mockResolvedValue(false);
  api.hideSubtitleWindow.mockResolvedValue(undefined);
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: () => ({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() }),
  });
});
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); });

describe("initial assistant retry ownership", () => {
  it.each(["resolve", "reject"] as const)("ignores a retired retry %s after new dictation starts", async (outcome) => {
    const retry = deferred();
    api.retryAssistantRequest.mockReturnValueOnce(retry.promise);
    const { container } = await renderOverlay();
    await showRetryableResult(106);
    fireEvent.click(screen.getByRole("button", { name: "common.retry" }));
    expect(api.retryAssistantRequest).toHaveBeenCalledWith({ sessionId: 106, request: "Initial assistant request" });

    await act(async () => {
      events.emit("recording-state", {
        sessionId: 107, revision: 1, phase: "recording", isStarting: false,
        isRecording: true, isProcessing: false, mode: "dictation",
      });
      events.emit("transcription-result", { sessionId: 107, text: "New dictation preview", interim: true, mode: "dictation" });
    });
    expect(container.querySelector(".subtitle-waveform-indicator")).not.toBeNull();
    expect(container.querySelector(".subtitle-text")).toHaveTextContent("New dictation preview");

    await settle(retry, outcome);

    expect.soft(container.querySelector(".subtitle-waveform-indicator")).not.toBeNull();
    expect.soft(container.querySelector(".subtitle-text")).toHaveTextContent("New dictation preview");
    expect.soft(screen.queryByText("Retired assistant answer")).not.toBeInTheDocument();
    expect.soft(screen.queryByText("subtitle.conversation.searchFailed")).not.toBeInTheDocument();
  });

  it.each(["resolve", "reject"] as const)("keeps a closed overlay retired after retry %s in the same session", async (outcome) => {
    const retry = deferred();
    api.retryAssistantRequest.mockReturnValueOnce(retry.promise);
    const { container } = await renderOverlay();
    await showRetryableResult(116);
    fireEvent.click(screen.getByRole("button", { name: "common.retry" }));
    // A streamed final can expose the dismissible result before the invoke settles.
    await act(async () => {
      events.emit("transcription-result", { sessionId: 116, text: "Streamed result", interim: false, mode: "assistant" });
    });
    fireEvent.click(container.querySelector(".subtitle-root")!);
    expect(api.hideSubtitleWindow).toHaveBeenCalledOnce();
    expect(container.querySelector(".subtitle-text")).toBeNull();

    await settle(retry, outcome);

    expect.soft(container.querySelector(".subtitle-text")).toBeNull();
    expect.soft(container.querySelector(".subtitle-root-interactive")).toBeNull();
    expect.soft(screen.queryByText("subtitle.conversation.searchFailed")).not.toBeInTheDocument();
  });

  it.each(["resolve", "reject"] as const)("does not let old retry %s clear a newer same-session retry's busy state", async (outcome) => {
    const oldRetry = deferred();
    const currentRetry = deferred();
    api.retryAssistantRequest.mockReturnValueOnce(oldRetry.promise).mockReturnValueOnce(currentRetry.promise);
    const { container } = await renderOverlay();
    await showRetryableResult(126);
    fireEvent.click(screen.getByRole("button", { name: "common.retry" }));
    await act(async () => {
      events.emit("transcription-result", { sessionId: 126, text: "First streamed result", interim: false, mode: "assistant" });
    });
    fireEvent.click(container.querySelector(".subtitle-root")!);
    await showRetryableResult(126, "Second request in the same session");
    fireEvent.click(screen.getByRole("button", { name: "common.retry" }));
    expect(api.retryAssistantRequest).toHaveBeenCalledTimes(2);
    await act(async () => {
      events.emit("assistant-stream", { sessionId: 126, status: "search_error", message: "SEARCH_RATE_LIMITED" });
      events.emit("transcription-result", { sessionId: 126, text: "Current streamed result", interim: false, mode: "assistant" });
    });
    expect(screen.getByRole("button", { name: "subtitle.conversation.retrying" })).toBeDisabled();

    await settle(oldRetry, outcome);

    expect.soft(container.querySelector(".subtitle-text")).toHaveTextContent("Current streamed result");
    expect.soft(screen.queryByRole("button", { name: "subtitle.conversation.retrying" })).toBeDisabled();
    expect.soft(screen.queryByText("subtitle.conversation.searchFailed")).not.toBeInTheDocument();
    await settle(currentRetry, "resolve", "Current retry completed");
    expect(container.querySelector(".subtitle-text")).toHaveTextContent("Current retry completed");
    expect(screen.getByRole("button", { name: "common.retry" })).toBeEnabled();
  });

  it.each(["resolve", "reject"] as const)("settles the currently owned retry on %s", async (outcome) => {
    const retry = deferred();
    api.retryAssistantRequest.mockReturnValueOnce(retry.promise);
    const { container } = await renderOverlay();
    await showRetryableResult(136);
    fireEvent.click(screen.getByRole("button", { name: "common.retry" }));
    await settle(retry, outcome, "Current retry completed");
    if (outcome === "resolve") {
      expect(container.querySelector(".subtitle-text")).toHaveTextContent("Current retry completed");
      expect(screen.queryByText("subtitle.conversation.searchFailed")).not.toBeInTheDocument();
    } else {
      expect(screen.getByText("subtitle.conversation.searchFailed")).toBeVisible();
      expect(screen.getByRole("button", { name: "common.retry" })).toBeEnabled();
    }
  });

  it.each([
    ["close", "resolve"], ["close", "reject"],
    ["followup", "resolve"], ["followup", "reject"],
  ] as const)("conversation %s does not strand a pending initial retry on %s", async (conversationAction, outcome) => {
    const retry = deferred();
    const nextRetry = deferred();
    const followup = deferred();
    api.retryAssistantRequest.mockReturnValueOnce(retry.promise).mockReturnValueOnce(nextRetry.promise);
    api.continueAssistantConversation.mockReturnValueOnce(followup.promise);
    const { container } = await renderOverlay();
    await showRetryableResult(146);
    fireEvent.click(screen.getByRole("button", { name: "common.retry" }));
    await act(async () => {
      events.emit("assistant-stream", { sessionId: 146, status: "search_error", message: "SEARCH_RATE_LIMITED" });
      events.emit("transcription-result", { sessionId: 146, text: "Streamed retry result", interim: false, mode: "assistant" });
    });
    expect(screen.getByRole("button", { name: "subtitle.conversation.retrying" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "subtitle.conversation.open" }));

    if (conversationAction === "close") {
      fireEvent.click(screen.getByRole("button", { name: "subtitle.conversation.close" }));
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    } else {
      const input = screen.getByRole("textbox", { name: "subtitle.conversation.placeholder" });
      fireEvent.change(input, { target: { value: "Follow-up while retry settles" } });
      fireEvent.click(screen.getByRole("button", { name: "subtitle.conversation.send" }));
      expect(api.continueAssistantConversation).toHaveBeenCalledWith(expect.objectContaining({
        sessionId: 146, initialRequest: "Initial assistant request", initialResponse: "Streamed retry result",
        message: "Follow-up while retry settles",
      }));
      expect(screen.getByRole("dialog")).toBeVisible();
    }

    await settle(retry, outcome, "Current retry completed after conversation action");
    if (conversationAction === "followup") {
      // Keep the follow-up pending until after the initial retry has settled:
      // submitting a turn must independently preserve that retry's ownership.
      fireEvent.click(screen.getByRole("button", { name: "subtitle.conversation.close" }));
      await settle(followup, "resolve", "Closed follow-up completed");
    }

    expect.soft(screen.queryByRole("button", { name: "common.retry" })).toBeEnabled();
    expect.soft(screen.queryByRole("button", { name: "subtitle.conversation.retrying" })).not.toBeInTheDocument();
    if (outcome === "resolve") {
      expect.soft(container.querySelector(".subtitle-text")).toHaveTextContent("Current retry completed after conversation action");
    } else {
      expect.soft(screen.queryByText("subtitle.conversation.searchFailed")).toBeVisible();
    }
    const retryButton = screen.queryByRole("button", { name: "common.retry" });
    if (retryButton) fireEvent.click(retryButton);
    expect(api.retryAssistantRequest).toHaveBeenCalledTimes(2);
    expect(api.retryAssistantRequest).toHaveBeenLastCalledWith({ sessionId: 146, request: "Initial assistant request" });
    await settle(nextRetry, "resolve", "Next retry completed");
    expect(container.querySelector(".subtitle-text")).toHaveTextContent("Next retry completed");
  });
});
