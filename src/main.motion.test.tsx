import type { ReactNode } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("react-dom/client", () => ({
  default: {
    createRoot: vi.fn(() => ({ render: vi.fn() })),
  },
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ label: "main" }),
}));

vi.mock("./contexts/RecordingContext", () => ({
  RecordingProvider: ({ children }: { children: ReactNode }) => children,
}));

vi.mock("./hooks/useTheme", () => ({ useTheme: vi.fn() }));
vi.mock("sonner", () => ({ Toaster: () => null }));
vi.mock("./i18n", () => ({ default: { t: (key: string) => key } }));

vi.mock("./pages/MainPage", () => ({
  default: ({
    onNavigate,
    animClass,
  }: {
    onNavigate: (target: "settings") => void;
    animClass: string;
  }) => (
    <main data-testid="main-page" className={animClass}>
      <button onClick={() => onNavigate("settings")}>open settings</button>
    </main>
  ),
}));

vi.mock("./pages/SettingsPage", () => ({
  default: ({
    onNavigate,
    animClass,
  }: {
    onNavigate: (target: "main") => void;
    animClass: string;
  }) => (
    <main data-testid="settings-page" className={animClass}>
      <button onClick={() => onNavigate("main")}>back to main</button>
    </main>
  ),
}));

import { App } from "./main";

function setReducedMotion(matches: boolean) {
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: vi.fn().mockReturnValue({
      matches,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }),
  });
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.runOnlyPendingTimers();
  vi.useRealTimers();
});

describe("App page motion", () => {
  it("switches immediately and stays unlocked when reduced motion is enabled", async () => {
    setReducedMotion(true);
    render(<App />);

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "open settings" }));
    });

    expect(screen.getByTestId("settings-page")).toHaveAttribute("class", "");

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "back to main" }));
    });

    expect(screen.getByTestId("main-page")).toHaveAttribute("class", "");
  });

  it("keeps the directional exit and entrance rhythm with regular motion", async () => {
    setReducedMotion(false);
    render(<App />);

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "open settings" }));
    });
    expect(screen.getByTestId("main-page")).toHaveClass("page-exit-left");

    act(() => vi.advanceTimersByTime(139));
    expect(screen.getByTestId("main-page")).toBeInTheDocument();

    act(() => vi.advanceTimersByTime(1));
    expect(screen.getByTestId("settings-page")).toHaveClass("page-enter-right");

    act(() => vi.advanceTimersByTime(180));
    expect(screen.getByTestId("settings-page")).toHaveAttribute("class", "");
  });
});
