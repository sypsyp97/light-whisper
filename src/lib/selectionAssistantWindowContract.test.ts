import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const repoRoot = path.resolve(import.meta.dirname, "../..");
const tauriConfig = fs.readFileSync(
  path.join(repoRoot, "src-tauri", "tauri.conf.json"),
  "utf8",
);
const rustSources = [
  path.join(repoRoot, "src-tauri", "src", "lib.rs"),
  path.join(repoRoot, "src-tauri", "src", "commands", "window.rs"),
]
  .filter((file) => fs.existsSync(file))
  .map((file) => fs.readFileSync(file, "utf8"))
  .join("\n");
const windowDefinition = `${tauriConfig}\n${rustSources}`;
const toolbarMarker = windowDefinition.search(/selection[-_]toolbar/i);
const selectionToolbarDefinition =
  toolbarMarker < 0
    ? undefined
    : windowDefinition.slice(
        Math.max(0, toolbarMarker - 240),
        toolbarMarker + 1_800,
      );
const selectionServiceSource = fs.readFileSync(
  path.join(repoRoot, "src-tauri", "src", "services", "selection_service.rs"),
  "utf8",
);
const selectionCommandSource = fs.readFileSync(
  path.join(repoRoot, "src-tauri", "src", "commands", "selection.rs"),
  "utf8",
);
const selectionCapability = JSON.parse(
  fs.readFileSync(
    path.join(repoRoot, "src-tauri", "capabilities", "selection.json"),
    "utf8",
  ),
) as { permissions: string[] };

function functionBlock(source: string, signature: string): string {
  const start = source.indexOf(signature);
  if (start < 0) return "";
  const nextFunction = source.indexOf("\npub fn ", start + signature.length);
  const nextAsyncFunction = source.indexOf("\npub async fn ", start + signature.length);
  const nextPrivateFunction = source.indexOf("\nfn ", start + signature.length);
  const boundaries = [nextFunction, nextAsyncFunction, nextPrivateFunction].filter(
    (value) => value >= 0,
  );
  const end = boundaries.length > 0 ? Math.min(...boundaries) : source.length;
  return source.slice(start, end);
}

describe("selection toolbar native window contract", () => {
  it("defines a dedicated selection toolbar window", () => {
    expect(selectionToolbarDefinition).toBeDefined();
  });

  it("keeps the toolbar non-focusable so the source application's selection survives", () => {
    expect(selectionToolbarDefinition).toBeDefined();
    expect(selectionToolbarDefinition).toMatch(
      /focusable[\s\S]{0,80}false|focused[\s\S]{0,80}false/i,
    );
  });

  it("uses a frameless transparent always-on-top surface outside the taskbar", () => {
    expect(selectionToolbarDefinition).toBeDefined();
    expect(selectionToolbarDefinition).toMatch(/decorations[\s\S]{0,80}false/i);
    expect(selectionToolbarDefinition).toMatch(/transparent[\s\S]{0,80}true/i);
    expect(selectionToolbarDefinition).toMatch(/always_on_top|alwaysOnTop/i);
    expect(selectionToolbarDefinition).toMatch(/skip_taskbar|skipTaskbar/i);
  });
});

describe("selection toolbar native interaction commands", () => {
  it("allows the selection window to use Tauri's native drag operation", () => {
    expect(selectionCapability.permissions).toContain(
      "core:window:allow-start-dragging",
    );
  });

  it("registers a dedicated selection-window drag command", () => {
    expect(selectionCommandSource).toMatch(
      /(?:async\s+)?fn\s+start_selection_window_drag\b/,
    );
    expect(rustSources).toMatch(/commands::selection::start_selection_window_drag/);
  });

  it("preserves the dragged window position when the result area expands", () => {
    const resizeService = functionBlock(
      selectionServiceSource,
      "pub fn set_selection_window_expanded",
    );

    expect(resizeService).toContain("outer_position");
    expect(resizeService).not.toContain("last_anchor");
  });

  it("enables native resizing only for the expanded result window", () => {
    const resizeService = functionBlock(
      selectionServiceSource,
      "pub fn set_selection_window_expanded",
    );

    expect(resizeService).toContain("set_resizable(expanded)");
    expect(resizeService).toContain("set_min_size");
  });

  it("enters the Win32 move loop synchronously after releasing pointer capture", () => {
    const dragService = functionBlock(
      selectionServiceSource,
      "pub fn start_selection_window_drag",
    );
    const releaseCapture = dragService.indexOf("ReleaseCapture");
    const sendMessage = dragService.indexOf("SendMessageW");

    expect(releaseCapture).toBeGreaterThanOrEqual(0);
    expect(sendMessage).toBeGreaterThan(releaseCapture);
    expect(dragService).toContain("WM_NCLBUTTONDOWN");
    expect(dragService).toContain("HTCAPTION");
    expect(dragService).not.toContain("PostMessageW");
  });

  it("keeps the transparent toolbar hit-testable", () => {
    expect(selectionServiceSource).not.toContain("WS_EX_TRANSPARENT");
    expect(selectionServiceSource).not.toMatch(
      /set_ignore_cursor_events\s*\(\s*true\s*\)/,
    );
  });

  it("does not silently swallow every native and Tauri hide failure", () => {
    const hideService = functionBlock(
      selectionServiceSource,
      "pub fn hide_selection_window",
    );
    const hideCommand = functionBlock(
      selectionCommandSource,
      "pub async fn hide_selection_assistant",
    );
    const hasWin32AndTauriPaths =
      /ShowWindow|SW_HIDE/.test(hideService) && /\.hide\(\)/.test(hideService);
    const explicitlyPropagatesFailure =
      /Result\s*</.test(hideService) &&
      /hide_selection_window\s*\([^)]*\)\s*(?:\?|\.map_err)/.test(hideCommand);

    expect(
      hasWin32AndTauriPaths || explicitlyPropagatesFailure,
      "hide must either use a verifiable Win32 path with Tauri fallback, or propagate the Tauri error to the IPC caller",
    ).toBe(true);
  });
});
