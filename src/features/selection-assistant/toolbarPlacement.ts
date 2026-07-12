export interface Point { x: number; y: number }
export interface Rectangle extends Point { width: number; height: number }
export interface DisplayArea { id: string; workArea: Rectangle }

interface PlacementInput {
  anchor: Point;
  dragStart: Point;
  dragEnd: Point;
  toolbarSize: { width: number; height: number };
  displays: DisplayArea[];
  gap: number;
}

function distanceSquaredToArea(point: Point, area: Rectangle): number {
  const dx = point.x < area.x
    ? area.x - point.x
    : point.x > area.x + area.width
      ? point.x - (area.x + area.width)
      : 0;
  const dy = point.y < area.y
    ? area.y - point.y
    : point.y > area.y + area.height
      ? point.y - (area.y + area.height)
      : 0;
  return dx * dx + dy * dy;
}

export function computeToolbarPlacement(input: PlacementInput) {
  if (input.displays.length === 0) {
    throw new Error("At least one display is required");
  }
  const display = input.displays.reduce((nearest, candidate) => (
    distanceSquaredToArea(input.anchor, candidate.workArea)
      < distanceSquaredToArea(input.anchor, nearest.workArea)
      ? candidate
      : nearest
  ));
  const area = display.workArea;
  const gap = Math.max(0, input.gap);
  const width = Math.max(0, input.toolbarSize.width);
  const height = Math.max(0, input.toolbarSize.height);
  let side: "above" | "below" = input.dragEnd.y < input.dragStart.y ? "above" : "below";
  const belowY = input.anchor.y + gap;
  const aboveY = input.anchor.y - height - gap;
  if (side === "below" && belowY + height > area.y + area.height && aboveY >= area.y) {
    side = "above";
  } else if (side === "above" && aboveY < area.y && belowY + height <= area.y + area.height) {
    side = "below";
  }

  const maxX = Math.max(area.x, area.x + area.width - width);
  const maxY = Math.max(area.y, area.y + area.height - height);
  const x = Math.min(maxX, Math.max(area.x, input.anchor.x - width / 2));
  const desiredY = side === "below" ? belowY : aboveY;
  const y = Math.min(maxY, Math.max(area.y, desiredY));
  return { x: Math.round(x), y: Math.round(y), displayId: display.id, side };
}
