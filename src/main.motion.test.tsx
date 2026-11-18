import type { ReactNode } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { toastError, useProfileSaveNotifications } = vi.hoisted(() => ({
  toastError: vi.fn(),
  useProfileSaveNotifications: vi.fn(),
}));

vi.mock("react-dom/client", () => ({
  default: { createRoot: vi.fn(() => ({ render: vi.fn() })) },
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ label: "main" }),
}));
vi.mock("./contexts/RecordingContext", () => ({
  RecordingProvider: ({ children }: { children: ReactNode }) => children,
}));
vi.mock("./hooks/useTheme", () => ({ useTheme: vi.fn() }));
vi.mock("./hooks/useProfileSaveNotifications", () => ({ useProfileSaveNotifications }));
vi.mock("sonner", () => ({ Toaster: () => null, toast: { error: toastError } }));
vi.mock("./i18n", () => ({ default: { t: (key: string) => key } }));

type View = "main" | "settings" | "history";
type PageProps = { onNavigate: (target: View) => void; animClass?: string };

vi.mock("./pages/MainPage", () => ({
  default: ({ onNavigate, animClass }: PageProps) => (
    <main data-testid="main-page" className={animClass}>
      <button onClick={() => onNavigate("settings")}>open settings</button>
      <button onClick={() => onNavigate("history")}>open history</button>
    </main>
  ),
}));

function deferred() {
  let resolve!: () => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<void>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

function setReducedMotion(matches: boolean) {
  vi.stubGlobal("matchMedia", vi.fn().mockReturnValue({
    matches,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  }));
}

async function openApp(reducedMotion = false) {
  setReducedMotion(reducedMotion);
  const settings = deferred();
  const history = deferred();
  const settingsStarted = vi.fn();
  const historyStarted = vi.fn();
  vi.doMock("./pages/SettingsPage", async () => {
    settingsStarted();
    await settings.promise;
    return {
      default: ({ onNavigate, animClass }: PageProps) => (
        <main data-testid="settings-page" className={animClass}>
          <button onClick={() => onNavigate("main")}>back to main</button>
          <button onClick={() => onNavigate("history")}>open history</button>
        </main>
      ),
    };
  });
  vi.doMock("./pages/HistoryPage", async () => {
    historyStarted();
    await history.promise;
    return {
      default: ({ onNavigate, animClass }: PageProps) => (
        <main data-testid="history-page" className={animClass}>
          <button onClick={() => onNavigate("main")}>back to main</button>
          <button onClick={() => onNavigate("settings")}>open settings</button>
        </main>
      ),
    };
  });

  const { App } = await import("./main");
  render(<App />);
  return { settings, history, settingsStarted, historyStarted };
}

function click(name: string) {
  fireEvent.click(screen.getByRole("button", { name }));
}

async function settle(resolve: () => void) {
  await act(async () => {
    resolve();
    await vi.dynamicImportSettled();
  });
}

beforeEach(() => {
  vi.resetModules();
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  vi.clearAllMocks();
});

afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.doUnmock("./pages/SettingsPage");
  vi.doUnmock("./pages/HistoryPage");
});

describe("App page motion", () => {
  it("mounts profile-save notifications with the main app", async () => {
    await openApp();
    expect(useProfileSaveNotifications).toHaveBeenCalled();
  });

  it.each(["settings", "history"] as const)(
    "opens %s as soon as it loads and accepts an immediate back",
    async (target) => {
      const app = await openApp();
      click(`open ${target}`);
      expect(screen.getByTestId("main-page")).toBeVisible();

      await settle(app[target].resolve);

      expect(screen.getByTestId(`${target}-page`)).toBeVisible();
      expect(screen.getByTestId(`${target}-page`)).toHaveClass("page-enter-right");
      click("back to main");
      expect(screen.getByTestId("main-page")).toBeVisible();
      expect(screen.getByTestId("main-page")).toHaveClass("page-enter-left");
    },
  );

  it("commits the ready page before the browser can dispatch another navigation click", async () => {
    const { settings } = await openApp();
    click("open settings");
    const current = screen.getByTestId("main-page");
    expect(current).toBeVisible();

    // Observe both sides of the import callback without act or a host scheduler turn.
    const published = deferred();
    const mediaQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
    let before: HTMLElement | null = null;
    let sameTurn: HTMLElement | null = null;
    vi.mocked(window.matchMedia).mockImplementation(() => {
      before = screen.queryByTestId("main-page");
      queueMicrotask(() => {
        sameTurn = screen.queryByTestId("settings-page");
        published.resolve();
      });
      return mediaQuery;
    });

    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", false);
    settings.resolve();
    await published.promise;
    await vi.dynamicImportSettled();

    expect(before).toBe(current);
    expect(sameTurn).toBe(screen.getByTestId("settings-page"));
  });

  it("switches immediately and accepts an immediate back with reduced motion", async () => {
    const { settings } = await openApp(true);
    click("open settings");
    await settle(settings.resolve);
    expect(screen.getByTestId("settings-page")).toBeVisible();
    expect(screen.getByTestId("settings-page")).not.toHaveClass("page-enter-right");

    click("back to main");
    expect(screen.getByTestId("main-page")).toBeVisible();
    expect(screen.getByTestId("main-page")).not.toHaveClass("page-enter-left");
  });

  it.each(["resolve", "reject"] as const)(
    "keeps the newer choice when an obsolete import later settles by %s",
    async (obsolete) => {
      const { settings, history, settingsStarted, historyStarted } = await openApp();
      // Capture factory startup without advancing any animation or preload timer.
      await act(async () => {
        click("open settings");
        await new Promise<void>((resolve) => setImmediate(resolve));
        click("open history");
        await new Promise<void>((resolve) => setImmediate(resolve));
      });
      const pendingCurrentPage = screen.getByTestId("main-page");
      const attemptedBoth = settingsStarted.mock.calls.length === 1 && historyStarted.mock.calls.length === 1;
      await act(async () => {
        history.resolve();
        if (attemptedBoth) {
          await import("./pages/HistoryPage");
          await new Promise<void>((resolve) => setImmediate(resolve));
        }
      });
      const committedNewest = screen.queryByTestId("history-page");

      await settle(() => {
        if (obsolete === "resolve") settings.resolve();
        else settings.reject(new Error("Obsolete settings import"));
      });

      expect(pendingCurrentPage).toHaveTextContent("open settings");
      expect(settingsStarted).toHaveBeenCalledTimes(1);
      expect(historyStarted).toHaveBeenCalledTimes(1);
      expect(committedNewest).toBeInTheDocument();
      expect(screen.getByTestId("history-page")).toBe(committedNewest);
      expect(screen.queryByTestId("settings-page")).toBeNull();
      expect(toastError).not.toHaveBeenCalled();
    },
  );

  it("reports the current import failure and keeps the current page ready for another choice", async () => {
    const { settings, history } = await openApp(true);
    const current = screen.getByTestId("main-page");
    click("open settings");
    await settle(() => settings.reject(new Error("Settings import failed")));

    expect(screen.getByTestId("main-page")).toBe(current);
    click("open history");
    await settle(history.resolve);
    expect(screen.getByTestId("history-page")).toBeVisible();
    expect(toastError).toHaveBeenCalledTimes(1);
  });
});
