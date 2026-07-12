import { describe, expect, it } from "vitest";

import {
  initialScreenshotSession,
  reduceScreenshotSession,
} from "../screenshotSession";

describe("reduceScreenshotSession", () => {
  it("starts exactly one capture and ignores conflicting starts", () => {
    const capturing = reduceScreenshotSession(initialScreenshotSession, {
      type: "start",
      captureId: "capture-1",
    });

    expect(capturing).toEqual({ status: "capturing", captureId: "capture-1" });
    expect(
      reduceScreenshotSession(capturing, {
        type: "start",
        captureId: "capture-2",
      }),
    ).toBe(capturing);
  });

  it("cancels capture cleanly and ignores a late OS completion", () => {
    const capturing = reduceScreenshotSession(initialScreenshotSession, {
      type: "start",
      captureId: "capture-1",
    });
    const cancelled = reduceScreenshotSession(capturing, {
      type: "cancel",
      captureId: "capture-1",
    });

    expect(cancelled).toEqual({ status: "idle" });
    expect(
      reduceScreenshotSession(cancelled, {
        type: "captured",
        captureId: "capture-1",
        imageBase64: "late-image",
      }),
    ).toBe(cancelled);
  });

  it("ignores stale OCR from an earlier capture", () => {
    const capturing = {
      status: "processing" as const,
      captureId: "capture-2",
      imageBase64: "new-image",
    };

    expect(
      reduceScreenshotSession(capturing, {
        type: "ocr_complete",
        captureId: "capture-1",
        text: "stale text",
      }),
    ).toBe(capturing);
  });

  it("keeps both OCR text and the screenshot so a vision-capable model can use either", () => {
    const state = reduceScreenshotSession(
      {
        status: "processing",
        captureId: "capture-1",
        imageBase64: "image-data",
      },
      {
        type: "ocr_complete",
        captureId: "capture-1",
        text: " recognized text ",
      },
    );

    expect(state).toEqual({
      status: "ready",
      captureId: "capture-1",
      imageBase64: "image-data",
      recognizedText: "recognized text",
    });
  });

  it("retains the screenshot when OCR finds no text", () => {
    const state = reduceScreenshotSession(
      {
        status: "processing",
        captureId: "capture-1",
        imageBase64: "image-data",
      },
      { type: "ocr_complete", captureId: "capture-1", text: "  " },
    );

    expect(state).toEqual({
      status: "ready",
      captureId: "capture-1",
      imageBase64: "image-data",
      recognizedText: undefined,
    });
  });

  it("returns to idle after an OS-level capture cancellation", () => {
    expect(
      reduceScreenshotSession(
        { status: "capturing", captureId: "capture-1" },
        { type: "capture_cancelled", captureId: "capture-1" },
      ),
    ).toEqual({ status: "idle" });
  });
});
