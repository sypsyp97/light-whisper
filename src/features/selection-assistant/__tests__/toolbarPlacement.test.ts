import { describe, expect, it } from "vitest";

import { computeToolbarPlacement } from "../toolbarPlacement";

const primary = {
  id: "primary",
  workArea: { x: 0, y: 0, width: 1920, height: 1040 },
};

describe("computeToolbarPlacement", () => {
  it("places a forward drag below and near the release point", () => {
    expect(
      computeToolbarPlacement({
        anchor: { x: 700, y: 300 },
        dragStart: { x: 400, y: 280 },
        dragEnd: { x: 700, y: 300 },
        toolbarSize: { width: 320, height: 44 },
        displays: [primary],
        gap: 8,
      }),
    ).toEqual({ x: 540, y: 308, displayId: "primary", side: "below" });
  });

  it("places an upward selection above the release point", () => {
    expect(
      computeToolbarPlacement({
        anchor: { x: 650, y: 250 },
        dragStart: { x: 700, y: 500 },
        dragEnd: { x: 650, y: 250 },
        toolbarSize: { width: 320, height: 44 },
        displays: [primary],
        gap: 8,
      }),
    ).toEqual({ x: 490, y: 198, displayId: "primary", side: "above" });
  });

  it("flips above when there is no room below, then clamps horizontally", () => {
    expect(
      computeToolbarPlacement({
        anchor: { x: 1915, y: 1035 },
        dragStart: { x: 1800, y: 1000 },
        dragEnd: { x: 1915, y: 1035 },
        toolbarSize: { width: 320, height: 44 },
        displays: [primary],
        gap: 8,
      }),
    ).toEqual({ x: 1600, y: 983, displayId: "primary", side: "above" });
  });

  it("uses the work area of a negative-coordinate secondary display", () => {
    const left = {
      id: "left",
      workArea: { x: -1280, y: 24, width: 1280, height: 984 },
    };

    expect(
      computeToolbarPlacement({
        anchor: { x: -1200, y: 40 },
        dragStart: { x: -1240, y: 35 },
        dragEnd: { x: -1200, y: 40 },
        toolbarSize: { width: 320, height: 44 },
        displays: [primary, left],
        gap: 8,
      }),
    ).toEqual({ x: -1280, y: 48, displayId: "left", side: "below" });
  });

  it("chooses the nearest display for an anchor in the gap between monitors", () => {
    const right = {
      id: "right",
      workArea: { x: 2000, y: 0, width: 1600, height: 900 },
    };

    const placement = computeToolbarPlacement({
      anchor: { x: 1985, y: 400 },
      dragStart: { x: 1980, y: 400 },
      dragEnd: { x: 1985, y: 400 },
      toolbarSize: { width: 320, height: 44 },
      displays: [primary, right],
      gap: 8,
    });

    expect(placement.displayId).toBe("right");
    expect(placement.x).toBeGreaterThanOrEqual(2000);
  });

  it("pins an oversized toolbar to the work-area origin", () => {
    const tiny = {
      id: "tiny",
      workArea: { x: 100, y: 200, width: 200, height: 30 },
    };

    expect(
      computeToolbarPlacement({
        anchor: { x: 180, y: 210 },
        dragStart: { x: 160, y: 210 },
        dragEnd: { x: 180, y: 210 },
        toolbarSize: { width: 320, height: 44 },
        displays: [tiny],
        gap: 8,
      }),
    ).toEqual({ x: 100, y: 200, displayId: "tiny", side: "below" });
  });
});
