import { describe, expect, it } from "vitest";
import {
  formatAsrEngineDescription,
  getAsrEngineCapability,
} from "@/lib/asrEngineCapabilities";

describe("ASR engine capabilities", () => {
  it.each([
    ["qwen3-asr-0.6b", { execution: "local", final: true, interim: true, downloadSize: "850 MB" }],
    ["qwen3-asr-1.7b", { execution: "local", final: true, interim: true, downloadSize: "2.19 GB" }],
    ["glm-asr", { execution: "cloud", final: true, interim: false, downloadSize: null }],
    ["alibaba-asr", { execution: "cloud", final: true, interim: false, downloadSize: null }],
  ] as const)("%s exposes its product capability contract", (engineKey, expected) => {
    expect(getAsrEngineCapability(engineKey)).toEqual(expected);
  });

  it("shows a download size for every local model and leaves cloud descriptions unchanged", () => {
    expect(formatAsrEngineDescription("qwen3-asr-0.6b", "最快 · 推荐"))
      .toBe("850 MB · 最快 · 推荐");
    expect(formatAsrEngineDescription("qwen3-asr-1.7b", "更高精度"))
      .toBe("2.19 GB · 更高精度");
    expect(formatAsrEngineDescription("glm-asr", "智谱在线语音识别"))
      .toBe("智谱在线语音识别");
  });

  it("does not advertise the retired SenseVoice engine", () => {
    expect(getAsrEngineCapability("sensevoice")).toBeNull();
  });

  it("does not advertise the retired Whisper engine", () => {
    expect(getAsrEngineCapability("whisper")).toBeNull();
  });
});
