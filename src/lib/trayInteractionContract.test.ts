import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

function readTraySetup(): string {
  const source = readFileSync(resolve("src-tauri/src/lib.rs"), "utf8");
  const start = source.indexOf("fn setup_system_tray");
  if (start < 0) {
    throw new Error("setup_system_tray was not found");
  }
  return source.slice(start);
}

describe("tray interaction contract", () => {
  it("opens the main window on left release and reserves the native menu for right click", () => {
    const setup = readTraySetup();

    expect(setup).toContain(".menu(&menu)");
    expect(setup).toContain(".show_menu_on_left_click(false)");
    expect(setup).toMatch(
      /button:\s*MouseButton::Left,[\s\S]*button_state:\s*MouseButtonState::Up,[\s\S]*focus_main_window\(tray\.app_handle\(\)\)/,
    );
    expect(setup).not.toContain("toggle_main_window");
    expect(setup).not.toContain("tray-menu");
  });

  it("keeps all native menu actions and cleans up before quitting", () => {
    const setup = readTraySetup();

    expect(setup).toMatch(/"show"\s*=>\s*focus_main_window\(app\)/);
    expect(setup).toMatch(/"hide"\s*=>\s*hide_main_window\(app\)/);
    expect(setup).toMatch(
      /"quit"\s*=>[\s\S]*stop_funasr_on_exit\(app\);[\s\S]*app\.exit\(0\)/,
    );
  });
});
