import { describe, expect, it } from "vitest";
import { getAsrEngineCapability } from "@/lib/asrEngineCapabilities";

describe("ASR engine capabilities", () => {
  it.each([
    ["whisper", { execution: "local", final: true, interim: true }],
    ["qwen3-asr-0.6b", { execution: "local", final: true, interim: true }],
    ["qwen3-asr-1.7b", { execution: "local", final: true, interim: true }],
    ["glm-asr", { execution: "cloud", final: true, interim: false }],
    ["alibaba-asr", { execution: "cloud", final: true, interim: false }],
  ] as const)("%s exposes its product capability contract", (engineKey, expected) => {
    expect(getAsrEngineCapability(engineKey)).toEqual(expected);
  });

  it("does not advertise the retired SenseVoice engine", () => {
    expect(getAsrEngineCapability("sensevoice")).toBeNull();
  });
});
