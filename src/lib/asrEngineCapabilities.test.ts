import { describe, expect, it } from "vitest";
import { getAsrEngineCapability } from "@/lib/asrEngineCapabilities";

describe("ASR engine capabilities", () => {
  it.each([
    ["sensevoice", { execution: "local", final: true, interim: true }],
    ["whisper", { execution: "local", final: true, interim: true }],
    ["glm-asr", { execution: "cloud", final: true, interim: false }],
    ["alibaba-asr", { execution: "cloud", final: true, interim: false }],
  ] as const)("%s exposes its product capability contract", (engineKey, expected) => {
    expect(getAsrEngineCapability(engineKey)).toEqual(expected);
  });
});
